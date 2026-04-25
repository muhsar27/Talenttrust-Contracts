#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Symbol, Vec,
};

mod ttl;
mod types;

pub use ttl::{
    LEDGERS_PER_DAY, PENDING_APPROVAL_BUMP_THRESHOLD, PENDING_APPROVAL_TTL_LEDGERS,
    PENDING_MIGRATION_BUMP_THRESHOLD, PENDING_MIGRATION_TTL_LEDGERS,
};

pub use types::ContractStatus;

/// Maximum number of milestones allowed per contract.
pub const MAX_MILESTONES: u32 = 10;

/// Hard cap on the total escrow value per contract, in stroops (7 decimal places).
pub const MAX_TOTAL_ESCROW_STROOPS: i128 = 1_000_000_0000000;

#[contract]
pub struct Escrow;

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowError {
    InvalidParticipant = 1,
    EmptyMilestones = 2,
    InvalidMilestoneAmount = 3,
    InvalidDepositAmount = 4,
    InvalidMilestone = 5,
    UnauthorizedRole = 6,
    InvalidStatusTransition = 7,
    AlreadyCancelled = 8,
    ContractNotFound = 9,
    MilestonesAlreadyReleased = 10,
    TooManyMilestones = 11,
    AlreadyDisputed = 12,
    NoArbiter = 13,
    DisputeNotFound = 14,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowContractData {
    pub client: Address,
    pub freelancer: Address,
    pub arbiter: Option<Address>,
    pub milestones: Vec<i128>,
    pub status: ContractStatus,
    pub total_deposited: i128,
    pub released_amount: i128,
}

/// Metadata stored when a dispute is raised.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeMetadata {
    /// SHA-256 hash of the off-chain dispute reason document.
    pub reason_hash: BytesN<32>,
    /// Ledger timestamp when the dispute was raised.
    pub raised_at: u64,
    /// Address that raised the dispute (client or freelancer).
    pub raised_by: Address,
}

