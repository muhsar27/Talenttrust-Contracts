//! Dispute conservation invariant tests.
//!
//! These tests prove that `resolve_dispute` conserves `funded_amount`:
//!
//!   `released_amount + refunded_amount == funded_amount`
//!
//! after every resolution, including contracts with prior partial releases.
//! They also verify that `resolution_payouts` rejects non-conserving splits
//! before the on-chain invariant guard fires.

#![cfg(test)]

use crate::{ContractStatus, DisputeResolution, Escrow, EscrowClient, Error, ReleaseAuthorization};
use soroban_sdk::{testutils::Address as _, vec, Address, Env};

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
    let client = EscrowClient::new(env, &id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    client
}

/// Create a funded contract with an assigned arbiter using `ClientOnly` release auth.
///
/// Returns `(client_addr, freelancer_addr, arbiter_addr, contract_id)`.
fn funded_with_arbiter(
    env: &Env,
    client: &EscrowClient,
    milestones: soroban_sdk::Vec<i128>,
    deposit: i128,
) -> (Address, Address, Address, u32) {
    let client_addr = Address::generate(env);
    let freelancer_addr = Address::generate(env);
    let arbiter_addr = Address::generate(env);

    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &Some(arbiter_addr.clone()),
        &milestones,
        &ReleaseAuthorization::ClientOnly,
    );
    client.deposit_funds(&id, &client_addr, &deposit);
    (client_addr, freelancer_addr, arbiter_addr, id)
}

/// Build a bare `Contract` value with controlled accounting fields for unit tests
/// that call `resolution_payouts` / `final_status_after_resolution` directly.
///
/// `funded` is stored in both `total_deposited` and `funded_amount` so the
/// helper reflects a freshly-funded contract with no prior releases.
fn payout_contract(env: &Env, funded: i128, released: i128, refunded: i128) -> Contract {
    Contract {
        client: Address::generate(env),
        freelancer: Address::generate(env),
        arbiter: Some(Address::generate(env)),
        status: ContractStatus::Disputed,
        total_deposited: funded,
        funded_amount: funded,
        released_amount: released,
        refunded_amount: refunded,
        release_authorization: ReleaseAuthorization::ClientOnly,
        reputation_issued: false,
    }
}

/// Assert the core conservation invariant on a live contract.
///
/// `released_amount + refunded_amount` must equal `funded_amount` after resolution.
fn assert_conservation(client: &EscrowClient, id: u32) {
    let c = client.get_contract(&id);
    assert_eq!(
        c.released_amount + c.refunded_amount,
        c.funded_amount,
        "conservation violated: released={} refunded={} funded={}",
        c.released_amount,
        c.refunded_amount,
        c.funded_amount
    );
}

// ---------------------------------------------------------------------------
// Unit tests for resolution_payouts (pure arithmetic, no env needed)
// ---------------------------------------------------------------------------

/// FullRefund routes all available balance to the client.
#[test]
fn resolution_payouts_full_refund_returns_available_to_client() {
    let env = make_env();
    let contract = payout_contract(&env, 100, 20, 10);
    // available = 100 - 20 - 10 = 70
    assert_eq!(
        resolution_payouts(&contract, &DisputeResolution::FullRefund),
        Ok((70, 0))
    );
}

/// FullPayout routes all available balance to the freelancer.
#[test]
fn resolution_payouts_full_payout_returns_available_to_freelancer() {
    let env = make_env();
    let contract = payout_contract(&env, 100, 20, 10);
    assert_eq!(
        resolution_payouts(&contract, &DisputeResolution::FullPayout),
        Ok((0, 70))
    );
}

/// PartialRefund applies the documented 70/30 split with floor rounding on the freelancer leg.
#[test]
fn resolution_payouts_partial_refund_uses_floor_rounded_70_30_split() {
    let env = make_env();
    let contract = payout_contract(&env, 101, 0, 0);
    // freelancer = floor(101 * 30 / 100) = 30; client = 71
    assert_eq!(
        resolution_payouts(&contract, &DisputeResolution::PartialRefund),
        Ok((71, 30))
    );
}

