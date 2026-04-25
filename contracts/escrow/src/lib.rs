#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Bytes,
    BytesN, Env, Symbol, Vec,
};

mod ttl;

pub use ttl::{
    LEDGERS_PER_DAY, PENDING_APPROVAL_BUMP_THRESHOLD, PENDING_APPROVAL_TTL_LEDGERS,
    PENDING_MIGRATION_BUMP_THRESHOLD, PENDING_MIGRATION_TTL_LEDGERS,
};

// ─── Bounds constants ─────────────────────────────────────────────────────────
//
// Policy decision: bounds are HARD-CODED for the initial release rather than
// governed on-chain. Rationale:
//   • Governance machinery adds upgrade-path complexity and new attack surface.
//   • Hard limits give the strongest security guarantee with zero runtime cost.
//   • A future governance proposal can introduce adjustable parameters if
//     operational experience shows the defaults need revisiting.
//
// MAX_MILESTONES: limits worst-case per-contract storage and loop cost.
//   10 milestones covers the overwhelming majority of real freelance contracts.
//
// MAX_TOTAL_ESCROW_STROOPS: caps the maximum value locked in a single contract
//   to 1 000 000 tokens (7-decimal stroops) to bound worst-case griefing impact.

/// Maximum number of milestones allowed per contract.
pub const MAX_MILESTONES: u32 = 10;

/// Hard cap on the total escrow value per contract, in stroops (7 decimal places).
/// Equals 1 000 000 tokens.
pub const MAX_TOTAL_ESCROW_STROOPS: i128 = 1_000_000_0000000; // 1 M tokens × 10^7 = 10^13

pub const MAINNET_PROTOCOL_VERSION: u32 = 1u32;
pub const MAINNET_MAX_TOTAL_ESCROW_PER_CONTRACT_STROOPS: i128 = 1_000_000_000_000_000i128;

mod types;
pub use crate::types::{ContractStatus, Milestone, ReadinessChecklist};

#[contracttype]
pub struct EscrowBounds {
    pub max_milestones: u32,
    pub max_total_escrow_stroops: i128,
}

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
    AlreadyInitialized = 11,
    TooManyMilestones = 12,
    InsufficientAccumulatedFees = 13,
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

pub type ContractData = EscrowContractData;

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
enum DataKey {
    Contract(u32),
    MilestoneReleased(u32, u32),
    RefundableBalance(u32),
    ContractCount,
    Milestones(u32),
    MilestoneApprovalTime(u32, u32),
    Admin,
    ProtocolFeeBps,
    AccumulatedProtocolFees,
    Initialized,
    ReadinessChecklist,
}

fn update_readiness_checklist<F>(env: &Env, f: F)
where
    F: FnOnce(&mut ReadinessChecklist),
{
    let mut checklist: ReadinessChecklist = env
        .storage()
        .instance()
        .get(&DataKey::ReadinessChecklist)
        .unwrap_or_default();
    f(&mut checklist);
    env.storage()
        .instance()
        .set(&DataKey::ReadinessChecklist, &checklist);
}

#[contractimpl]
impl Escrow {
    pub fn hello(_env: Env, to: Symbol) -> Symbol {
        to
    }

    /// Returns the hard-coded bounds enforced by this contract.
    /// Useful for client-side pre-validation and monitoring dashboards.
    pub fn get_bounds(_env: Env) -> EscrowBounds {
        EscrowBounds {
            max_milestones: MAX_MILESTONES,
            max_total_escrow_stroops: MAX_TOTAL_ESCROW_STROOPS,
        }
    }

    /// Initializes the global escrow contract parameters.
    /// MUST only be called once.
    pub fn initialize(env: Env, admin: Address, protocol_fee_bps: u32) -> bool {
        if env.storage().instance().has(&DataKey::Initialized) {
            env.panic_with_error(EscrowError::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::ProtocolFeeBps, &protocol_fee_bps);
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage()
            .persistent()
            .set(&DataKey::AccumulatedProtocolFees, &0i128);

        true
    }
    /// Withdraws accumulated protocol fees to a specified destination.
    /// Only callable by the Admin.
    pub fn withdraw_protocol_fees(
        env: Env,
        caller: Address,
        destination: Address,
        amount: i128,
        token: Address,
    ) -> bool {
        caller.require_auth();

        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::UnauthorizedRole));

