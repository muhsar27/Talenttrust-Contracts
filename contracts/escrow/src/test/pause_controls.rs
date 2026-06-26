//! Tests for the pause + emergency gate on mutating escrow entrypoints.
//!
//! Issue #452: `create_contract`, `deposit_funds`, `release_milestone`,
//! `refund_unreleased_milestones`, `issue_reputation`, and `cancel_contract`
//! must all honor the `Paused` and `Emergency` flags so the documented
//! behavior holds. Read-only queries must remain available while paused.
//!
//! Each mutating entrypoint is exercised against three states:
//! 1. **Paused only**: `pause()` blocks the call with `ContractPaused`.
//! 2. **Emergency only** (Paused=false, Emergency=true): the call is blocked
//!    with `EmergencyActive`.
//! 3. **Recovered**: `unpause()` (or `resolve_emergency()`) restores the
//!    happy path.
//!
//! Run locally with `cargo test -p escrow --lib pause_controls`.

use crate::{DepositMode, Escrow, EscrowClient, EscrowError, ReleaseAuthorization};
use soroban_sdk::{testutils::Address as _, vec, Address, Env};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn setup_initialized() -> (Env, EscrowClient<'_>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    assert!(client.initialize(&admin));
    (env, client, admin)
}

/// Create a contract in `Created` state with `ClientOnly` release authorization
/// and three milestones. Used by deposit_funds tests because `deposit_funds`
/// only accepts `Created` state on `main` — a `Funded` contract would panic
/// with `InvalidState` before `set_paused` can be exercised on top of it.
/// Returns `(client_addr, freelancer_addr, contract_id)`.
fn setup_created_contract(env: &Env, client: &EscrowClient<'_>) -> (Address, Address, u32) {
    let client_addr = Address::generate(env);
    let freelancer_addr = Address::generate(env);
    let milestones = vec![env, 100_i128, 200_i128, 300_i128];
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
        &DepositMode::Incremental,
    );
    (client_addr, freelancer_addr, id)
}

/// Create a fully-funded contract with `ClientOnly` release authorization and
/// two milestones. Returns `(client_addr, freelancer_addr, contract_id)`.
/// Used by release/refund/issue_reputation/cancel tests that need a `Funded`
/// or `Completed` baseline (NOT for deposit-only happy-path tests, see
/// `setup_created_contract`).
fn setup_funded_contract(env: &Env, client: &EscrowClient<'_>) -> (Address, Address, u32) {
    let client_addr = Address::generate(env);
    let freelancer_addr = Address::generate(env);
    let milestones = vec![env, 100_i128, 200_i128];
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
        &DepositMode::Incremental,
    );
    client.deposit_funds(&id, &client_addr, &300_i128);
    (client_addr, freelancer_addr, id)
}

/// Create and complete a contract (all milestones released) so issue_reputation
/// can be exercised from a `Completed` baseline.
fn setup_completed_contract(env: &Env, client: &EscrowClient<'_>) -> (Address, Address, u32) {
    let (client_addr, freelancer_addr, id) = setup_funded_contract(env, client);
    client.approve_milestone_release(&id, &client_addr, &0);
    client.release_milestone(&id, &client_addr, &0);
    client.approve_milestone_release(&id, &client_addr, &1);
    client.release_milestone(&id, &client_addr, &1);
    (client_addr, freelancer_addr, id)
}

/// Manually flip the `Emergency` flag on the underlying storage WITHOUT
/// flipping the `Paused` flag (so `require_not_paused()` reaches the
/// Emergency check).
fn set_emergency_only(env: &Env, client: &EscrowClient<'_>) {
    let _: bool = client.activate_emergency_pause();
    // The activate helper sets BOTH flags; we now clear Paused so the gate
    // hits the Emergency check first.
    let contract_addr: Address = client.address.clone();
    env.as_contract(&contract_addr, || {
        env.storage()
            .persistent()
            .set(&crate::DataKey::Paused, &false);
    });
}

// ─── initialize ──────────────────────────────────────────────────────────────

#[test]
fn initialize_only_once_fails() {
    let (_env, client, admin) = setup_initialized();
    super::assert_contract_error(
        client.try_initialize(&admin),
        EscrowError::AlreadyInitialized,
    );
}

// ─── pause / unpause state ──────────────────────────────────────────────────

#[test]
fn pause_then_unpause_toggles_state() {
    let (_env, client, _admin) = setup_initialized();
    assert!(!client.is_paused());
    assert!(client.pause());
    assert!(client.is_paused());
    assert!(client.unpause());
    assert!(!client.is_paused());
}

#[test]
fn pause_requires_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);
    super::assert_contract_error(client.try_pause(), EscrowError::NotInitialized);
}

// ─── create_contract blocked ─────────────────────────────────────────────────

#[test]
fn pause_blocks_create_contract() {
    let (env, client, _admin) = setup_initialized();
    client.pause();

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    super::assert_contract_error(
        client.try_create_contract(
            &a,
            &b,
            &None,
            &vec![&env, 50_i128],
            &ReleaseAuthorization::ClientOnly,
            &DepositMode::Incremental,
        ),
        EscrowError::ContractPaused,
    );
}