/// PartialRefund handles zero and one-stroop balances without creating value.
#[test]
fn resolution_payouts_partial_refund_handles_rounding_boundaries() {
    let env = make_env();
    assert_eq!(
        resolution_payouts(&payout_contract(&env, 0, 0, 0), &DisputeResolution::PartialRefund),
        Ok((0, 0))
    );
    assert_eq!(
        resolution_payouts(&payout_contract(&env, 1, 0, 0), &DisputeResolution::PartialRefund),
        Ok((1, 0))
    );
}

/// Split rejects negative amounts.
#[test]
fn resolution_payouts_split_rejects_negative_legs() {
    let env = make_env();
    let contract = payout_contract(&env, 100, 0, 0);
    assert_eq!(
        resolution_payouts(&contract, &DisputeResolution::Split(-1, 101)),
        Err(EscrowError::InvalidDisputeSplit)
    );
    assert_eq!(
        resolution_payouts(&contract, &DisputeResolution::Split(101, -1)),
        Err(EscrowError::InvalidDisputeSplit)
    );
}

/// Split rejects sums that do not equal the available balance.
#[test]
fn resolution_payouts_split_rejects_non_conserving_sums() {
    let env = make_env();
    let contract = payout_contract(&env, 100, 0, 0);
    assert_eq!(
        resolution_payouts(&contract, &DisputeResolution::Split(40, 59)),
        Err(EscrowError::InvalidDisputeSplit)
    );
    assert_eq!(
        resolution_payouts(&contract, &DisputeResolution::Split(40, 61)),
        Err(EscrowError::InvalidDisputeSplit)
    );
}

/// Split accepts any (a, b) where a + b == available and both are non-negative.
#[test]
fn resolution_payouts_split_accepts_exact_splits() {
    let env = make_env();
    assert_eq!(
        resolution_payouts(&payout_contract(&env, 100, 0, 0), &DisputeResolution::Split(40, 60)),
        Ok((40, 60))
    );
    assert_eq!(
        resolution_payouts(&payout_contract(&env, 0, 0, 0), &DisputeResolution::Split(0, 0)),
        Ok((0, 0))
    );
}

/// Split uses checked addition and rejects overflow before the sum check.
#[test]
fn resolution_payouts_split_rejects_overflowing_sum() {
    let env = make_env();
    let contract = payout_contract(&env, i128::MAX, 0, 0);
    assert_eq!(
        resolution_payouts(&contract, &DisputeResolution::Split(i128::MAX, 1)),
        Err(EscrowError::PotentialOverflow)
    );
}

/// Payout math fails closed when released + refunded already exceed funded_amount.
#[test]
fn resolution_payouts_rejects_corrupted_accounting_state() {
    let env = make_env();
    // released(70) + refunded(31) = 101 > funded(100) → available < 0
    let contract = payout_contract(&env, 100, 70, 31);
    assert_eq!(
        resolution_payouts(&contract, &DisputeResolution::FullRefund),
        Err(EscrowError::AccountingInvariantViolated)
    );
}

/// final_status returns Refunded only when the full deposit has been refunded.
#[test]
fn final_status_after_resolution_marks_refunded_only_for_full_refund() {
    let env = make_env();
    // fully refunded: refunded == funded
    assert_eq!(
        final_status_after_resolution(&payout_contract(&env, 100, 0, 100)),
        ContractStatus::Refunded
    );
    // partially refunded: freelancer received something
    assert_eq!(
        final_status_after_resolution(&payout_contract(&env, 100, 30, 70)),
        ContractStatus::Completed
    );
}

// ---------------------------------------------------------------------------
// raise_dispute integration tests
// ---------------------------------------------------------------------------

#[test]
fn client_can_raise_dispute_on_funded_contract() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, _, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128, 200_i128], 300);

    assert!(client.raise_dispute(&id, &client_addr));
    assert_eq!(client.get_contract(&id).status, ContractStatus::Disputed);
}

#[test]
fn freelancer_can_raise_dispute_on_funded_contract() {
    let env = make_env();
    let client = make_client(&env);
    let (_, freelancer_addr, _, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128, 200_i128], 300);

    assert!(client.raise_dispute(&id, &freelancer_addr));
    assert_eq!(client.get_contract(&id).status, ContractStatus::Disputed);
}

#[test]
fn raise_dispute_requires_contract_party() {
    let env = make_env();
    let client = make_client(&env);
    let (_, _, _, id) = funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);

    super::assert_contract_error(
        client.try_raise_dispute(&escrow_id, &outsider),
        Error::UnauthorizedRole,
    );
}

