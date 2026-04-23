#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, vec, Address, Env};

use crate::{ContractStatus, Escrow, EscrowClient};

fn new_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

#[test]
fn test_hello() {
    let env = new_env();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let result = client.hello(&symbol_short!("World"));
    assert_eq!(result, symbol_short!("World"));
}

#[test]
fn test_create_contract() {
    let env = new_env();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let milestones = vec![&env, 200_0000000_i128, 400_0000000_i128, 600_0000000_i128];

    let id = client.create_contract(&client_addr, &freelancer_addr, &milestones);
    assert_eq!(id, 0);

    let data = client.get_contract(&id);
    assert_eq!(data.total_amount, 1_200_0000000_i128);
    assert_eq!(data.funded_amount, 0);
    assert_eq!(data.status, ContractStatus::Created);
    assert_eq!(client.get_milestones(&id).len(), 3);
}

#[test]
fn test_deposit_funds() {
    let env = new_env();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let milestones = vec![&env, 500_0000000_i128, 500_0000000_i128];
    let id = client.create_contract(&client_addr, &freelancer_addr, &milestones);

    assert!(client.deposit_funds(&id, &400_0000000_i128));
    let data = client.get_contract(&id);
    assert_eq!(data.funded_amount, 400_0000000_i128);
    assert_eq!(data.status, ContractStatus::Funded);
}

#[test]
fn test_release_milestone() {
    let env = new_env();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let milestones = vec![&env, 300_0000000_i128, 700_0000000_i128];
    let id = client.create_contract(&client_addr, &freelancer_addr, &milestones);

    client.deposit_funds(&id, &1_000_0000000_i128);
    assert!(client.release_milestone(&id, &0));
    let data = client.get_contract(&id);
    assert_eq!(data.released_amount, 300_0000000_i128);
    assert_eq!(data.status, ContractStatus::Funded);

    client.release_milestone(&id, &1);
    assert_eq!(client.get_contract(&id).status, ContractStatus::Completed);
}

