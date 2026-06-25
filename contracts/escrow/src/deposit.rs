use crate::{
    ttl, Contract, ContractStatus, DataKey, Error, Escrow, EscrowArgs, EscrowClient, Milestone,
};
use soroban_sdk::{contractimpl, symbol_short, Address, Env, Symbol, Vec};

#[contractimpl]
impl Escrow {
    /// Deposits funds into the contract. Transitions to Funded status when fully funded.
    ///
    /// # Arguments
    /// * `env` - The contract environment
    /// * `contract_id` - The contract ID
    /// * `amount` - The amount to deposit (in stroops)
    ///
    /// # Returns
    /// `true` if deposit was successful
    ///
    /// # Errors
    /// * `AmountMustBePositive` - If amount is <= 0
    /// * `ContractNotFound` - If contract doesn't exist
    /// * `InvalidState` - If contract is not in Created state
    /// * `UnauthorizedRole` - If caller is not the client
    pub fn deposit_funds(env: Env, contract_id: u32, caller: Address, amount: i128) -> bool {
        if amount <= 0 {
            env.panic_with_error(Error::AmountMustBePositive);
        }

        // Reject if paused or emergency is active (must run before loading
        // contract data so that unauthorised callers also get the same error).
        Self::require_not_paused(&env);

        Self::require_not_finalized(&env, contract_id);

        let mut contract: Contract = env
            .storage()
            .persistent()
            .get(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(Error::ContractNotFound));

        ttl::extend_contract_ttl(&env, contract_id);

        if caller != contract.client {
            env.panic_with_error(Error::UnauthorizedRole);
        }
        caller.require_auth();

        if contract.status != ContractStatus::Created {
            env.panic_with_error(Error::InvalidState);
        }

        contract.funded_amount += amount;

        let milestone_key = Symbol::new(&env, "milestones");
        let milestones: Vec<Milestone> = env
            .storage()
            .persistent()
            .get(&(DataKey::Contract(contract_id), milestone_key))
            .unwrap();

        ttl::extend_milestone_ttl(&env, contract_id);

        let total_amount: i128 = milestones.iter().map(|m| m.amount).sum();

        if contract.funded_amount >= total_amount && contract.status == ContractStatus::Created {
            contract.status = ContractStatus::Funded;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Contract(contract_id), &contract);

        ttl::extend_contract_ttl(&env, contract_id);

        env.events().publish(
            (symbol_short!("deposit"), contract_id),
            (
                caller,
                amount,
                contract.funded_amount,
                env.ledger().timestamp(),
            ),
        );

        true
    }
}