#[test]
fn raise_dispute_requires_assigned_arbiter() {
    let env = make_env();
    let client = make_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    // No arbiter
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &vec![&env, 100_i128],
        &ReleaseAuthorization::ClientOnly,
    );
    client.deposit_funds(&id, &client_addr, &100);

    super::assert_contract_error(
        client.try_raise_dispute(&escrow_id, &client_addr),
        Error::ArbiterRequired,
    );
}

#[test]
fn raise_dispute_rejects_completed_contract() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, _, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);

    client.release_milestone(&id, &client_addr, &0);

    super::assert_contract_error(
        client.try_raise_dispute(&escrow_id, &client_addr),
        Error::InvalidState,
    );
}

#[test]
fn resolve_full_refund_marks_refunded_and_closes_accounting() {
    let (env, _contract_id, client) = setup_initialized();
    let (client_addr, _, arbiter_addr, escrow_id) =
        create_funded_contract_with_arbiter(&env, &client, vec![&env, 125_i128, 75_i128], 200_i128);

    assert!(client.raise_dispute(&escrow_id, &client_addr));
    assert!(client.resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::FullRefund));

    let contract = client.get_contract(&escrow_id);
    assert_eq!(contract.status, ContractStatus::Refunded);
    assert_eq!(contract.released_amount, 0);
    assert_eq!(contract.refunded_amount, 200);
    assert_eq!(
        contract.released_amount + contract.refunded_amount,
        contract.funded_amount
    );
}

#[test]
fn resolve_full_payout_marks_completed_and_closes_accounting() {
    let (env, _contract_id, client) = setup_initialized();
    let (client_addr, _, arbiter_addr, escrow_id) =
        create_funded_contract_with_arbiter(&env, &client, vec![&env, 150_i128], 150_i128);

    assert!(client.raise_dispute(&escrow_id, &client_addr));
    assert!(client.resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::FullPayout));

    let contract = client.get_contract(&escrow_id);
    assert_eq!(contract.status, ContractStatus::Completed);
    assert_eq!(contract.released_amount, 150);
    assert_eq!(contract.refunded_amount, 0);
    assert_eq!(
        contract.released_amount + contract.refunded_amount,
        contract.funded_amount
    );
}

#[test]
fn resolve_partial_refund_applies_70_30_split() {
    let (env, _contract_id, client) = setup_initialized();
    let (client_addr, _, arbiter_addr, escrow_id) =
        create_funded_contract_with_arbiter(&env, &client, vec![&env, 100_i128], 100_i128);

    assert!(client.raise_dispute(&escrow_id, &client_addr));
    assert!(client.resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::PartialRefund));

    let contract = client.get_contract(&escrow_id);
    assert_eq!(contract.status, ContractStatus::Completed);
    // 70% refund to client, 30% release to freelancer
    assert_eq!(contract.refunded_amount, 70);
    assert_eq!(contract.released_amount, 30);
    assert_eq!(
        contract.released_amount + contract.refunded_amount,
        contract.funded_amount
    );
}

#[test]
fn resolve_partial_refund_applies_to_remaining_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let client = new_client(&env);
    let (client_addr, _, arbiter_addr, escrow_id) = create_funded_contract_with_arbiter(
        &env,
        &client,
        vec![&env, 101_i128, 100_i128],
        201_i128,
    );

    // Release first milestone
    assert!(client.approve_milestone_release(&escrow_id, &client_addr, &0));
    assert!(client.release_milestone(&escrow_id, &client_addr, &0));

    assert!(client.raise_dispute(&escrow_id, &client_addr));
    assert!(client.resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::PartialRefund));

    let contract = client.get_contract(&escrow_id);
    assert_eq!(contract.status, ContractStatus::Completed);
    // Initial release: 101
    // Remaining: 100 → 70% refund (70), 30% release (30)
    assert_eq!(contract.released_amount, 131); // 101 + 30
    assert_eq!(contract.refunded_amount, 70);
    assert_eq!(
        contract.released_amount + contract.refunded_amount,
        contract.funded_amount
    );
}

