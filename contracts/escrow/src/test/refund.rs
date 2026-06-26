use soroban_sdk::vec;

use super::{
    assert_contract_state, assert_milestone_flags, create_client, create_default_contract, setup,
};
use crate::ContractStatus;

/// Tests that selected unreleased milestones can be refunded while preserving remaining balance.
/// 
/// # Security
/// - Validates refund accounting accuracy
/// - Ensures refunded_amount tracking is correct
/// - Verifies milestone refunded flag is set
/// - Confirms refundable balance calculation
#[test]
fn refunds_selected_unreleased_milestones_and_preserves_remaining_balance() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));

    let refund_ids = vec![&env, 1_u32];
    let refunded = client.refund_unreleased_milestones(&contract_id, &refund_ids);
    assert_eq!(refunded, 400_0000000_i128);

    let contract = client.get_contract(&contract_id);
    assert_contract_state(
        contract,
        ContractStatus::Funded,
        1_200_0000000_i128,
        200_0000000_i128,
        400_0000000_i128,
    );
    assert_milestone_flags(client.get_milestones(&contract_id), 1, false, true);
    assert_eq!(
        client.get_refundable_balance(&contract_id),
        600_0000000_i128
    );
}

/// Tests that contract transitions to Refunded status when all unreleased milestones are refunded.
/// 
/// # Security
/// - Validates state transition to Refunded
/// - Ensures all milestones are properly marked
/// - Confirms zero refundable balance
#[test]
fn marks_contract_refunded_when_all_unreleased_milestones_are_refunded() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    let refund_ids = vec![&env, 0_u32, 1_u32, 2_u32];
    let refunded = client.refund_unreleased_milestones(&contract_id, &refund_ids);
    assert_eq!(refunded, 1_200_0000000_i128);

    let contract = client.get_contract(&contract_id);
    assert_contract_state(
        contract,
        ContractStatus::Refunded,
        1_200_0000000_i128,
        0,
        1_200_0000000_i128,
    );
    assert_eq!(client.get_refundable_balance(&contract_id), 0);
}

/// Tests that empty refund requests are rejected.
/// 
/// # Security
/// - Prevents invalid state transitions
/// - Validates input sanitization
#[test]
#[should_panic]
fn rejects_empty_refund_request() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    let refund_ids = vec![&env];
    client.refund_unreleased_milestones(&contract_id, &refund_ids);
}

/// Tests that duplicate milestone indices in a single refund request are rejected.
/// 
/// # Security
/// - Prevents double-refund attacks
/// - Validates input sanitization
#[test]
#[should_panic]
fn rejects_duplicate_milestones_in_single_refund() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    let refund_ids = vec![&env, 1_u32, 1_u32];
    client.refund_unreleased_milestones(&contract_id, &refund_ids);
}

/// Tests that refunding a released milestone is rejected.
/// 
/// # Security
/// - Prevents double-spending
/// - Validates milestone state before refund
#[test]
#[should_panic]
fn rejects_refunding_released_milestone() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));

    let refund_ids = vec![&env, 0_u32];
    client.refund_unreleased_milestones(&contract_id, &refund_ids);
}

/// Tests that refunding the same milestone twice is rejected.
/// 
/// # Security
/// - Prevents double-refund attacks
/// - Validates milestone refunded flag
#[test]
#[should_panic]
fn rejects_refunding_same_milestone_twice() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &1_200_0000000_i128));
    let refund_ids = vec![&env, 2_u32];
    assert_eq!(
        client.refund_unreleased_milestones(&contract_id, &refund_ids),
        600_0000000_i128
    );

    client.refund_unreleased_milestones(&contract_id, &refund_ids);
}

/// Tests that refund is rejected when insufficient balance is available.
/// 
/// # Security
/// - Prevents overdraft attacks
/// - Validates balance checks before refund
#[test]
#[should_panic]
fn rejects_refund_when_balance_is_not_available() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let contract_id = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&contract_id, &client_addr, &200_0000000_i128));
    let refund_ids = vec![&env, 1_u32];
    client.refund_unreleased_milestones(&contract_id, &refund_ids);
}

