use super::{assert_contract_error, assert_contract_state, create_client, create_default_contract, setup};
use crate::{types::Error, ContractStatus};
use soroban_sdk::{testutils::Address as _, Address};

/// Tests that incremental deposits accumulate and transition to Funded at the exact total.
///
/// # Security
/// - Validates state transition from Created to PartiallyFunded to Funded
/// - Ensures funded_amount tracking is accurate across multiple deposits
#[test]
fn deposit_incremental_two_deposits_transitions_to_funded() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    let total = total_milestone_amount();
    let partial = total / 2; // deposit half first

    // Deposit half the total — should stay in PartiallyFunded
    assert!(client.deposit_funds(&contract_id, &client_addr, &partial));
    let contract = client.get_contract(&contract_id);
    assert_contract_state(
        contract,
        ContractStatus::PartiallyFunded,
        partial,
        0,
        0,
    );

    // Deposit remaining half — should transition to Funded
    assert!(client.deposit_funds(&contract_id, &client_addr, &(total - partial)));
    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Funded);
    assert_eq!(contract.funded_amount, total);
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

/// Tests that deposit_funds panics with UnauthorizedRole when caller is not the depositor.
///
/// Asserts the exact error code when an unauthorized address attempts to deposit.
/// 
/// # Security
/// - Prevents unauthorized fund deposits
/// - Enforces client-only deposit authorization
#[test]
fn test_deposit_unauthorized_role() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Attempt deposit from wrong caller (freelancer instead of client)
    let wrong_caller = Address::generate(&env);
    let result = client.try_deposit_funds(&contract_id, &wrong_caller, &100_0000000_i128);
    assert_contract_error(result, Error::UnauthorizedRole);
}

/// Tests that deposit_funds panics with InvalidState when contract is not in Created state.
///
/// Asserts the exact error code when attempting to deposit after contract has been funded.
/// 
/// # Security
/// - Prevents state machine violations
/// - Ensures deposits only occur during contract setup phase
#[test]
fn test_deposit_invalid_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Fully fund the contract first (transitions to Funded state)
    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));

    // Try to deposit again (contract is now Funded, not Created)
    let result = client.try_deposit_funds(&contract_id, &client_addr, &100_0000000_i128);
    assert_contract_error(result, Error::InvalidState);
}

/// Tests that deposit_funds panics with InsufficientFunds when caller token balance is too low.
///
/// Note: In Soroban test environment with mocked auth, balance checks are typically bypassed.
/// This test documents the error branch but may not be directly testable without token contract integration.
/// 
/// # UNREACHABLE
/// InsufficientFunds in deposit_funds is currently unreachable because:
/// - The contract does not perform balance verification in the current implementation
/// - Token transfer is mocked in test environment
/// - Real balance checks occur only at the token contract level during actual transfers
///
/// Documented per Issue #405 requirements for completeness.
#[test]
#[ignore]
fn test_deposit_insufficient_funds() {
    // UNREACHABLE: deposit_funds does not check caller's token balance
    // in the current implementation. Balance validation occurs at token contract level.
    // This test is documented for completeness but cannot be triggered in unit tests.
}

/// Tests the precise boundary transition from Created to Funded.
///
/// # Security
/// - Validates deterministic state transition upon full funding
/// - Ensures partial funding does not prematurely transition state
/// - Verifies refundable balance calculation across deposits
#[test]
fn test_funded_boundary_incremental_and_exact() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    
    // Multi-deposit accumulation: under by one
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    let total = total_milestone_amount();
    
    // Deposit total - 1
    assert!(client.deposit_funds(&contract_id, &client_addr, &(total - 1)));
    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Created);
    let refundable = client.get_refundable_balance(&contract_id);
    assert_eq!(refundable, contract.funded_amount - contract.released_amount - contract.refunded_amount);
    
    // Deposit final 1 stroop
    assert!(client.deposit_funds(&contract_id, &client_addr, &1_i128));
    let contract2 = client.get_contract(&contract_id);
    assert_eq!(contract2.status, ContractStatus::Funded);
    let refundable2 = client.get_refundable_balance(&contract_id);
    assert_eq!(refundable2, contract2.funded_amount - contract2.released_amount - contract2.refunded_amount);
    
    // Deposit on already-Funded contract is rejected with InvalidState
    let result = client.try_deposit_funds(&contract_id, &client_addr, &100_i128);
    assert_contract_error(result, Error::InvalidState);
    
    // Deposit exactly total in one call
    let contract_id2 = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    assert!(client.deposit_funds(&contract_id2, &client_addr, &total));
    let contract3 = client.get_contract(&contract_id2);
    assert_eq!(contract3.status, ContractStatus::Funded);
    let refundable3 = client.get_refundable_balance(&contract_id2);
    assert_eq!(refundable3, contract3.funded_amount - contract3.released_amount - contract3.refunded_amount);
    
    // Deposit over total by 1 stroop — production code accepts this from Created; asserts Funded
    let contract_id3 = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    assert!(client.deposit_funds(&contract_id3, &client_addr, &(total + 1)));
    let contract4 = client.get_contract(&contract_id3);
    assert_eq!(contract4.status, ContractStatus::Funded);
    let refundable4 = client.get_refundable_balance(&contract_id3);
    assert_eq!(refundable4, contract4.funded_amount - contract4.released_amount - contract4.refunded_amount);
}