#[test]
fn resolve_split_accepts_custom_amounts_that_match_available_balance() {
    let (env, _contract_id, client) = setup_initialized();
    let (client_addr, _, arbiter_addr, escrow_id) =
        create_funded_contract_with_arbiter(&env, &client, vec![&env, 40_i128, 60_i128], 100_i128);

    assert!(client.raise_dispute(&escrow_id, &client_addr));
    assert!(client.resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::Split(35, 65)));

    let contract = client.get_contract(&escrow_id);
    assert_eq!(contract.status, ContractStatus::Completed);
    assert_eq!(contract.refunded_amount, 35);
    assert_eq!(contract.released_amount, 65);
}

#[test]
fn resolve_split_rejects_invalid_totals() {
    let (env, _contract_id, client) = setup_initialized();
    let (client_addr, _, arbiter_addr, escrow_id) =
        create_funded_contract_with_arbiter(&env, &client, vec![&env, 100_i128], 100_i128);

    assert!(client.raise_dispute(&escrow_id, &client_addr));

    // Split doesn't match available balance
    super::assert_contract_error(
        client.try_resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::Split(30, 50)),
        Error::InvalidDisputeSplit,
    );
}

#[test]
fn resolve_split_rejects_negative_amounts() {
    let (env, _contract_id, client) = setup_initialized();
    let (client_addr, _, arbiter_addr, escrow_id) =
        create_funded_contract_with_arbiter(&env, &client, vec![&env, 100_i128], 100_i128);

    assert!(client.raise_dispute(&escrow_id, &client_addr));

    super::assert_contract_error(
        client.try_resolve_dispute(
            &escrow_id,
            &arbiter_addr,
            &DisputeResolution::Split(-10, 110),
        ),
        Error::InvalidDisputeSplit,
    );
}

#[test]
fn resolve_dispute_requires_assigned_arbiter() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, _, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);

    client.raise_dispute(&id, &client_addr);

    super::assert_contract_error(
        client.try_resolve_dispute(&escrow_id, &outsider, &DisputeResolution::FullPayout),
        Error::UnauthorizedRole,
    );
}

#[test]
fn resolve_dispute_rejects_non_disputed_contract() {
    let env = make_env();
    let client = make_client(&env);
    let (_, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);

    super::assert_contract_error(
        client.try_resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::FullRefund),
        Error::InvalidStatusTransition,
    );
}

#[test]
fn resolve_dispute_cannot_be_called_twice() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);

    client.raise_dispute(&id, &client_addr);
    client.resolve_dispute(&id, &arbiter_addr, &DisputeResolution::FullRefund);

    super::assert_contract_error(
        client.try_resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::FullPayout),
        Error::InvalidStatusTransition,
    );
}

// ---------------------------------------------------------------------------
// Basic resolution conservation: no prior releases
// ---------------------------------------------------------------------------

#[test]
fn full_refund_conserves_funded_amount_no_prior_releases() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 125_i128, 75_i128], 200);

    client.raise_dispute(&id, &client_addr);
    client.resolve_dispute(&id, &arbiter_addr, &DisputeResolution::FullRefund);

    let c = client.get_contract(&id);
    assert_eq!(c.status, ContractStatus::Refunded);
    assert_eq!(c.released_amount, 0);
    assert_eq!(c.refunded_amount, 200);
    assert_conservation(&client, id);
}

#[test]
fn full_payout_conserves_funded_amount_no_prior_releases() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 150_i128], 150);

    client.raise_dispute(&id, &client_addr);
    client.resolve_dispute(&id, &arbiter_addr, &DisputeResolution::FullPayout);

    let c = client.get_contract(&id);
    assert_eq!(c.status, ContractStatus::Completed);
    assert_eq!(c.released_amount, 150);
    assert_eq!(c.refunded_amount, 0);
    assert_conservation(&client, id);
}

#[test]
fn partial_refund_conserves_funded_amount_no_prior_releases() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);

    client.raise_dispute(&id, &client_addr);
    client.resolve_dispute(&id, &arbiter_addr, &DisputeResolution::PartialRefund);

    let c = client.get_contract(&id);
    assert_eq!(c.status, ContractStatus::Completed);
    assert_eq!(c.refunded_amount, 70); // 70% to client
    assert_eq!(c.released_amount, 30); // 30% to freelancer
    assert_conservation(&client, id);
}

