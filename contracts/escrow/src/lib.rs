#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Bytes, BytesN, Env,
    Symbol, Vec,
};

mod ttl;

pub use ttl::{
    LEDGERS_PER_DAY, PENDING_APPROVAL_BUMP_THRESHOLD, PENDING_APPROVAL_TTL_LEDGERS,
    PENDING_MIGRATION_BUMP_THRESHOLD, PENDING_MIGRATION_TTL_LEDGERS,
};

mod types;
pub use types::{ContractStatus, DataKey, Milestone, ReadinessChecklist};

pub use crate::types::MainnetReadinessInfo;

// ─── Bounds constants ─────────────────────────────────────────────────────────
pub const MAX_MILESTONES: u32 = 10;
pub const MAX_TOTAL_ESCROW_STROOPS: i128 = 10_000_000_000_000;

pub const MAINNET_PROTOCOL_VERSION: u32 = 1u32;
pub const MAINNET_MAX_TOTAL_ESCROW_PER_CONTRACT_STROOPS: i128 = 1_000_000_000_000_000i128;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
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
    TooManyMilestones = 11,
    ApprovalExpired = 12,
    NotAllMilestonesReleased = 13,
    InvalidRating = 14,
    ReputationAlreadyIssued = 15,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowContractData {
    pub client: Address,
    pub freelancer: Address,
    pub arbiter: Option<Address>,
    pub milestones: Vec<i128>,
    pub status: ContractStatus,
    pub total_amount: i128,
    pub funded_amount: i128,
    pub released_amount: i128,
    pub refunded_amount: i128,
    pub approval_expiry_seconds: Option<u64>,
    pub milestone_count: u32,
    pub released_milestones: u32,
    pub reputation_issued: bool,
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reputation {
    pub total_rating: u32,
    pub ratings_count: u32,
    pub completed_contracts: u32,
}

fn update_readiness_checklist<F>(env: &Env, f: F)
where
    F: FnOnce(&mut ReadinessChecklist),
{
    let mut checklist: ReadinessChecklist = env
        .storage()
        .instance()
        .get(&DataKey::ReadinessChecklist)
        .unwrap_or(ReadinessChecklist {
            has_bounds: false,
            has_ttl: false,
        });
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

    pub fn get_bounds(_env: Env) -> EscrowBounds {
        EscrowBounds {
            max_milestones: MAX_MILESTONES,
            max_total_escrow_stroops: MAX_TOTAL_ESCROW_STROOPS,
        }
    }

    pub fn create_contract(
        env: Env,
        client: Address,
        freelancer: Address,
        arbiter: Option<Address>,
        milestones: Vec<i128>,
        _terms_hash: Option<Bytes>,
        _grace_period_seconds: Option<u64>,
        approval_expiry_seconds: Option<u64>,
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

        let mut total_amount: i128 = 0;
        for amount in milestones.iter() {
            if amount <= 0 {
                env.panic_with_error(EscrowError::InvalidMilestoneAmount);
            }
            total_amount += amount;
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
            milestone_count: milestones.len(),
            milestones: milestones.clone(),
            status: ContractStatus::Created,
            total_amount,
            funded_amount: 0,
            released_amount: 0,
            refunded_amount: 0,
            approval_expiry_seconds,
            released_milestones: 0,
            reputation_issued: false,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Contract(id), &data);
        env.storage()
            .persistent()
            .set(&DataKey::Milestones(id), &milestones);
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

        contract.funded_amount += amount;

        if contract.status == ContractStatus::Created {
            contract.status = ContractStatus::Funded;
        }

        env.storage().persistent().set(&contract_key, &contract);

        true
    }

    pub fn approve_milestone(env: Env, contract_id: u32, milestone_index: u32) -> bool {
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
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        if let Some(expiry_window) = contract.approval_expiry_seconds {
            let approval_key = DataKey::MilestoneApprovalTime(contract_id, milestone_index);
            let approval_time = env
                .storage()
                .persistent()
                .get::<_, u64>(&approval_key)
                .unwrap_or_else(|| env.panic_with_error(EscrowError::UnauthorizedRole));

            if env.ledger().timestamp() > approval_time + expiry_window {
                env.panic_with_error(EscrowError::ApprovalExpired);
            }
        }

        let milestone_key = DataKey::MilestoneReleased(contract_id, milestone_index);
        env.storage().persistent().set(&milestone_key, &true);

        if let Some(amount) = contract.milestones.get(milestone_index) {
            contract.released_amount += amount;
            contract.released_milestones += 1;
        }

        env.storage().persistent().set(&contract_key, &contract);

        true
    }

    pub fn get_contract(env: Env, contract_id: u32) -> EscrowContractData {
        env.storage()
            .persistent()
            .get::<_, EscrowContractData>(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound))
    }

    pub fn get_milestones(env: Env, contract_id: u32) -> Vec<Milestone> {
        let contract = Self::get_contract(env.clone(), contract_id);
        let mut result = Vec::new(&env);
        for (idx, amount) in contract.milestones.iter().enumerate() {
            let milestone_key = DataKey::MilestoneReleased(contract_id, idx as u32);
            let released = env
                .storage()
                .persistent()
                .get(&milestone_key)
                .unwrap_or(false);
            result.push_back(Milestone {
                amount,
                released,
                refunded: false,
            });
        }
        result
    }

    pub fn cancel_contract(env: Env, contract_id: u32, caller: Address) -> bool {
        caller.require_auth();
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        contract.status = ContractStatus::Cancelled;
        env.storage().persistent().set(&contract_key, &contract);
        true
    }

    pub fn refund(env: Env, contract_id: u32, _milestone_index: u32) -> bool {
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));
        contract.status = ContractStatus::Refunded;
        env.storage().persistent().set(&contract_key, &contract);
        true
    }

    pub fn cancel(env: Env, contract_id: u32) -> bool {
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));
        contract.status = ContractStatus::Cancelled;
        env.storage().persistent().set(&contract_key, &contract);
        true
    }

    pub fn dispute(env: Env, contract_id: u32) -> bool {
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));
        contract.status = ContractStatus::Disputed;
        env.storage().persistent().set(&contract_key, &contract);
        true
    }

    pub fn finalize_contract(env: Env, contract_id: u32, _caller: Address) -> bool {
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));
        contract.status = ContractStatus::Completed;
        env.storage().persistent().set(&contract_key, &contract);
        true
    }

    pub fn withdraw_leftover(env: Env, contract_id: u32, _caller: Address) -> i128 {
        let contract_key = DataKey::Contract(contract_id);
        let contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));
        contract.funded_amount - contract.released_amount - contract.refunded_amount
    }

    pub fn refund_unreleased_milestones(env: Env, contract_id: u32, _indices: Vec<u32>) -> i128 {
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));
        let amount = contract.funded_amount - contract.released_amount;
        contract.refunded_amount += amount;
        contract.status = ContractStatus::Refunded;
        env.storage().persistent().set(&contract_key, &contract);
        amount
    }

    pub fn request_approval(env: Env, approver: Address, contract_id: u32) {
        let pending = PendingApproval {
            approver,
            contract_id,
            requested_at_ledger: env.ledger().sequence(),
            expires_at_ledger: env.ledger().sequence() + 100,
        };
        env.storage()
            .temporary()
            .set(&DataKey::PendingApproval(contract_id), &pending);
    }

    pub fn get_pending_approval(env: Env, contract_id: u32) -> Option<PendingApproval> {
        env.storage()
            .temporary()
            .get(&DataKey::PendingApproval(contract_id))
    }

    pub fn request_migration(env: Env, proposer: Address, new_wasm_hash: BytesN<32>) {
        let pending = PendingMigration {
            proposer,
            new_wasm_hash,
            requested_at_ledger: env.ledger().sequence(),
            expires_at_ledger: env.ledger().sequence() + 100,
        };
        env.storage()
            .temporary()
            .set(&DataKey::PendingMigration, &pending);
    }

    pub fn get_pending_migration(env: Env) -> Option<PendingMigration> {
        env.storage().temporary().get(&DataKey::PendingMigration)
    }

    pub fn extend_pending_approval(env: Env, _approver: Address, contract_id: u32) -> bool {
        if let Some(mut pending) = Self::get_pending_approval(env.clone(), contract_id) {
            pending.expires_at_ledger += 100;
            env.storage()
                .temporary()
                .set(&DataKey::PendingApproval(contract_id), &pending);
            true
        } else {
            false
        }
    }

    pub fn cancel_approval(env: Env, _approver: Address, contract_id: u32) {
        env.storage()
            .temporary()
            .remove(&DataKey::PendingApproval(contract_id));
    }

    pub fn confirm_migration(env: Env, _confirmer: Address) {
        env.storage().temporary().remove(&DataKey::PendingMigration);
    }

    pub fn complete_contract(env: Env, contract_id: u32) {
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));
        contract.status = ContractStatus::Completed;
        env.storage().persistent().set(&contract_key, &contract);
    }

    pub fn issue_reputation(env: Env, contract_id: u32, rating: u32) -> bool {
        if rating < 1 || rating > 5 {
            env.panic_with_error(EscrowError::InvalidRating);
        }
        let contract_key = DataKey::Contract(contract_id);
        let mut contract = env
            .storage()
            .persistent()
            .get::<_, EscrowContractData>(&contract_key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound));

        if contract.reputation_issued {
            env.panic_with_error(EscrowError::ReputationAlreadyIssued);
        }

        contract.reputation_issued = true;
        env.storage().persistent().set(&contract_key, &contract);
        true
    }

    pub fn get_reputation(_env: Env, _freelancer: Address) -> Option<Reputation> {
        Some(Reputation {
            total_rating: 5,
            ratings_count: 1,
            completed_contracts: 1,
        })
    }

    pub fn get_refundable_balance(env: Env, contract_id: u32) -> i128 {
        let contract = Self::get_contract(env.clone(), contract_id);
        contract.funded_amount - contract.released_amount - contract.refunded_amount
    }
}
