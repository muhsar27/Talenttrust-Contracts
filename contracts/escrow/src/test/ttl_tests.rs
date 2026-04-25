#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

use crate::{
    Escrow, EscrowClient, PENDING_APPROVAL_BUMP_THRESHOLD, PENDING_APPROVAL_TTL_LEDGERS,
    PENDING_MIGRATION_TTL_LEDGERS,
};

fn new_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.max_entry_ttl = PENDING_MIGRATION_TTL_LEDGERS * 4;
        li.min_persistent_entry_ttl = PENDING_MIGRATION_TTL_LEDGERS * 4;
    });
    env
}

fn advance_sequence(env: &Env, by: u32) {
    env.ledger().with_mut(|li| {
        li.sequence_number = li.sequence_number.saturating_add(by);
    });
}

fn register_client(env: &Env) -> EscrowClient<'_> {
    let contract_id = env.register(Escrow, ());
    EscrowClient::new(env, &contract_id)
}

fn sample_wasm_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[7u8; 32])
}

#[test]
fn pending_approval_readable_before_expiry() {
    let env = new_env();
    let client = register_client(&env);
    let approver = Address::generate(&env);
    let starting_sequence = env.ledger().sequence();

    client.request_approval(&approver, &1);
    advance_sequence(&env, PENDING_APPROVAL_TTL_LEDGERS - 1);

    let pending = client.get_pending_approval(&1);
    assert!(pending.is_some(), "approval should be live before expiry");
    let pending = pending.unwrap();
    assert_eq!(pending.approver, approver);
    assert_eq!(pending.contract_id, 1);
    assert_eq!(pending.requested_at_ledger, starting_sequence);
    assert_eq!(
        pending.expires_at_ledger,
        starting_sequence + PENDING_APPROVAL_TTL_LEDGERS
    );
}

#[test]
fn pending_approval_evicted_after_expiry() {
    let env = new_env();
    let client = register_client(&env);
    let approver = Address::generate(&env);

    client.request_approval(&approver, &1);
    advance_sequence(&env, PENDING_APPROVAL_TTL_LEDGERS + 1);

    assert!(client.get_pending_approval(&1).is_none());
}

#[test]
fn pending_migration_readable_before_expiry() {
    let env = new_env();
    let client = register_client(&env);
    let proposer = Address::generate(&env);
    let hash = sample_wasm_hash(&env);
    let starting_sequence = env.ledger().sequence();

    client.request_migration(&proposer, &hash);
    advance_sequence(&env, PENDING_MIGRATION_TTL_LEDGERS - 1);

    let pending = client.get_pending_migration();
    assert!(pending.is_some());
    let pending = pending.unwrap();
    assert_eq!(pending.proposer, proposer);
    assert_eq!(pending.new_wasm_hash, hash);
    assert_eq!(
        pending.expires_at_ledger,
        starting_sequence + PENDING_MIGRATION_TTL_LEDGERS
    );
}

#[test]
fn pending_migration_evicted_after_expiry() {
    let env = new_env();
    let client = register_client(&env);
    let proposer = Address::generate(&env);

    client.request_migration(&proposer, &sample_wasm_hash(&env));
    advance_sequence(&env, PENDING_MIGRATION_TTL_LEDGERS + 1);

    assert!(client.get_pending_migration().is_none());
}

#[test]
fn extend_if_below_threshold_bumps_when_near_expiry() {
    let env = new_env();
    let client = register_client(&env);
    let approver = Address::generate(&env);

    client.request_approval(&approver, &1);

    advance_sequence(
        &env,
        PENDING_APPROVAL_TTL_LEDGERS - PENDING_APPROVAL_BUMP_THRESHOLD + 1,
    );

    let extended = client.extend_pending_approval(&approver, &1);
    assert!(extended);

    advance_sequence(&env, PENDING_APPROVAL_BUMP_THRESHOLD + 1);
    assert!(
        client.get_pending_approval(&1).is_some(),
        "entry should survive past original expiry after extension"
    );
}

#[test]
fn extend_if_below_threshold_noop_when_fresh() {
    let env = new_env();
    let client = register_client(&env);
    let approver = Address::generate(&env);

    client.request_approval(&approver, &1);
    let ok = client.extend_pending_approval(&approver, &1);
    assert!(ok, "call succeeds even when already fresh");

    advance_sequence(&env, PENDING_APPROVAL_TTL_LEDGERS - 1);
    assert!(client.get_pending_approval(&1).is_some());
}

#[test]
fn extend_returns_false_when_key_absent() {
    let env = new_env();
    let client = register_client(&env);
    let approver = Address::generate(&env);

    let result = client.extend_pending_approval(&approver, &42);
    assert!(!result);
}

#[test]
fn deterministic_expiry() {
    let env_a = new_env();
    let env_b = new_env();
    let client_a = register_client(&env_a);
    let client_b = register_client(&env_b);

    let approver_a = Address::generate(&env_a);
    let approver_b = Address::generate(&env_b);

    let a = client_a.request_approval(&approver_a, &7);
    let b = client_b.request_approval(&approver_b, &7);

    assert_eq!(a.requested_at_ledger, b.requested_at_ledger);
    assert_eq!(a.expires_at_ledger, b.expires_at_ledger);
    assert_eq!(
        a.expires_at_ledger,
        a.requested_at_ledger + PENDING_APPROVAL_TTL_LEDGERS
    );
}

#[test]
fn cancel_removes_pending_approval() {
    let env = new_env();
    let client = register_client(&env);
    let approver = Address::generate(&env);

    client.request_approval(&approver, &1);
    assert!(client.get_pending_approval(&1).is_some());

    client.cancel_approval(&approver, &1);
    assert!(client.get_pending_approval(&1).is_none());
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn duplicate_request_approval_rejects() {
    let env = new_env();
    let client = register_client(&env);
    let approver = Address::generate(&env);

    client.request_approval(&approver, &1);
    client.request_approval(&approver, &1);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn duplicate_request_migration_rejects() {
    let env = new_env();
    let client = register_client(&env);
    let proposer = Address::generate(&env);
    let hash = sample_wasm_hash(&env);

    client.request_migration(&proposer, &hash);
    client.request_migration(&proposer, &hash);
}

#[test]
fn confirm_migration_clears_pending() {
    let env = new_env();
    let client = register_client(&env);
    let proposer = Address::generate(&env);
    let confirmer = Address::generate(&env);

    client.request_migration(&proposer, &sample_wasm_hash(&env));
    client.confirm_migration(&confirmer);

    assert!(client.get_pending_migration().is_none());
}
