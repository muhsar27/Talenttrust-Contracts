use crate::{
    approvals, ttl, Contract, ContractStatus, DataKey, Error, Escrow, ReleaseAuthorization,
};
use soroban_sdk::{contractimpl, Address, Env};

#[contractimpl]
impl Escrow {
    /// Releases a specific milestone, transferring funds to the freelancer.
    ///
    /// Requires valid, non-expired approvals based on the contract's ReleaseAuthorization mode.
    ///
    /// MultiSig semantics are client-and-freelancer approval. A MultiSig
    /// milestone can be released only by the stored client or freelancer after
    /// both of those addresses have approved the same milestone.
    ///
    /// # Arguments
    /// * `env` - The contract environment
    /// * `contract_id` - The contract ID
    /// * `caller` - The address of the caller (must be authorized)
    /// * `milestone_index` - The index of the milestone to release
    ///
    /// # Returns
    /// `true` if release was successful
    ///
    /// # Errors
    /// * `ContractNotFound` - If contract doesn't exist
    /// * `InvalidState` - If contract is not in Funded state
    /// * `IndexOutOfBounds` - If milestone index is invalid
    /// * `MilestoneAlreadyReleased` - If milestone was already released
    /// * `AlreadyRefunded` - If milestone was already refunded
    /// * `InsufficientFunds` - If contract doesn't have enough funded balance
    /// * `InsufficientApprovals` - If required approvals are missing
    /// * `ApprovalExpired` - If approvals have expired
    /// * `UnauthorizedRole` - If caller is not authorized to release
    ///
    /// # Security
    /// - **Caller Authentication**: Enforced right at entry layer via `caller.require_auth()`.
    /// - Requires valid approvals that haven't expired under temporary tracking storage keys.
    /// - Approvals are cleared instantly after a successful milestone release execution loop.
    /// - Fail-closed: missing or expired approvals prevent downstream release logic.
    pub fn release_milestone(
        env: Env,
        contract_id: u32,
        caller: Address,
        milestone_index: u32,
    ) -> bool {
        Self::require_not_paused(&env);
        caller.require_auth();

        Self::require_not_paused(&env);

        Self::require_not_finalized(&env, contract_id);

        let mut contract: Contract = env
            .storage()
            .persistent()
            .get(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(Error::ContractNotFound));

        ttl::extend_contract_ttl(&env, contract_id);

        Self::require_not_paused(&env);
        Self::require_not_finalized(&env, contract_id);

        if contract.status != ContractStatus::Funded {
            env.panic_with_error(Error::InvalidState);
        }

        let is_client = caller == contract.client;
        let is_freelancer = caller == contract.freelancer;
        let is_arbiter = contract.arbiter.as_ref() == Some(&caller);

        match contract.release_authorization {
            ReleaseAuthorization::ClientOnly => {
                if !is_client {
                    env.panic_with_error(Error::UnauthorizedRole);
                }
            }
            ReleaseAuthorization::ArbiterOnly => {
                if !is_arbiter {
                    env.panic_with_error(Error::UnauthorizedRole);
                }
            }
            ReleaseAuthorization::ClientAndArbiter => {
                if !is_client && !is_arbiter {
                    env.panic_with_error(Error::UnauthorizedRole);
                }
            }
            ReleaseAuthorization::MultiSig => {
                if !is_client && !is_freelancer {
                    env.panic_with_error(Error::UnauthorizedRole);
                }
            }
        }

        let milestone_key = Symbol::new(&env, "milestones");
        let mut milestones: Vec<Milestone> = env
            .storage()
            .persistent()
            .get(&(DataKey::Contract(contract_id), milestone_key.clone()))
            .unwrap();

        ttl::extend_milestone_ttl(&env, contract_id);

        if milestone_index >= milestones.len() {
            env.panic_with_error(Error::IndexOutOfBounds);
        }

        let mut milestone = milestones.get(milestone_index).unwrap().clone();

        if milestone.released {
            env.panic_with_error(Error::MilestoneAlreadyReleased);
        }

        if milestone.refunded {
            env.panic_with_error(Error::AlreadyRefunded);
        }

        approvals::check_approvals(&env, &contract, contract_id, milestone_index)
            .unwrap_or_else(|e| env.panic_with_error(e));

        let available_balance =
            contract.funded_amount - contract.released_amount - contract.refunded_amount;
        if available_balance < milestone.amount {
            env.panic_with_error(Error::InsufficientFunds);
        }

        let _release_amount = milestone.amount;
        milestone.released = true;
        milestones.set(milestone_index, milestone.clone());
        contract.released_amount += milestone.amount;

        if is_initialized(&env) {
            let fee_bps = get_protocol_fee_bps(&env);
            if fee_bps > 0 {
                let fee = calculate_protocol_fee(milestone.amount, fee_bps);
                let current_accumulated: i128 = env
                    .storage()
                    .persistent()
                    .get(&DataKey::AccumulatedProtocolFees)
                    .unwrap_or(0);
                env.storage().persistent().set(
                    &DataKey::AccumulatedProtocolFees,
                    &(current_accumulated + fee),
                );
            }
        }

        approvals::clear_approvals(&env, contract_id, milestone_index);

        let all_released = milestones.iter().all(|m| m.released || m.refunded);
        if all_released {
            contract.status = ContractStatus::Completed;
            let pending_key = DataKey::PendingReputationCredits(contract.freelancer.clone());
            let pending: i128 = env.storage().persistent().get(&pending_key).unwrap_or(0);
            env.storage().persistent().set(&pending_key, &(pending + 1));
        }

        ttl::store_milestones(&env, contract_id, &milestones);
        env.storage()
            .persistent()
            .set(&DataKey::Contract(contract_id), &contract);

        ttl::extend_contract_ttl(&env, contract_id);

        env.events().publish(
            (
                Symbol::new(&env, "milestone_released"),
                contract_id,
                milestone_index,
            ),
            (
                caller,
                milestone.amount,
                contract.released_amount,
                env.ledger().timestamp(),
            ),
        );

        true
    }

