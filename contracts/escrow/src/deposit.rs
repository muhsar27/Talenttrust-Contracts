use crate::{
    ttl, Contract, ContractStatus, DataKey, Error, GovernedParameters, Milestone,
};
use soroban_sdk::{Address, Env, Symbol, Vec};

/// Deposits funds into the contract and allocates them across milestones in order.
///
/// Each accepted deposit fills the first underfunded milestone before moving to
/// the next one. The aggregate `funded_amount` is still maintained for
/// backward-compatible reads.
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
/// * `FundingExceedsRequired` - If the deposit would exceed the milestone total
pub fn deposit_funds_impl(env: &Env, contract_id: u32, caller: Address, amount: i128) -> bool {
    if amount <= 0 {
        env.panic_with_error(Error::AmountMustBePositive);
    }

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

    if contract.status != ContractStatus::Created
        && contract.status != ContractStatus::PartiallyFunded
    {
        env.panic_with_error(Error::InvalidState);
    }

    // Check governed max_escrow_total_stroops cap if set
    if let Some(params) = env
        .storage()
        .persistent()
        .get::<_, GovernedParameters>(&DataKey::GovernedParameters)
    {
        if params.max_escrow_total_stroops > 0 {
            let new_total = contract
                .funded_amount
                .checked_add(amount)
                .unwrap_or_else(|| {
                    env.panic_with_error(Error::PotentialOverflow);
                });
            if new_total > params.max_escrow_total_stroops {
                env.panic_with_error(Error::InvalidProtocolParameters);
            }
        }
    }

    contract.funded_amount += amount;

    let milestone_key = Symbol::new(&env, "milestones");
    let mut milestones: Vec<Milestone> = env
        .storage()
        .persistent()
        .get(&(DataKey::Contract(contract_id), milestone_key.clone()))
        .unwrap();

    ttl::extend_milestone_ttl(&env, contract_id);

    // Distribute the deposited amount across milestones in order,
    // filling each milestone's funded_amount up to its amount.
    let mut remaining = amount;
    for i in 0..milestones.len() {
        if remaining <= 0 {
            break;
        }
        let mut milestone = milestones.get(i).unwrap();
        if milestone.funded_amount < milestone.amount {
            let needed = milestone
                .amount
                .checked_sub(milestone.funded_amount)
                .unwrap_or_else(|| env.panic_with_error(Error::AmountMustBePositive));
            let to_add = if remaining >= needed {
                needed
            } else {
                remaining
            };
            milestone.funded_amount = milestone
                .funded_amount
                .checked_add(to_add)
                .unwrap_or_else(|| env.panic_with_error(Error::AmountMustBePositive));
            milestones.set(i, milestone);
            remaining = remaining
                .checked_sub(to_add)
                .unwrap_or_else(|| env.panic_with_error(Error::AmountMustBePositive));
        }
    }

    let total_amount: i128 = milestones.iter().map(|m| m.amount).sum();

    if contract.funded_amount >= total_amount && contract.status == ContractStatus::Created {
        contract.status = ContractStatus::Funded;
        env.events().publish(
            (symbol_short!("status"), contract_id),
            (ContractStatus::Funded, env.ledger().timestamp()),
        );
    }

    env.storage().persistent().set(
        &(DataKey::Contract(contract_id), milestone_key),
        &milestones,
    );
    env.storage()
        .persistent()
        .set(&DataKey::Contract(contract_id), &contract);

    ttl::extend_contract_and_milestones_ttl(&env, contract_id);

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{create_contract, register_client, total_milestone_amount};
    use soroban_sdk::testutils::Events as _;
    extern crate std;
    use std::format;

    #[test]
    fn deposit_emits_status_changed_event() {
        let env = Env::default();
        env.mock_all_auths();

        let client = register_client(&env);
        let (client_addr, _, contract_id) = create_contract(&env, &client);

        assert!(client.deposit_funds(
            &contract_id,
            &client_addr,
            &total_milestone_amount(),
        ));

        let events = env.events().all();

        assert!(events.iter().any(|e| {
            format!("{:?}", e).contains("status_changed")
        }));
    }
}