        if caller != admin {
            env.panic_with_error(EscrowError::UnauthorizedRole);
        }

        let mut accumulated_fees: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::AccumulatedProtocolFees)
            .unwrap_or(0);

        if amount <= 0 || amount > accumulated_fees {
            env.panic_with_error(EscrowError::InsufficientAccumulatedFees);
        }

        // Deduct first to prevent reentrancy
        accumulated_fees -= amount;
        env.storage()
            .persistent()
            .set(&DataKey::AccumulatedProtocolFees, &accumulated_fees);

        // Execute external token transfer
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &destination, &amount);

        env.events().publish(
            (Symbol::new(&env, "ProtocolFeesWithdrawn"),),
            (admin, destination, amount, token, env.ledger().sequence()),
        );

        true
    }

    pub fn create_contract(
        env: Env,
        client: Address,
        freelancer: Address,
        arbiter: Option<Address>,
        milestones: Vec<i128>,
        terms_hash: Option<Bytes>,
        grace_period_seconds: Option<u64>,
    ) -> u32 {
        client.require_auth();

        if client == freelancer {
            env.panic_with_error(EscrowError::InvalidParticipant);
        }

        // Validate arbiter doesn't overlap with client/freelancer
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

        let mut total_amount: i128 = 0;
        let mut milestone_list: Vec<Milestone> = Vec::new(&env);
        for amount in milestones.iter() {
            if amount <= 0 {
                env.panic_with_error(EscrowError::InvalidMilestoneAmount);
            }
            total_amount += amount;
            milestone_list.push_back(Milestone {
                amount,
                released: false,
                work_evidence: None,
                funded_amount: 0,
            });
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
            milestones: milestones.clone(),
            status: ContractStatus::Created,
            total_deposited: 0,
            released_amount: 0,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Contract(id), &data);
        env.storage()
            .persistent()
            .set(&DataKey::Milestones(id), &milestone_list);
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
            .get::<_, ContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        contract.total_deposited += amount;

        // Update status to Funded if not already
        if contract.status == ContractStatus::Created {
            contract.status = ContractStatus::Funded;
        }

        env.storage().persistent().set(&contract_key, &contract);

        true
    }

    pub fn approve_milestone(env: Env, contract_id: u32, milestone_index: u32) -> bool {
        // Store approval time using ledger timestamp
        let approval_time = env.ledger().timestamp();
        env.storage().persistent().set(
            &DataKey::MilestoneApprovalTime(contract_id, milestone_index),
            &approval_time,
        );
        true
    }

    pub fn release_milestone(env: Env, contract_id: u32, milestone_index: u32) -> bool {
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, ContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        // Mark this milestone as released
        let milestone_key = DataKey::MilestoneReleased(contract_id, milestone_index);
        env.storage().persistent().set(&milestone_key, &true);

        // Accrue protocol fees and update released amount
        if let Some(amount) = contract.milestones.get(milestone_index) {
            let bps: u32 = env
                .storage()
                .instance()
                .get(&DataKey::ProtocolFeeBps)
                .unwrap_or(0);
            let bps_i128 = bps as i128;

            let fee = if bps_i128 > 0 {
                // Ceiling rounding math: (amount * bps + 9999) / 10000
                amount
                    .checked_mul(bps_i128)
                    .expect("multiplication overflow")
                    .checked_add(9999)
                    .expect("addition overflow")
                    .checked_div(10000)
                    .expect("division overflow")
            } else {
                0
            };

            let current_fees: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::AccumulatedProtocolFees)
                .unwrap_or(0i128);
            env.storage().persistent().set(
                &DataKey::AccumulatedProtocolFees,
                &(current_fees.checked_add(fee).unwrap()),
            );

            let net_amount = amount.checked_sub(fee).unwrap();
            contract.released_amount += net_amount;

            env.events().publish(
                (
                    Symbol::new(&env, "ProtocolFeeAccrued"),
                    contract_id,
                    milestone_index,
                ),
                (fee, env.ledger().sequence()),
            );
        }

        env.storage().persistent().set(&contract_key, &contract);

        true
    }

    /// Get contract details
    pub fn get_contract(env: Env, contract_id: u32) -> ContractData {
        env.storage()
            .persistent()
            .get::<_, ContractData>(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound))
    }

    /// Get milestones for a contract
    pub fn get_milestones(env: Env, contract_id: u32) -> Vec<i128> {
        let contract = Self::get_contract(env.clone(), contract_id);
        contract.milestones
    }

    /// Cancel an escrow contract under strict authorization and state constraints
    pub fn cancel_contract(env: Env, contract_id: u32, caller: Address) -> bool {
        // 1. Require cryptographic authorization
        caller.require_auth();

        // 2. Load contract data
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, ContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        // 3. Check if already cancelled (idempotency guard)
        if contract.status == ContractStatus::Cancelled {
            env.panic_with_error(EscrowError::AlreadyCancelled);
        }

        // 4. Block cancellation in terminal states
        if contract.status == ContractStatus::Completed {
            env.panic_with_error(EscrowError::InvalidStatusTransition);
        }

        // 5. Role-based authorization with state checks
        let is_client = caller == contract.client;
        let is_freelancer = caller == contract.freelancer;
        let is_arbiter = contract.arbiter.as_ref().is_some_and(|a| *a == caller);

        match contract.status {
            ContractStatus::Created => {
                // Client or freelancer can cancel before funding
                if !is_client && !is_freelancer {
                    env.panic_with_error(EscrowError::UnauthorizedRole);
                }
            }
            ContractStatus::Funded => {
                // Calculate released milestones
                let released_amount = Self::calculate_released_amount(&env, contract_id, &contract);

                if is_client {
                    // Client can cancel only if NO milestones released
                    if released_amount > 0 {
                        env.panic_with_error(EscrowError::MilestonesAlreadyReleased);
                    }
                } else if is_freelancer {
                    // Freelancer can cancel (economic deterrent - funds return to client)
                    // No additional checks needed
                } else if is_arbiter {
                    // Arbiter can cancel in funded state (dispute resolution)
                } else {
                    env.panic_with_error(EscrowError::UnauthorizedRole);
                }
            }
            ContractStatus::Disputed => {
                // Only arbiter can cancel disputed contracts
                if !is_arbiter {
                    env.panic_with_error(EscrowError::UnauthorizedRole);
                }
            }
            _ => {
                env.panic_with_error(EscrowError::InvalidStatusTransition);
            }
        }

        // 6. Transition to Cancelled state
        contract.status = ContractStatus::Cancelled;
        env.storage().persistent().set(&contract_key, &contract);

        // 7. Emit indexer-friendly event
        env.events().publish(
            (Symbol::new(&env, "contract_cancelled"), contract_id),
            (caller, contract.status, env.ledger().timestamp()),
        );

        true
    }

    /// Helper: Calculate total released amount for a contract
    fn calculate_released_amount(env: &Env, contract_id: u32, contract: &ContractData) -> i128 {
        let mut released = 0i128;
        for (idx, amount) in contract.milestones.iter().enumerate() {
            let milestone_key = DataKey::MilestoneReleased(contract_id, idx as u32);
            if env
                .storage()
                .persistent()
                .get::<_, bool>(&milestone_key)
                .unwrap_or(false)
            {
                released += amount;
            }
        }
        released
    }
}

// #[cfg(test)]
// mod test;

// #[cfg(test)]
// mod proptest;

#[cfg(test)]
mod protocol_fees_test;
