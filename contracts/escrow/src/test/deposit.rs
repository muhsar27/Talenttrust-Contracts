use super::{
    assert_contract_error, assert_contract_state, create_client, create_default_contract, setup,
};
use crate::{types::Error, ContractStatus};
use soroban_sdk::{testutils::Address as _, Address};

/// Tests that deposits accumulate correctly and transition to Funded status when exactly fully funded.
///
/// # Security
/// - Validates state transition from Created to Funded
/// - Ensures funded_amount tracking is accurate
#[test]
fn accumulates_deposits_without_exceeding_total() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    // Deposit 600 (half of the 1,200 required milestone total)
    assert!(client.deposit_funds(&contract_id, &client_addr, &600_0000000_i128));
    let contract = client.get_contract(&contract_id);
    assert_contract_state(
        contract,
        ContractStatus::PartiallyFunded,
        600_0000000_i128,
        0,
        0,
    );

    // Deposit remaining 600, transitions status to Funded
    assert!(client.deposit_funds(&contract_id, &client_addr, &600_0000000_i128));
    let contract = client.get_contract(&contract_id);
    assert_contract_state(contract, ContractStatus::Funded, 1_200_0000000_i128, 0, 0);
}

/// Tests that non-client callers are rejected with UnauthorizedRole.
///
/// # Security
/// - Prevents unauthorized parties (freelancer, arbiter, or attacker) from depositing funds.
#[test]
fn rejects_non_client_caller() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    // Freelancer tries to deposit
    let result = client.try_deposit_funds(&contract_id, &freelancer_addr, &600_0000000_i128);
    assert_contract_error(result, Error::UnauthorizedRole);

    // Random attacker tries to deposit
    let attacker = Address::generate(&env);
    let result = client.try_deposit_funds(&contract_id, &attacker, &600_0000000_i128);
    assert_contract_error(result, Error::UnauthorizedRole);
}

/// Tests that zero-amount deposits are rejected.
///
/// # Security
/// - Prevents dust attacks and invalid state transitions
#[test]
fn rejects_zero_deposit() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    let result = client.try_deposit_funds(&contract_id, &client_addr, &0_i128);
    assert_contract_error(result, Error::AmountMustBePositive);
}

/// Tests that negative-amount deposits are rejected.
///
/// # Security
/// - Prevents balance draining via negative amounts
#[test]
fn rejects_negative_deposit() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    let result = client.try_deposit_funds(&contract_id, &client_addr, &-100_i128);
    assert_contract_error(result, Error::AmountMustBePositive);
}

/// Tests that deposits exceeding the total milestone amount (overfunding) are rejected.
///
/// # Security
/// - Prevents overfunding attacks
/// - Ensures contract accounting integrity
#[test]
fn rejects_overfunding() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    // Try to deposit 1,300 (total is 1,200) in a single deposit
    let result = client.try_deposit_funds(&contract_id, &client_addr, &1_300_0000000_i128);
    assert_contract_error(result, Error::InvalidState);

    // Deposit 600 first (valid)
    assert!(client.deposit_funds(&contract_id, &client_addr, &600_0000000_i128));

    // Try to deposit 700 next (exceeds remaining 600 limit)
    let result = client.try_deposit_funds(&contract_id, &client_addr, &700_0000000_i128);
    assert_contract_error(result, Error::InvalidState);
}

/// Tests that deposits are rejected after contract is fully refunded.
///
/// # Security
/// - Validates fail-closed state machine
/// - Prevents re-funding of resolved contracts
#[test]
fn rejects_deposit_after_full_refund_resolution() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    let refund_ids = soroban_sdk::vec![&env, 0_u32, 1_u32, 2_u32];
    let refunded = client.refund_unreleased_milestones(&contract_id, &refund_ids);
    assert_eq!(refunded, 1_200_0000000_i128);

    // Attempting deposit after refund should fail because contract status is Refunded
    let result = client.try_deposit_funds(&contract_id, &client_addr, &1_i128);
    assert_contract_error(result, Error::InvalidState);
}
