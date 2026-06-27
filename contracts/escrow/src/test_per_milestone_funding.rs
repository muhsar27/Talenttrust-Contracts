#![cfg(test)]

use soroban_sdk::{testutils::Address as _, vec, Address, Env};

use crate::{ContractStatus, Escrow, EscrowClient, Milestone, ReleaseAuthorization};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn make_client(env: &Env) -> EscrowClient<'_> {
    let id = env.register(Escrow, ());
    EscrowClient::new(env, &id)
}

fn participants(env: &Env) -> (Address, Address) {
    (Address::generate(env), Address::generate(env))
}

/// Sum of per-milestone `funded_amount` across all milestones.
fn sum_milestone_funded(milestones: &soroban_sdk::Vec<Milestone>) -> i128 {
    milestones.iter().map(|m| m.funded_amount).sum()
}

/// Sum of per-milestone `refunded_amount` across all milestones.
fn sum_milestone_refunded(milestones: &soroban_sdk::Vec<Milestone>) -> i128 {
    milestones.iter().map(|m| m.refunded_amount).sum()
}

/// Assert the per-milestone invariant: sums reconcile to contract totals.
fn assert_per_milestone_invariant(client: &EscrowClient, id: u32) {
    let contract = client.get_contract(&id);
    let milestones = client.get_milestones(&id);
    let total_funded = sum_milestone_funded(&milestones);
    let total_refunded = sum_milestone_refunded(&milestones);
    assert_eq!(
        total_funded, contract.funded_amount,
        "per-milestone funded_amount sum {} != contract.funded_amount {}",
        total_funded, contract.funded_amount
    );
    assert_eq!(
        total_refunded, contract.refunded_amount,
        "per-milestone refunded_amount sum {} != contract.refunded_amount {}",
        total_refunded, contract.refunded_amount
    );
}

// ---------------------------------------------------------------------------
// Deposit – per-milestone funded_amount distribution
// ---------------------------------------------------------------------------

#[test]
fn deposit_distributes_funds_across_milestones_in_order() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 100_i128, 200_i128, 300_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    // Deposit 150: fills milestone[0] (100) + 50 toward milestone[1]
    client.deposit_funds(&id, &ca, &150_i128);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 100);
    assert_eq!(ms.get(1).unwrap().funded_amount, 50);
    assert_eq!(ms.get(2).unwrap().funded_amount, 0);
    assert_per_milestone_invariant(&client, id);

    // Deposit remaining 450: fills milestone[1] (remaining 150) + milestone[2] (300)
    client.deposit_funds(&id, &ca, &450_i128);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 100);
    assert_eq!(ms.get(1).unwrap().funded_amount, 200);
    assert_eq!(ms.get(2).unwrap().funded_amount, 300);
    assert_per_milestone_invariant(&client, id);

    assert_eq!(client.get_contract(&id).status, ContractStatus::Funded);
}

#[test]
fn deposit_exact_full_fills_all_milestones() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 100_i128, 200_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &300_i128);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 100);
    assert_eq!(ms.get(1).unwrap().funded_amount, 200);
    assert_per_milestone_invariant(&client, id);
}

#[test]
fn deposit_partial_only_fills_first_milestones() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 100_i128, 200_i128, 300_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &50_i128);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 50);
    assert_eq!(ms.get(1).unwrap().funded_amount, 0);
    assert_eq!(ms.get(2).unwrap().funded_amount, 0);
    assert_per_milestone_invariant(&client, id);
}

// ---------------------------------------------------------------------------
// Release – milestone.funded_amount is set to amount on release
// ---------------------------------------------------------------------------

#[test]
fn release_sets_milestone_funded_amount_to_amount() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 100_i128, 200_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &300_i128);

    // Approve and release milestone 0
    client.approve_milestone_release(&id, &ca, &0);
    client.release_milestone(&id, &ca, &0);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 100);
    assert_eq!(ms.get(0).unwrap().released, true);
    assert_per_milestone_invariant(&client, id);

    // Approve and release milestone 1
    client.approve_milestone_release(&id, &ca, &1);
    client.release_milestone(&id, &ca, &1);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(1).unwrap().funded_amount, 200);
    assert_per_milestone_invariant(&client, id);
}

// ---------------------------------------------------------------------------
// Refund – milestone.refunded_amount is set to amount on refund
// ---------------------------------------------------------------------------

#[test]
fn refund_sets_milestone_refunded_amount_to_amount() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 100_i128, 200_i128, 300_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &600_i128);

    // Refund milestone 1 only
    client.refund_unreleased_milestones(&id, &vec![&env, 1_u32]);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().refunded_amount, 0);
    assert_eq!(ms.get(0).unwrap().refunded, false);
    assert_eq!(ms.get(1).unwrap().refunded_amount, 200);
    assert_eq!(ms.get(1).unwrap().refunded, true);
    assert_eq!(ms.get(2).unwrap().refunded_amount, 0);
    assert_eq!(ms.get(2).unwrap().refunded, false);
    assert_per_milestone_invariant(&client, id);
}

#[test]
fn refund_multiple_milestones_sets_refunded_amounts() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 100_i128, 200_i128, 300_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &600_i128);

    // Refund milestones 0 and 2
    client.refund_unreleased_milestones(&id, &vec![&env, 0_u32, 2_u32]);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().refunded_amount, 100);
    assert_eq!(ms.get(0).unwrap().refunded, true);
    assert_eq!(ms.get(1).unwrap().refunded_amount, 0);
    assert_eq!(ms.get(1).unwrap().refunded, false);
    assert_eq!(ms.get(2).unwrap().refunded_amount, 300);
    assert_eq!(ms.get(2).unwrap().refunded, true);
    assert_per_milestone_invariant(&client, id);
}