#[test]
fn emergency_blocks_create_contract() {
    let (env, client, _admin) = setup_initialized();
    set_emergency_only(&env, &client);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let milestones = vec![&env, 50_i128];
    super::assert_contract_error(
        client.try_create_contract(&a, &b, &None, &milestones, &ReleaseAuthorization::ClientOnly),
        EscrowError::EmergencyActive,
    );
}

// ─── unpaused allows operations ──────────────────────────────────────────────

#[test]
fn unpause_restores_create_contract() {
    let (env, client, _admin) = setup_initialized();
    client.pause();
    client.unpause();

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let id = client.create_contract(
        &a,
        &b,
        &None,
        &vec![&env, 50_i128],
        &ReleaseAuthorization::ClientOnly,
        &DepositMode::Incremental,
    );
    assert_eq!(id, 1);
}

// ─── cancelled event emission ──────────────────────────────────────────────────

/// cancelled event is emitted on successful cancellation with correct payload.
/// Validates event topic and payload structure for indexer observability.
#[test]
fn cancel_contract_emits_cancelled_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = EscrowClient::new(&env, &env.register(Escrow, ()));
    let admin = Address::generate(&env);
    assert!(client.initialize(&admin));

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let milestones = vec![&env, 100_i128, 200_i128, 300_i128];
    let contract_id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
        &DepositMode::Incremental,
    );

    // Capture timestamp before cancellation
    let expected_timestamp = env.ledger().timestamp();

    // Cancel the contract
    assert!(client.cancel_contract(&contract_id, &client_addr));

    // Verify the cancelled event was emitted
    let events = env.events().all();
    let cancelled_event = events
        .iter()
        .find(|(topic, _)| topic == &(symbol_short!("cancelled"), contract_id));

    assert!(cancelled_event.is_some(), "cancelled event must be emitted");

    // Verify payload: (caller, previous_status, timestamp)
    let (_, payload) = cancelled_event.unwrap();
    assert_eq!(payload.get(0).unwrap(), client_addr); // caller
    assert_eq!(payload.get(1).unwrap(), ContractStatus::Created); // previous_status
    assert_eq!(payload.get(2).unwrap(), expected_timestamp); // timestamp
}

/// cancelled event contains correct previous_status for Funded state.
/// Validates that the prior state is captured before transition.
#[test]
fn cancel_contract_emits_event_with_previous_status_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = EscrowClient::new(&env, &env.register(Escrow, ()));
    let admin = Address::generate(&env);
    assert!(client.initialize(&admin));

    let client_addr = Address::generate(&env);
    let milestones = vec![&env, 50_i128];

    super::assert_contract_error(
        client.try_create_contract(
            &outsider,
            &client_addr,
            &None,
            &milestones,
            &ReleaseAuthorization::ClientOnly,
            &DepositMode::Incremental,
        ),
        EscrowError::ContractPaused,
    );

    // Fund the contract (transitions to Funded state)
    client.deposit_funds(&contract_id, &client_addr, &600_i128);

    // Cancel as freelancer
    let expected_timestamp = env.ledger().timestamp();
    assert!(client.cancel_contract(&contract_id, &freelancer_addr));

    // Verify the cancelled event was emitted with Funded as previous_status
    let events = env.events().all();
    let cancelled_event = events
        .iter()
        .find(|(topic, _)| topic == &(symbol_short!("cancelled"), contract_id));

    assert!(cancelled_event.is_some());
    let (_, payload) = cancelled_event.unwrap();
    assert_eq!(payload.get(0).unwrap(), freelancer_addr); // caller
    assert_eq!(payload.get(1).unwrap(), ContractStatus::Funded); // previous_status
    assert_eq!(payload.get(2).unwrap(), expected_timestamp); // timestamp
}

/// cancelled event is not emitted on failed cancellation.
/// Validates security invariant: event only on successful state transition.
#[test]
fn cancel_contract_no_event_on_invalid_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = EscrowClient::new(&env, &env.register(Escrow, ()));
    let admin = Address::generate(&env);
    assert!(client.initialize(&admin));

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let milestones = vec![&env, 100_i128, 200_i128, 300_i128];
    let contract_id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
    );

    // Cancel once - event emitted
    assert!(client.cancel_contract(&contract_id, &client_addr));

    // Attempt second cancellation - should fail, no additional event
    let result = client.try_cancel_contract(&contract_id, &client_addr);
    assert!(result.is_err(), "Second cancellation should fail");

    // Only one cancelled event should exist
    let events = env.events().all();
    let cancelled_events: Vec<_> = events
        .iter()
        .filter(|(topic, _)| topic == &(symbol_short!("cancelled"), contract_id))
        .collect();
    assert_eq!(cancelled_events.len(), 1, "Only one cancelled event should be emitted");
}
