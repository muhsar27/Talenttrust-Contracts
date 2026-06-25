use crate::{ttl, Contract, ContractStatus, DataKey, Error, Escrow, Milestone};
use soroban_sdk::{Address, Env, Symbol, Vec};

impl Escrow {
    /// Core logic for depositing funds into a contract.
    ///
    /// Called from the single `#[contractimpl]` block in lib.rs after the
    /// initialization and pause guards have been checked.
    pub(crate) fn deposit_funds_impl(
        env: &Env,
        contract_id: u32,
        caller: Address,
        amount: i128,
    ) -> bool {
        if amount <= 0 {
            env.panic_with_error(Error::AmountMustBePositive);
        }

        let mut contract: Contract = env
            .storage()
            .persistent()
            .get(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(Error::ContractNotFound));

        ttl::extend_contract_ttl(env, contract_id);

        if caller != contract.client {
            env.panic_with_error(Error::UnauthorizedRole);
        }
        caller.require_auth();

        if contract.status != ContractStatus::Created {
            env.panic_with_error(Error::InvalidState);
        }

        contract.funded_amount += amount;

        let milestone_key = Symbol::new(env, "milestones");
        let milestones: Vec<Milestone> = env
            .storage()
            .persistent()
            .get(&(DataKey::Contract(contract_id), milestone_key))
            .unwrap();

        ttl::extend_milestone_ttl(env, contract_id);

        let total_amount: i128 = milestones.iter().map(|m| m.amount).sum();

        if contract.funded_amount >= total_amount && contract.status == ContractStatus::Created {
            contract.status = ContractStatus::Funded;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Contract(contract_id), &contract);

        ttl::extend_contract_ttl(env, contract_id);

        true
    }
}