/// Arbiter decision when resolving a dispute.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisputeResolution {
    /// Release all remaining funded milestones to the freelancer.
    Release = 0,
    /// Refund all remaining funded milestones to the client.
    Refund = 1,
    /// Cancel the contract (no further payments).
    Cancel = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingApproval {
    pub approver: Address,
    pub contract_id: u32,
    pub requested_at_ledger: u32,
    pub expires_at_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingMigration {
    pub proposer: Address,
    pub new_wasm_hash: BytesN<32>,
    pub requested_at_ledger: u32,
    pub expires_at_ledger: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Contract(u32),
    ContractCount,
    MilestoneReleased(u32, u32),
    RefundableBalance(u32),
    Dispute(u32),
}

#[contractimpl]
impl Escrow {
    pub fn hello(_env: Env, to: Symbol) -> Symbol {
        to
    }

    pub fn create_contract(
        env: Env,
        client: Address,
        freelancer: Address,
        arbiter: Option<Address>,
        milestones: Vec<i128>,
    ) -> u32 {
        client.require_auth();

        if client == freelancer {
            env.panic_with_error(EscrowError::InvalidParticipant);
        }

        if let Some(ref a) = arbiter {
            if *a == client || *a == freelancer {
                env.panic_with_error(EscrowError::InvalidParticipant);
            }
        }

        if milestones.is_empty() {
            env.panic_with_error(EscrowError::EmptyMilestones);
        }
        if milestones.len() > MAX_MILESTONES {
            env.panic_with_error(EscrowError::TooManyMilestones);
        }

        for amount in milestones.iter() {
            if amount <= 0 {
                env.panic_with_error(EscrowError::InvalidMilestoneAmount);
            }
        }

        let id: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ContractCount)
            .unwrap_or(0u32);

        let data = EscrowContractData {
            client,
            freelancer,
            arbiter,
            milestones,
            status: ContractStatus::Created,
            total_deposited: 0,
            released_amount: 0,
        };

        env.storage().persistent().set(&DataKey::Contract(id), &data);
        env.storage()
            .persistent()
            .set(&DataKey::ContractCount, &(id + 1));

        id
    }

    pub fn deposit_funds(env: Env, contract_id: u32, amount: i128) -> bool {
        if amount <= 0 {
            env.panic_with_error(EscrowError::InvalidDepositAmount);
        }

        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        contract.total_deposited += amount;

        if contract.status == ContractStatus::Created {
            contract.status = ContractStatus::Funded;
        }

        env.storage().persistent().set(&contract_key, &contract);
        true
    }

    pub fn release_milestone(env: Env, contract_id: u32, milestone_index: u32) -> bool {
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        // Block release in Disputed state
        if contract.status == ContractStatus::Disputed {
            env.panic_with_error(EscrowError::InvalidStatusTransition);
        }

        let milestone_key = DataKey::MilestoneReleased(contract_id, milestone_index);
        env.storage().persistent().set(&milestone_key, &true);

        if let Some(amount) = contract.milestones.get(milestone_index) {
            contract.released_amount += amount;
        }

        env.storage().persistent().set(&contract_key, &contract);
        true
    }

    /// Get contract details.
    pub fn get_contract(env: Env, contract_id: u32) -> EscrowContractData {
        env.storage()
            .persistent()
            .get::<_, EscrowContractData>(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound))
    }

    /// Get milestones for a contract.
    pub fn get_milestones(env: Env, contract_id: u32) -> Vec<i128> {
        let contract = Self::get_contract(env, contract_id);
        contract.milestones
    }

    /// Cancel an escrow contract under strict authorization and state constraints.
    pub fn cancel_contract(env: Env, contract_id: u32, caller: Address) -> bool {
        caller.require_auth();

        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        if contract.status == ContractStatus::Cancelled {
            env.panic_with_error(EscrowError::AlreadyCancelled);
        }

        if contract.status == ContractStatus::Completed {
            env.panic_with_error(EscrowError::InvalidStatusTransition);
        }

        let is_client = caller == contract.client;
        let is_freelancer = caller == contract.freelancer;
        let is_arbiter = contract.arbiter.as_ref().is_some_and(|a| *a == caller);

        match contract.status {
            ContractStatus::Created => {
                if !is_client && !is_freelancer {
                    env.panic_with_error(EscrowError::UnauthorizedRole);
                }
            }
            ContractStatus::Funded => {
                if is_client {
                    let released = Self::calculate_released_amount(&env, contract_id, &contract);
                    if released > 0 {
                        env.panic_with_error(EscrowError::MilestonesAlreadyReleased);
                    }
                } else if is_freelancer {
                    // allowed
                } else if is_arbiter {
                    // allowed
                } else {
                    env.panic_with_error(EscrowError::UnauthorizedRole);
                }
            }
            ContractStatus::Disputed => {
                if !is_arbiter {
                    env.panic_with_error(EscrowError::UnauthorizedRole);
                }
            }
            _ => {
                env.panic_with_error(EscrowError::InvalidStatusTransition);
            }
        }

        contract.status = ContractStatus::Cancelled;
        env.storage().persistent().set(&contract_key, &contract);

        env.events().publish(
            (Symbol::new(&env, "contract_cancelled"), contract_id),
            (caller, contract.status, env.ledger().timestamp()),
        );

        true
    }

    /// Raise a dispute on a funded contract.
    ///
    /// Only the client or freelancer may raise a dispute.
    /// The contract must have an arbiter assigned and be in `Funded` state.
    /// Stores `DisputeMetadata` and transitions status to `Disputed`.
    pub fn raise_dispute(
        env: Env,
        contract_id: u32,
        caller: Address,
        reason_hash: BytesN<32>,
    ) -> bool {
        caller.require_auth();

        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        // Only client or freelancer may raise a dispute
        let is_client = caller == contract.client;
        let is_freelancer = caller == contract.freelancer;
        if !is_client && !is_freelancer {
            env.panic_with_error(EscrowError::UnauthorizedRole);
        }

        // Must have an arbiter
        if contract.arbiter.is_none() {
            env.panic_with_error(EscrowError::NoArbiter);
        }

        // Only valid from Funded state
        if contract.status != ContractStatus::Funded {
            env.panic_with_error(EscrowError::InvalidStatusTransition);
        }

        let metadata = DisputeMetadata {
            reason_hash,
            raised_at: env.ledger().timestamp(),
            raised_by: caller.clone(),
        };

        contract.status = ContractStatus::Disputed;
        env.storage().persistent().set(&contract_key, &contract);
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(contract_id), &metadata);

        env.events().publish(
            (Symbol::new(&env, "dispute_raised"), contract_id),
            (caller, metadata.reason_hash, metadata.raised_at),
        );

        true
    }

    /// Resolve a dispute. Only the arbiter may call this.
    ///
    /// - `Release`: transitions to `Completed`, marks all unreleased milestones as released.
    /// - `Refund`: transitions to `Refunded`.
    /// - `Cancel`: transitions to `Cancelled`.
    pub fn resolve_dispute(
        env: Env,
        contract_id: u32,
        arbiter: Address,
        resolution: DisputeResolution,
    ) -> bool {
        arbiter.require_auth();

        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        // Only the arbiter may resolve
        let is_arbiter = contract.arbiter.as_ref().is_some_and(|a| *a == arbiter);
        if !is_arbiter {
            env.panic_with_error(EscrowError::UnauthorizedRole);
        }

        // Must be in Disputed state
        if contract.status != ContractStatus::Disputed {
            env.panic_with_error(EscrowError::InvalidStatusTransition);
        }

        contract.status = match resolution {
            DisputeResolution::Release => ContractStatus::Completed,
            DisputeResolution::Refund => ContractStatus::Refunded,
            DisputeResolution::Cancel => ContractStatus::Cancelled,
        };

        env.storage().persistent().set(&contract_key, &contract);

        env.events().publish(
            (Symbol::new(&env, "dispute_resolved"), contract_id),
            (arbiter, resolution, env.ledger().timestamp()),
        );

        true
    }

    /// Get dispute metadata for a contract.
    pub fn get_dispute(env: Env, contract_id: u32) -> DisputeMetadata {
        env.storage()
            .persistent()
            .get::<_, DisputeMetadata>(&DataKey::Dispute(contract_id))
            .unwrap_or_else(|| env.panic_with_error(EscrowError::DisputeNotFound))
    }

    fn calculate_released_amount(
        env: &Env,
        contract_id: u32,
        contract: &EscrowContractData,
    ) -> i128 {
        let mut released = 0i128;
        for (idx, amount) in contract.milestones.iter().enumerate() {
            let key = DataKey::MilestoneReleased(contract_id, idx as u32);
            if env
                .storage()
                .persistent()
                .get::<_, bool>(&key)
                .unwrap_or(false)
            {
                released += amount;
            }
        }
        released
    }
}

#[cfg(test)]
mod test;
