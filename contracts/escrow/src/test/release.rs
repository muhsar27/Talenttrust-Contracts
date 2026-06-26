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

/// Tests that batch release of milestones works correctly and contract completes when all are released.
/// 
/// # Security
/// - Validates atomicity (all-or-nothing release)
/// - Validates authorization checks
/// - Ensures released_amount tracking is accurate
/// - Verifies state transition to Completed
/// - Confirms refundable balance calculation
#[test]
fn batch_releases_funded_milestones_and_completes_when_all_are_released() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));

    // Approve all milestones
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &2));

    // Batch release all milestones
    let milestone_indices = vec![&env, 0_u32, 1_u32, 2_u32];
    let total_released = client.release_milestones(&contract_id, &client_addr, &milestone_indices);
    assert_eq!(total_released, 1_200_0000000_i128);

    let contract = client.get_contract(&contract_id);
    assert_contract_state(
        contract,
        ContractStatus::Completed,
        1_200_0000000_i128,
        1_200_0000000_i128,
        0,
    );
    assert_milestone_flags(client.get_milestones(&contract_id), 0, true, false);
    assert_milestone_flags(client.get_milestones(&contract_id), 1, true, false);
    assert_milestone_flags(client.get_milestones(&contract_id), 2, true, false);
    assert_eq!(client.get_refundable_balance(&contract_id), 0);
}

/// Tests that batch release is rejected with empty milestone indices.
#[test]
#[should_panic]
fn rejects_batch_release_with_empty_indices() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    let milestone_indices = vec![&env];
    client.release_milestones(&contract_id, &client_addr, &milestone_indices);
}

/// Tests that batch release is rejected with duplicate milestone indices.
#[test]
#[should_panic]
fn rejects_batch_release_with_duplicate_indices() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    let milestone_indices = vec![&env, 0_u32, 0_u32];
    client.release_milestones(&contract_id, &client_addr, &milestone_indices);
}

/// Tests that batch release is rejected when insufficient funds are available for all milestones.
#[test]
#[should_panic]
fn rejects_batch_release_without_sufficient_balance() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &500_0000000_i128));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    let milestone_indices = vec![&env, 0_u32, 1_u32];
    client.release_milestones(&contract_id, &client_addr, &milestone_indices);
}

/// Tests that batch release is rejected with invalid milestone index.
#[test]
#[should_panic]
fn rejects_batch_release_of_invalid_milestone() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &3));
    let milestone_indices = vec![&env, 0_u32, 3_u32];
    client.release_milestones(&contract_id, &client_addr, &milestone_indices);
}

/// Tests that batch release is rejected if any milestone is already released.
#[test]
#[should_panic]
fn rejects_batch_release_with_already_released_milestone() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    let milestone_indices = vec![&env, 0_u32, 1_u32];
    client.release_milestones(&contract_id, &client_addr, &milestone_indices);
}

/// Tests that batch release is rejected if any milestone is already refunded.
#[test]
#[should_panic]
fn rejects_batch_release_with_refunded_milestone() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    let refund_ids = vec![&env, 1_u32];
    client.refund_unreleased_milestones(&contract_id, &refund_ids);
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    let milestone_indices = vec![&env, 0_u32, 1_u32];
    client.release_milestones(&contract_id, &client_addr, &milestone_indices);
}

/// Tests that batch release is rejected if any milestone lacks valid approvals.
#[test]
#[should_panic]
fn rejects_batch_release_with_insufficient_approvals() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    // Don't approve milestone 1
    let milestone_indices = vec![&env, 0_u32, 1_u32];
    client.release_milestones(&contract_id, &client_addr, &milestone_indices);
}