// ---------------------------------------------------------------------------
// get_refundable_balance accounting invariant tests
//
// Core invariant: get_refundable_balance == funded_amount - released_amount - refunded_amount
//
// These tests drive a contract through mixed release/refund sequences and assert:
//  1. get_refundable_balance matches the computed formula at every step.
//  2. The value never goes negative.
//  3. It reaches zero only when all milestones are in a terminal state.
//  4. ContractStatus transitions are consistent with the balance.
// ---------------------------------------------------------------------------

/// Helper: asserts `get_refundable_balance == funded - released - refunded` and `>= 0`.
fn assert_balance_invariant(
    client: &crate::EscrowClient,
    contract_id: u32,
) {
    let c = client.get_contract(&contract_id);
    let reported = client.get_refundable_balance(&contract_id);
    let computed = c.funded_amount - c.released_amount - c.refunded_amount;
    assert!(reported >= 0, "refundable balance must never be negative (got {reported})");
    assert_eq!(
        reported, computed,
        "get_refundable_balance ({reported}) != funded({}) - released({}) - refunded({}) = {computed}",
        c.funded_amount, c.released_amount, c.refunded_amount,
    );
}

/// After funding but before any operation the full funded amount is refundable.
#[test]
fn refundable_balance_equals_funded_amount_before_any_operation() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let cid = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    assert!(client.deposit_funds(&cid, &client_addr, &1_200_0000000_i128));

    assert_eq!(client.get_refundable_balance(&cid), 1_200_0000000_i128);
    assert_balance_invariant(&client, cid);
}

/// Release-then-refund: release M0, refund M1, check balance at each step.
#[test]
fn balance_invariant_holds_after_release_then_refund() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let cid = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    assert!(client.deposit_funds(&cid, &client_addr, &1_200_0000000_i128));

    // Step 1 – approve and release M0 (200).
    assert!(client.approve_milestone_release(&cid, &client_addr, &0));
    assert!(client.release_milestone(&cid, &client_addr, &0));
    // balance = 1200 - 200 - 0 = 1000
    assert_eq!(client.get_refundable_balance(&cid), 1_000_0000000_i128);
    assert_balance_invariant(&client, cid);

    // Step 2 – refund M1 (400).
    let r = client.refund_unreleased_milestones(&cid, &vec![&env, 1_u32]);
    assert_eq!(r, 400_0000000_i128);
    // balance = 1200 - 200 - 400 = 600
    assert_eq!(client.get_refundable_balance(&cid), 600_0000000_i128);
    assert_balance_invariant(&client, cid);
    // M2 still unreleased → status remains Funded.
    assert_eq!(client.get_contract(&cid).status, crate::ContractStatus::Funded);
}

/// Refund-then-release: refund M2 first, then release M0, then M1.
#[test]
fn balance_invariant_holds_after_refund_then_release() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let cid = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    assert!(client.deposit_funds(&cid, &client_addr, &1_200_0000000_i128));

    // Step 1 – refund M2 (600).
    let r = client.refund_unreleased_milestones(&cid, &vec![&env, 2_u32]);
    assert_eq!(r, 600_0000000_i128);
    assert_eq!(client.get_refundable_balance(&cid), 600_0000000_i128);
    assert_balance_invariant(&client, cid);

    // Step 2 – release M0 (200).
    assert!(client.approve_milestone_release(&cid, &client_addr, &0));
    assert!(client.release_milestone(&cid, &client_addr, &0));
    assert_eq!(client.get_refundable_balance(&cid), 400_0000000_i128);
    assert_balance_invariant(&client, cid);

    // Step 3 – release M1 (400) → all terminal, Completed.
    assert!(client.approve_milestone_release(&cid, &client_addr, &1));
    assert!(client.release_milestone(&cid, &client_addr, &1));
    assert_eq!(client.get_refundable_balance(&cid), 0);
    assert_balance_invariant(&client, cid);
    assert_eq!(client.get_contract(&cid).status, crate::ContractStatus::Completed);
}