    /// Releases multiple milestones atomically, transferring funds to the freelancer for each.
    ///
    /// Requires valid, non-expired approvals for each milestone based on the contract's ReleaseAuthorization mode.
    /// Validates all milestones and approvals first before any state mutation.
    ///
    /// # Arguments
    /// * `env` - The contract environment
    /// * `contract_id` - The contract ID
    /// * `caller` - The address of the caller (must be authorized)
    /// * `milestone_indices` - Vector of milestone indices to release
    ///
    /// # Returns
    /// The total amount released
    ///
    /// # Errors
    /// * `ContractNotFound` - If contract doesn't exist
    /// * `InvalidState` - If contract is not in Funded state
    /// * `EmptyRefundRequest` (reused) - If milestone_indices is empty
    /// * `DuplicateMilestoneInRefund` (reused) - If the same milestone appears multiple times
    /// * `IndexOutOfBounds` - If any milestone index is out of bounds
    /// * `MilestoneAlreadyReleased` - If any milestone was already released
    /// * `AlreadyRefunded` - If any milestone was already refunded
    /// * `InsufficientFunds` - If contract doesn't have enough funded balance for all milestones
    /// * `InsufficientApprovals` - If required approvals are missing for any milestone
    /// * `ApprovalExpired` - If approvals have expired for any milestone
    /// * `UnauthorizedRole` - If caller is not authorized to release
    ///
    /// # Security
    /// - Validates all milestones and approvals first to ensure atomicity (all-or-nothing)
    /// - Requires valid approvals that haven't expired
    /// - Approvals are cleared after successful release for each milestone
    /// - Fail-closed: missing or expired approvals prevent release
    pub fn release_milestones(
        env: Env,
        contract_id: u32,
        caller: Address,
        milestone_indices: Vec<u32>,
    ) -> i128 {
        // Validate non-empty request
        if milestone_indices.is_empty() {
            env.panic_with_error(Error::EmptyRefundRequest);
        }

        // Check for duplicates
        for i in 0..milestone_indices.len() {
            for j in (i + 1)..milestone_indices.len() {
                if milestone_indices.get(i).unwrap() == milestone_indices.get(j).unwrap() {
                    env.panic_with_error(Error::DuplicateMilestoneInRefund);
                }
            }
        }

        // Authenticate caller before any state-dependent logic
        caller.require_auth();

        let mut contract: Contract = env
            .storage()
            .persistent()
            .get(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(Error::ContractNotFound));

        ttl::extend_contract_ttl(&env, contract_id);

        Self::require_not_finalized(&env, contract_id);

        if contract.status != ContractStatus::Funded {
            env.panic_with_error(Error::InvalidState);
        }

        let is_client = caller == contract.client;
        let is_freelancer = caller == contract.freelancer;
        let is_arbiter = contract.arbiter.as_ref() == Some(&caller);

        match contract.release_authorization {
            ReleaseAuthorization::ClientOnly => {
                if !is_client {
                    env.panic_with_error(Error::UnauthorizedRole);
                }
            }
            ReleaseAuthorization::ArbiterOnly => {
                if !is_arbiter {
                    env.panic_with_error(Error::UnauthorizedRole);
                }
            }
            ReleaseAuthorization::ClientAndArbiter => {
                if !is_client && !is_arbiter {
                    env.panic_with_error(Error::UnauthorizedRole);
                }
            }
            ReleaseAuthorization::MultiSig => {
                if !is_client && !is_freelancer {
                    env.panic_with_error(Error::UnauthorizedRole);
                }
            }
        }

        let milestone_key = Symbol::new(&env, "milestones");
        let mut milestones: Vec<Milestone> = env
            .storage()
            .persistent()
            .get(&(DataKey::Contract(contract_id), milestone_key.clone()))
            .unwrap();

        ttl::extend_milestone_ttl(&env, contract_id);

        let mut total_release_amount: i128 = 0;

        // Validate all milestones and approvals first (atomic validation)
        for idx in milestone_indices.iter() {
            if idx >= milestones.len() {
                env.panic_with_error(Error::IndexOutOfBounds);
            }

            let milestone = milestones.get(idx).unwrap();

            if milestone.released {
                env.panic_with_error(Error::MilestoneAlreadyReleased);
            }

            if milestone.refunded {
                env.panic_with_error(Error::AlreadyRefunded);
            }

            // Check for valid approvals for this milestone
            approvals::check_approvals(&env, &contract, contract_id, idx)
                .unwrap_or_else(|e| env.panic_with_error(e));

            total_release_amount += milestone.amount;
        }

        // Check if there's enough balance for all milestones
        let available_balance =
            contract.funded_amount - contract.released_amount - contract.refunded_amount;
        if available_balance < total_release_amount {
            env.panic_with_error(Error::InsufficientFunds);
        }

        // Now perform the actual releases (state mutations)
        for idx in milestone_indices.iter() {
            let mut milestone = milestones.get(idx).unwrap().clone();
            milestone.released = true;
            milestones.set(idx, milestone.clone());
            contract.released_amount += milestone.amount;

            // Accumulate protocol fees if initialized with a fee rate
            if is_initialized(&env) {
                let fee_bps = get_protocol_fee_bps(&env);
                if fee_bps > 0 {
                    let fee = calculate_protocol_fee(milestone.amount, fee_bps);
                    let current_accumulated: i128 = env
                        .storage()
                        .persistent()
                        .get(&DataKey::AccumulatedProtocolFees)
                        .unwrap_or(0);
                    env.storage().persistent().set(
                        &DataKey::AccumulatedProtocolFees,
                        &(current_accumulated + fee),
                    );
                }
            }

            // Clear approvals for this milestone
            approvals::clear_approvals(&env, contract_id, idx);
        }

        // Check if all milestones are released or refunded
        let all_released_or_refunded = milestones.iter().all(|m| m.released || m.refunded);
        if all_released_or_refunded {
            contract.status = ContractStatus::Completed;
        }

        env.storage().persistent().set(
            &(DataKey::Contract(contract_id), milestone_key),
            &milestones,
        );
        env.storage()
            .persistent()
            .set(&DataKey::Contract(contract_id), &contract);

        ttl::extend_contract_and_milestones_ttl(&env, contract_id);

        total_release_amount
    }
}

/// Returns true if the contract has been initialized.
fn is_initialized(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get::<_, bool>(&DataKey::Initialized)
        .unwrap_or(false)
}

/// Returns the protocol fee in basis points.
pub(crate) fn get_protocol_fee_bps(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get::<_, u32>(&DataKey::ProtocolFeeBps)
        .unwrap_or(0)
}

/// Calculates the protocol fee for a given amount.
fn calculate_protocol_fee(amount: i128, fee_bps: u32) -> i128 {
    if fee_bps == 0 {
        return 0;
    }
    amount * fee_bps as i128 / 10_000
}