#[test]
fn refund_all_milestones_sets_all_refunded_amounts() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 100_i128, 200_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &300_i128);
    client.refund_unreleased_milestones(&id, &vec![&env, 0_u32, 1_u32]);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().refunded_amount, 100);
    assert_eq!(ms.get(1).unwrap().refunded_amount, 200);
    assert_per_milestone_invariant(&client, id);
    assert_eq!(client.get_contract(&id).status, ContractStatus::Refunded);
}

// ---------------------------------------------------------------------------
// Mixed release + refund – per-milestone accounting consistency
// ---------------------------------------------------------------------------

#[test]
fn mixed_release_and_refund_maintains_invariant() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 100_i128, 200_i128, 300_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &600_i128);

    // Release milestone 0
    client.approve_milestone_release(&id, &ca, &0);
    client.release_milestone(&id, &ca, &0);
    assert_per_milestone_invariant(&client, id);

    // Refund milestone 2 (unreleased)
    client.refund_unreleased_milestones(&id, &vec![&env, 2_u32]);
    assert_per_milestone_invariant(&client, id);

    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 100);
    assert_eq!(ms.get(0).unwrap().released, true);
    assert_eq!(ms.get(0).unwrap().refunded_amount, 0);

    assert_eq!(ms.get(1).unwrap().funded_amount, 200);
    assert_eq!(ms.get(1).unwrap().refunded_amount, 0);
    assert_eq!(ms.get(1).unwrap().released, false);

    assert_eq!(ms.get(2).unwrap().refunded_amount, 300);
    assert_eq!(ms.get(2).unwrap().refunded, true);
    assert_eq!(ms.get(2).unwrap().funded_amount, 300);

    let contract = client.get_contract(&id);
    assert_eq!(contract.funded_amount, 600);
    assert_eq!(contract.released_amount, 100);
    assert_eq!(contract.refunded_amount, 300);
}

// ---------------------------------------------------------------------------
// Invariant holds through full lifecycle
// ---------------------------------------------------------------------------

#[test]
fn invariant_holds_from_deposit_to_complete() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 200_i128, 400_i128, 600_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &600_i128);
    assert_per_milestone_invariant(&client, id);

    client.deposit_funds(&id, &ca, &600_i128);
    assert_per_milestone_invariant(&client, id);

    client.approve_milestone_release(&id, &ca, &0);
    client.release_milestone(&id, &ca, &0);
    assert_per_milestone_invariant(&client, id);

    client.approve_milestone_release(&id, &ca, &1);
    client.release_milestone(&id, &ca, &1);
    assert_per_milestone_invariant(&client, id);

    client.approve_milestone_release(&id, &ca, &2);
    client.release_milestone(&id, &ca, &2);
    assert_per_milestone_invariant(&client, id);

    let contract = client.get_contract(&id);
    assert_eq!(contract.status, ContractStatus::Completed);
    assert_eq!(contract.released_amount, 1_200_0000000_i128);
    assert_eq!(client.get_refundable_balance(&id), 0);
}

#[test]
fn invariant_holds_with_partial_release_and_partial_refund() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 200_i128, 400_i128, 600_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &1_200_0000000_i128);
    assert_per_milestone_invariant(&client, id);

    // Release first milestone
    client.approve_milestone_release(&id, &ca, &0);
    client.release_milestone(&id, &ca, &0);
    assert_per_milestone_invariant(&client, id);

    // Refund third milestone
    client.refund_unreleased_milestones(&id, &vec![&env, 2_u32]);
    assert_per_milestone_invariant(&client, id);

    let contract = client.get_contract(&id);
    assert_eq!(contract.funded_amount, 1_200_0000000_i128);
    assert_eq!(contract.released_amount, 200_0000000_i128);
    assert_eq!(contract.refunded_amount, 600_0000000_i128);
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn single_milestone_contract_tracks_funded_and_refunded() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 500_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    client.deposit_funds(&id, &ca, &500_i128);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 500);
    assert_per_milestone_invariant(&client, id);

    client.refund_unreleased_milestones(&id, &vec![&env, 0_u32]);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().refunded_amount, 500);
    assert_per_milestone_invariant(&client, id);
    assert_eq!(client.get_contract(&id).status, ContractStatus::Refunded);
}

#[test]
fn incremental_deposits_distribute_in_order() {
    let env = make_env();
    let client = make_client(&env);
    let (ca, fa) = participants(&env);
    let id = client.create_contract(
        &ca,
        &fa,
        &None,
        &vec![&env, 100_i128, 100_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    // Deposit 50 – first milestone partially funded
    client.deposit_funds(&id, &ca, &50_i128);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 50);
    assert_eq!(ms.get(1).unwrap().funded_amount, 0);
    assert_per_milestone_invariant(&client, id);

    // Deposit 100 – fills milestone[0] (remaining 50) + milestone[1] (50)
    client.deposit_funds(&id, &ca, &100_i128);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 100);
    assert_eq!(ms.get(1).unwrap().funded_amount, 50);
    assert_per_milestone_invariant(&client, id);

    // Deposit 50 – fills milestone[1] (remaining 50)
    client.deposit_funds(&id, &ca, &50_i128);
    let ms = client.get_milestones(&id);
    assert_eq!(ms.get(0).unwrap().funded_amount, 100);
    assert_eq!(ms.get(1).unwrap().funded_amount, 100);
    assert_per_milestone_invariant(&client, id);
}
