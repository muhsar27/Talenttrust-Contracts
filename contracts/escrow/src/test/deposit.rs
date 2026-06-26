use super::{assert_contract_state, assert_contract_error, register_client, create_contract, total_milestone_amount, default_milestones};
use crate::{ContractStatus, Error as EscrowError, ReleaseAuthorization};
use soroban_sdk::{testutils::Address as _, Address, Env, vec};

/// Tests that deposits accumulate correctly and transition to Funded status when fully funded.
/// 
/// # Security
/// - Validates state transition from Created to Funded
/// - Ensures funded_amount tracking is accurate
#[test]
fn accumulates_deposits_without_exceeding_total() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &600_0000000_i128));
    let contract = client.get_contract(&contract_id);
    assert_contract_state(contract, ContractStatus::Created, 600_0000000_i128, 0, 0);

    assert!(client.deposit_funds(&contract_id, &client_addr, &600_0000000_i128));
    let contract = client.get_contract(&contract_id);
    assert_contract_state(contract, ContractStatus::Funded, 1_200_0000000_i128, 0, 0);
}

/// Tests that zero-amount deposits are rejected.
/// 
/// # Security
/// - Prevents dust attacks and invalid state transitions
#[test]
#[should_panic]
fn rejects_zero_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    client.deposit_funds(&contract_id, &client_addr, &0_i128);
}

/// Tests that deposits exceeding the total milestone amount are rejected.
/// 
/// # Security
/// - Prevents overfunding attacks
/// - Ensures contract accounting integrity
#[test]
#[should_panic]
fn rejects_overfunding() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    client.deposit_funds(&contract_id, &client_addr, &1_300_0000000_i128);
}

/// Tests that deposits are rejected after contract is fully refunded.
/// 
/// # Security
/// - Validates fail-closed state machine
/// - Prevents re-funding of resolved contracts
#[test]
#[should_panic]
fn rejects_deposit_after_full_refund_resolution() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    let refund_ids = vec![&env, 0_u32, 1_u32, 2_u32];
    let refunded = client.refund_unreleased_milestones(&contract_id, &refund_ids);
    assert_eq!(refunded, total_milestone_amount());

    client.deposit_funds(&contract_id, &client_addr, &1_i128);
}

// =========================================================================
// NEGATIVE-PATH TESTS FOR Issue #405
// =========================================================================

/// Tests that deposit_funds panics with AmountMustBePositive when amount == 0.
///
/// Asserts the exact error code for zero-amount deposits.
/// 
/// # Security
/// - Prevents accounting anomalies from zero deposits
/// - Validates amount validation at entry point
#[test]
fn test_deposit_amount_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    let result = client.try_deposit_funds(&contract_id, &client_addr, &0_i128);
    assert_contract_error(result, EscrowError::AmountMustBePositive);
}

/// Tests that deposit_funds panics with AmountMustBePositive when amount < 0.
///
/// Asserts the exact error code for negative amounts.
/// 
/// # Security
/// - Prevents accounting anomalies from negative deposits
/// - Validates amount validation rejects all non-positive values
#[test]
fn test_deposit_amount_negative() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    let result = client.try_deposit_funds(&contract_id, &client_addr, &-1_i128);
    assert_contract_error(result, EscrowError::AmountMustBePositive);
}

/// Tests that deposit_funds panics with ContractNotFound for unknown contract id.
///
/// Asserts the exact error code when contract does not exist in storage.
/// 
/// # Security
/// - Prevents operations on non-existent contracts
/// - Ensures fail-closed behavior for invalid contract IDs
#[test]
fn test_deposit_contract_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let client_addr = Address::generate(&env);

    // Use a contract_id that was never created
    let invalid_contract_id = 9999_u32;
    let result = client.try_deposit_funds(&invalid_contract_id, &client_addr, &100_0000000_i128);
    assert_contract_error(result, EscrowError::ContractNotFound);
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
    assert_contract_error(result, EscrowError::UnauthorizedRole);
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
    assert_contract_error(result, EscrowError::InvalidState);
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