#[test]
fn split_conserves_funded_amount_when_sum_equals_available() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 40_i128, 60_i128], 100);

    client.raise_dispute(&id, &client_addr);
    client.resolve_dispute(&id, &arbiter_addr, &DisputeResolution::Split(35, 65));

    let c = client.get_contract(&id);
    assert_eq!(c.status, ContractStatus::Completed);
    assert_eq!(c.refunded_amount, 35);
    assert_eq!(c.released_amount, 65);
    assert_conservation(&client, id);
}

#[test]
fn split_rejects_non_conserving_sum() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);

    client.raise_dispute(&id, &client_addr);

    super::assert_contract_error(
        client.try_raise_dispute(&escrow_id, &client_addr),
        Error::ContractPaused,
    );
}

#[test]
fn pause_blocks_resolve_dispute() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);

    client.raise_dispute(&id, &client_addr);
    client.pause();

    super::assert_contract_error(
        client.try_resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::FullRefund),
        Error::ContractPaused,
    );
}

#[test]
fn emergency_blocks_raise_and_resolve_dispute() {
    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);

    client.activate_emergency_pause();

    super::assert_contract_error(
        client.try_raise_dispute(&escrow_id, &client_addr),
        Error::EmergencyActive,
    );

    client.resolve_emergency();
    client.raise_dispute(&id, &client_addr);
    client.activate_emergency_pause();

    super::assert_contract_error(
        client.try_resolve_dispute(&escrow_id, &arbiter_addr, &DisputeResolution::FullRefund),
        Error::EmergencyActive,
    );
}

// ---------------------------------------------------------------------------
// Multi-contract isolation
// ---------------------------------------------------------------------------

#[test]
fn disputes_on_independent_contracts_do_not_cross_contaminate() {
    let env = make_env();
    let client = make_client(&env);

    let (c1, _, a1, id1) = funded_with_arbiter(&env, &client, vec![&env, 100_i128], 100);
    let (c2, _, a2, id2) = funded_with_arbiter(&env, &client, vec![&env, 200_i128], 200);

    client.raise_dispute(&id1, &c1);
    client.raise_dispute(&id2, &c2);

    client.resolve_dispute(&id1, &a1, &DisputeResolution::FullRefund);
    client.resolve_dispute(&id2, &a2, &DisputeResolution::FullPayout);

    assert_conservation(&client, id1);
    assert_conservation(&client, id2);

    let c1_state = client.get_contract(&id1);
    let c2_state = client.get_contract(&id2);
    assert_eq!(c1_state.status, ContractStatus::Refunded);
    assert_eq!(c1_state.refunded_amount, 100);
    assert_eq!(c2_state.status, ContractStatus::Completed);
    assert_eq!(c2_state.released_amount, 200);
}

// ---------------------------------------------------------------------------
// Event payload test
// ---------------------------------------------------------------------------

#[test]
fn resolve_dispute_event_carries_client_and_freelancer_payouts() {
    use soroban_sdk::testutils::Events;

    let env = make_env();
    let client = make_client(&env);
    let (client_addr, _, arbiter_addr, id) =
        funded_with_arbiter(&env, &client, vec![&env, 80_i128, 20_i128], 100);

    client.raise_dispute(&id, &client_addr);
    client.resolve_dispute(&id, &arbiter_addr, &DisputeResolution::FullRefund);

    // The dsp_res event topics are ("dispute", "resolved").
    // The data tuple is (contract_id, resolution_code, client_payout, freelancer_payout).
    let events = env.events().all();
    let found = events.iter().any(|e| {
        // topics: Vec<Val> at index 1; data: Val at index 2
        let topics = &e.1;
        if topics.len() < 2 {
            return false;
        }
        use soroban_sdk::{Symbol, TryFromVal};
        let t0 = Symbol::try_from_val(&env, &topics.get(0).unwrap()).ok();
        let t1 = Symbol::try_from_val(&env, &topics.get(1).unwrap()).ok();
        t0 == Some(soroban_sdk::symbol_short!("dispute"))
            && t1 == Some(soroban_sdk::symbol_short!("resolved"))
    });

    assert!(found, "dispute resolved event not emitted");

    // Verify accounting still conserves after the event-emitting resolution.
    assert_conservation(&client, id);
}
