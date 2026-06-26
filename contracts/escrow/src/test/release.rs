use soroban_sdk::vec;

use super::{
    assert_contract_state, assert_contract_error, assert_milestone_flags, register_client, 
    create_contract, create_contract_with_arbiter, total_milestone_amount,
};
use crate::{ContractStatus, Error as EscrowError, ReleaseAuthorization};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Tests that milestones can be released sequentially and contract completes when all are released.
/// 
/// # Security
/// - Validates authorization checks for release
/// - Ensures released_amount tracking is accurate
/// - Verifies state transition to Completed
/// - Confirms refundable balance calculation
#[test]
fn releases_funded_milestones_and_completes_when_all_are_released() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));

    // Approve and release first milestone
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));
    let contract = client.get_contract(&contract_id);
    assert_contract_state(
        contract,
        ContractStatus::Funded,
        total_milestone_amount(),
        200_0000000_i128,
        0,
    );
    assert_milestone_flags(client.get_milestones(&contract_id), 0, true, false);
    assert_eq!(
        client.get_refundable_balance(&contract_id),
        1_000_0000000_i128
    );

    // Approve and release remaining milestones
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    assert!(client.release_milestone(&contract_id, &client_addr, &1));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &2));
    assert!(client.release_milestone(&contract_id, &client_addr, &2));

    let contract = client.get_contract(&contract_id);
    assert_contract_state(
        contract,
        ContractStatus::Completed,
        total_milestone_amount(),
        total_milestone_amount(),
        0,
    );
    assert_eq!(client.get_refundable_balance(&contract_id), 0);
}

/// Tests that release is rejected when insufficient funds are available.
/// 
/// # Security
/// - Prevents overdraft attacks
/// - Validates balance checks before release
#[test]
#[should_panic]
fn rejects_release_without_sufficient_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &100_0000000_i128));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    client.release_milestone(&contract_id, &client_addr, &0);
}

/// Tests that release of invalid milestone index is rejected.
/// 
/// # Security
/// - Prevents out-of-bounds access
/// - Validates milestone index bounds
#[test]
#[should_panic]
fn rejects_release_of_invalid_milestone() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &3));
    client.release_milestone(&contract_id, &client_addr, &3);
}

/// Tests that releasing a refunded milestone is rejected.
/// 
/// # Security
/// - Prevents double-spending
/// - Validates milestone state before release
#[test]
#[should_panic]
fn rejects_releasing_refunded_milestone() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    let refund_ids = vec![&env, 1_u32];
    client.refund_unreleased_milestones(&contract_id, &refund_ids);

    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    client.release_milestone(&contract_id, &client_addr, &1);
}

/// Tests that releasing the same milestone twice is rejected.
/// 
/// # Security
/// - Prevents double-spending
/// - Validates milestone released flag
#[test]
#[should_panic]
fn rejects_releasing_same_milestone_twice() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));

    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    client.release_milestone(&contract_id, &client_addr, &0);
}

// =========================================================================
// NEGATIVE-PATH TESTS FOR Issue #405
// =========================================================================

/// Tests that release_milestone panics with ContractNotFound for unknown contract id.
///
/// Asserts the exact error code when contract does not exist in storage.
/// 
/// # Security
/// - Prevents operations on non-existent contracts
/// - Ensures fail-closed behavior for invalid contract IDs
#[test]
fn test_release_contract_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let caller = Address::generate(&env);

    // Use a contract_id that was never created
    let invalid_contract_id = 9999_u32;
    let result = client.try_release_milestone(&invalid_contract_id, &caller, &0_u32);
    assert_contract_error(result, EscrowError::ContractNotFound);
}

/// Tests that release_milestone panics with UnauthorizedRole when caller is not authorized.
///
/// Asserts the exact error code when unauthorized address attempts to release.
/// 
/// # Security
/// - Prevents unauthorized milestone releases
/// - Enforces release authorization model
#[test]
fn test_release_unauthorized_role() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Fund the contract
    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));

    // Attempt release from wrong caller (a third party, not client)
    let wrong_caller = Address::generate(&env);
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    let result = client.try_release_milestone(&contract_id, &wrong_caller, &0_u32);
    assert_contract_error(result, EscrowError::UnauthorizedRole);
}

/// Tests that release_milestone panics with InvalidState when contract is not in Funded state.
///
/// Asserts the exact error code when attempting to release before funding.
/// 
/// # Security
/// - Prevents state machine violations
/// - Ensures funds must be deposited before release
#[test]
fn test_release_invalid_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Try to release without funding (contract is still in Created state)
    let result = client.try_release_milestone(&contract_id, &client_addr, &0_u32);
    assert_contract_error(result, EscrowError::InvalidState);
}

/// Tests that release_milestone panics with MilestoneAlreadyReleased on duplicate release.
///
/// Asserts the exact error code when attempting to release an already-released milestone.
/// 
/// # Security
/// - Prevents double-spending of milestones
/// - Enforces idempotency check on milestone release state
#[test]
fn test_release_milestone_already_released() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Fund and release first milestone once
    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));

    // Try to release the same milestone again
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    let result = client.try_release_milestone(&contract_id, &client_addr, &0_u32);
    assert_contract_error(result, EscrowError::MilestoneAlreadyReleased);
}

/// Tests that release_milestone panics with AlreadyRefunded when contract was refunded.
///
/// Asserts the exact error code when attempting to release a refunded milestone.
/// 
/// # Security
/// - Prevents release of already-refunded milestones
/// - Enforces mutual exclusivity of release and refund states
#[test]
fn test_release_already_refunded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Fund the contract and refund first milestone
    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    let refund_ids = vec![&env, 0_u32];
    client.refund_unreleased_milestones(&contract_id, &refund_ids);

    // Try to release the refunded milestone
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    let result = client.try_release_milestone(&contract_id, &client_addr, &0_u32);
    assert_contract_error(result, EscrowError::AlreadyRefunded);
}

/// Tests that release_milestone panics with IndexOutOfBounds for invalid milestone index.
///
/// Asserts the exact error code when milestone index exceeds array bounds.
/// 
/// # Security
/// - Prevents out-of-bounds memory access
/// - Validates milestone index bounds before release
#[test]
fn test_release_index_out_of_bounds() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Fund the contract (has 3 milestones: indices 0, 1, 2)
    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));

    // Try to release with out-of-bounds index
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &99));
    let result = client.try_release_milestone(&contract_id, &client_addr, &99_u32);
    assert_contract_error(result, EscrowError::IndexOutOfBounds);
}

/// Tests that release_milestone panics with InsufficientFunds when contract balance is too low.
///
/// Asserts the exact error code when available balance is less than milestone amount.
/// 
/// # Security
/// - Prevents overdraft attacks
/// - Validates balance availability before release
#[test]
fn test_release_insufficient_funds() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Deposit only partial amount (insufficient to release first milestone which is 200_0000000)
    assert!(client.deposit_funds(&contract_id, &client_addr, &100_0000000_i128));

    // Try to release when funded_amount < milestone amount
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    let result = client.try_release_milestone(&contract_id, &client_addr, &0_u32);
    assert_contract_error(result, EscrowError::InsufficientFunds);
}

