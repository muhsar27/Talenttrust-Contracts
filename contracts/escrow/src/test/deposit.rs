use super::{
    assert_contract_error, create_contract, register_client, total_milestone_amount, MILESTONE_ONE,
    MILESTONE_THREE, MILESTONE_TWO,
};
use crate::{Contract, ContractStatus, DataKey, Error, Milestone, ReleaseAuthorization};
use soroban_sdk::{testutils::Address as _, vec, Address, Env, Symbol, Vec};

fn milestone_funding(env: &Env, client: &crate::EscrowClient<'_>, contract_id: u32) -> Vec<i128> {
    let milestones = client.get_milestones(&contract_id);
    let mut funding = Vec::new(env);
    for milestone in milestones.iter() {
        funding.push_back(milestone.funded_amount);
    }
    funding
}

#[test]
fn partial_deposit_allocates_only_to_first_milestone() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _, contract_id) = create_contract(&env, &client);
    let deposit = MILESTONE_ONE / 2;

    assert!(client.deposit_funds(&contract_id, &client_addr, &deposit));

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Created);
    assert_eq!(contract.funded_amount, deposit);
    assert_eq!(
        milestone_funding(&env, &client, contract_id),
        vec![&env, deposit, 0, 0]
    );
}

#[test]
fn spanning_deposit_fills_milestones_in_order() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _, contract_id) = create_contract(&env, &client);
    let deposit = MILESTONE_ONE + (MILESTONE_TWO / 2);

    assert!(client.deposit_funds(&contract_id, &client_addr, &deposit));

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Created);
    assert_eq!(contract.funded_amount, deposit);
    assert_eq!(
        milestone_funding(&env, &client, contract_id),
        vec![&env, MILESTONE_ONE, MILESTONE_TWO / 2, 0]
    );
}

#[test]
fn incremental_deposits_resume_at_next_unfunded_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _, contract_id) = create_contract(&env, &client);
    let first_deposit = MILESTONE_ONE + (MILESTONE_TWO / 2);
    let second_deposit = (MILESTONE_TWO / 2) + (MILESTONE_THREE / 3);

    assert!(client.deposit_funds(&contract_id, &client_addr, &first_deposit));
    assert!(client.deposit_funds(&contract_id, &client_addr, &second_deposit));

    assert_eq!(
        milestone_funding(&env, &client, contract_id),
        vec![&env, MILESTONE_ONE, MILESTONE_TWO, MILESTONE_THREE / 3]
    );
    assert_eq!(
        client.get_contract(&contract_id).funded_amount,
        first_deposit + second_deposit
    );
}

#[test]
fn exact_total_deposit_funds_all_milestones_and_preserves_aggregate() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Funded);
    assert_eq!(contract.funded_amount, total_milestone_amount());
    assert_eq!(
        milestone_funding(&env, &client, contract_id),
        vec![&env, MILESTONE_ONE, MILESTONE_TWO, MILESTONE_THREE]
    );
}

#[test]
fn overfunding_is_rejected_without_allocating_to_milestones() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _, contract_id) = create_contract(&env, &client);

    assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &(total_milestone_amount() + 1)),
        Error::FundingExceedsRequired,
    );

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Created);
    assert_eq!(contract.funded_amount, 0);
    assert_eq!(
        milestone_funding(&env, &client, contract_id),
        vec![&env, 0, 0, 0]
    );
}

#[test]
fn release_rejects_legacy_aggregate_funding_without_milestone_allocation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let milestones = vec![&env, MILESTONE_ONE, MILESTONE_TWO, MILESTONE_THREE];
    let contract_id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
    );

    env.as_contract(&client.address, || {
        let contract_key = DataKey::Contract(contract_id);
        let mut contract: Contract = env.storage().persistent().get(&contract_key).unwrap();
        contract.status = ContractStatus::Funded;
        contract.funded_amount = total_milestone_amount();
        env.storage().persistent().set(&contract_key, &contract);

        let milestone_key = Symbol::new(&env, "milestones");
        let mut stored: Vec<Milestone> = env
            .storage()
            .persistent()
            .get(&(DataKey::Contract(contract_id), milestone_key.clone()))
            .unwrap();
        let mut first = stored.get(0).unwrap();
        first.funded_amount = MILESTONE_ONE - 1;
        stored.set(0, first);
        env.storage()
            .persistent()
            .set(&(DataKey::Contract(contract_id), milestone_key), &stored);
    });

    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert_contract_error(
        client.try_release_milestone(&contract_id, &client_addr, &0),
        Error::InsufficientFunds,
    );
}