/// All-released: every milestone is released; balance reaches zero and status is Completed.
#[test]
fn balance_reaches_zero_when_all_milestones_released() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let cid = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    assert!(client.deposit_funds(&cid, &client_addr, &1_200_0000000_i128));

    for idx in [0_u32, 1, 2] {
        assert!(client.approve_milestone_release(&cid, &client_addr, &idx));
        assert!(client.release_milestone(&cid, &client_addr, &idx));
        assert_balance_invariant(&client, cid);
    }

    assert_eq!(client.get_refundable_balance(&cid), 0);
    assert_eq!(client.get_contract(&cid).status, crate::ContractStatus::Completed);
}

/// All-refunded: every milestone is refunded; balance reaches zero and status is Refunded.
#[test]
fn balance_reaches_zero_when_all_milestones_refunded() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let cid = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    assert!(client.deposit_funds(&cid, &client_addr, &1_200_0000000_i128));

    let all = vec![&env, 0_u32, 1_u32, 2_u32];
    let r = client.refund_unreleased_milestones(&cid, &all);
    assert_eq!(r, 1_200_0000000_i128);
    assert_balance_invariant(&client, cid);

    assert_eq!(client.get_refundable_balance(&cid), 0);
    assert_eq!(client.get_contract(&cid).status, crate::ContractStatus::Refunded);
}

/// Interleaved alternating release/refund across all three milestones.
/// Asserts the invariant after every individual operation.
#[test]
fn balance_invariant_holds_across_every_interleaved_operation() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let cid = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    assert!(client.deposit_funds(&cid, &client_addr, &1_200_0000000_i128));

    // M1 refunded first.
    client.refund_unreleased_milestones(&cid, &vec![&env, 1_u32]);
    assert_balance_invariant(&client, cid);
    assert!(client.get_refundable_balance(&cid) >= 0);

    // M0 released.
    assert!(client.approve_milestone_release(&cid, &client_addr, &0));
    assert!(client.release_milestone(&cid, &client_addr, &0));
    assert_balance_invariant(&client, cid);
    assert!(client.get_refundable_balance(&cid) >= 0);

    // M2 refunded → all terminal.
    client.refund_unreleased_milestones(&cid, &vec![&env, 2_u32]);
    assert_balance_invariant(&client, cid);
    assert_eq!(client.get_refundable_balance(&cid), 0);

    // Status is Completed (one released, two refunded).
    assert_eq!(client.get_contract(&cid).status, crate::ContractStatus::Completed);
}

/// Cross-check: get_refundable_balance is consistent with get_contract fields.
#[test]
fn get_refundable_balance_is_consistent_with_get_contract_fields() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let cid = create_default_contract(&env, &client, &client_addr, &freelancer_addr);
    assert!(client.deposit_funds(&cid, &client_addr, &1_200_0000000_i128));

    // Release M0 and refund M1.
    assert!(client.approve_milestone_release(&cid, &client_addr, &0));
    assert!(client.release_milestone(&cid, &client_addr, &0));
    client.refund_unreleased_milestones(&cid, &vec![&env, 1_u32]);

    let c = client.get_contract(&cid);
    let balance = client.get_refundable_balance(&cid);

    // Explicit cross-check against stored fields.
    assert_eq!(balance, c.funded_amount - c.released_amount - c.refunded_amount);
    assert_eq!(balance, 600_0000000_i128);

    // M2 is the only outstanding milestone.
    let milestones = client.get_milestones(&cid);
    let m2 = milestones.get(2).unwrap();
    assert!(!m2.released);
    assert!(!m2.refunded);
    assert_eq!(m2.amount, balance);
}

/// Balance never goes negative even when only some milestones are funded (partial deposit).
#[test]
fn balance_never_negative_with_partial_deposit_and_partial_refund() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    let cid = create_default_contract(&env, &client, &client_addr, &freelancer_addr);

    // Fund only M0 (200).
    assert!(client.deposit_funds(&cid, &client_addr, &200_0000000_i128));
    assert_balance_invariant(&client, cid);

    // Refund M0.
    client.refund_unreleased_milestones(&cid, &vec![&env, 0_u32]);
    assert_balance_invariant(&client, cid);
    assert_eq!(client.get_refundable_balance(&cid), 0);
}
