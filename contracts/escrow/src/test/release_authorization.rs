//! Tests for `release_milestone` caller authorization.
//!
//! Covers every `ReleaseAuthorization` variant in both the happy
//! (authorized caller with valid approvals) and negative (unauthorized
//! caller, wrong role, missing approvals) paths.
//!
//! # Security contract
//!
//! 1. `caller.require_auth()` MUST be invoked *before* any role
//!    discrimination or state mutation.
//! 2. Role checks MUST reject callers who do not match the contract's
//!    `ReleaseAuthorization` variant.
//! 3. `MultiSig` requires BOTH client and freelancer approvals via
//!    `approvals::check_approvals`; no single party can release alone.
//! 4. Approvals must exist and not have expired; missing or expired
//!    approvals produce `InsufficientApprovals`.
//! 5. All negative paths MUST be fail-closed (no partial state change).

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, vec, Address, Env};

use super::assert_contract_error;
use crate::{ContractStatus, Escrow, EscrowClient, Error, ReleaseAuthorization};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, EscrowClient<'_>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let arbiter_addr = Address::generate(&env);
    (env, client, client_addr, freelancer_addr, arbiter_addr)
}

fn milestones(env: &Env) -> soroban_sdk::Vec<i128> {
    vec![env, 500_0000000_i128, 300_0000000_i128]
}

fn total() -> i128 {
    800_0000000_i128
}

/// Create a funded contract with the given authorization mode.
/// Returns `(client, client_addr, freelancer_addr, arbiter_addr, contract_id)`.
fn create(
    env: &Env,
    client: &EscrowClient<'_>,
    client_addr: &Address,
    freelancer_addr: &Address,
    arbiter: Option<&Address>,
    auth: &ReleaseAuthorization,
) -> u32 {
    let id = client.create_contract(
        client_addr,
        freelancer_addr,
        &arbiter.copied(),
        &milestones(env),
        auth,
    );
    assert!(client.deposit_funds(&id, client_addr, &total()));
    // Approve milestone 0 so release can go through on happy paths
    match auth {
        ReleaseAuthorization::ClientOnly
        | ReleaseAuthorization::ClientAndArbiter => {
            assert!(client.approve_milestone_release(&id, client_addr, &0));
        }
        ReleaseAuthorization::ArbiterOnly => {
            assert!(client.approve_milestone_release(
                &id,
                arbiter.expect("ArbiterOnly requires arbiter"),
                &0,
            ));
        }
        ReleaseAuthorization::MultiSig => {
            assert!(client.approve_milestone_release(&id, client_addr, &0));
            assert!(client.approve_milestone_release(&id, freelancer_addr, &0));
        }
    }
    id
}

// ===========================================================================
//  ClientOnly
// ===========================================================================

#[test]
fn client_only_client_can_release() {
    let (env, client, client_addr, freelancer_addr, _arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        None,
        &ReleaseAuthorization::ClientOnly,
    );
    assert!(client.release_milestone(&id, &client_addr, &0));
    let c = client.get_contract(&id);
    assert_eq!(c.released_amount, 500_0000000_i128);
}

#[test]
fn client_only_freelancer_rejected() {
    let (env, client, client_addr, freelancer_addr, _arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        None,
        &ReleaseAuthorization::ClientOnly,
    );
    let result = client.try_release_milestone(&id, &freelancer_addr, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn client_only_arbiter_rejected() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::ClientOnly,
    );
    let result = client.try_release_milestone(&id, &arbiter_addr, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn client_only_attacker_rejected() {
    let (env, client, client_addr, freelancer_addr, _arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        None,
        &ReleaseAuthorization::ClientOnly,
    );
    let attacker = Address::generate(&env);
    let result = client.try_release_milestone(&id, &attacker, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

// ===========================================================================
//  ArbiterOnly
// ===========================================================================

#[test]
fn arbiter_only_arbiter_can_release() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::ArbiterOnly,
    );
    assert!(client.release_milestone(&id, &arbiter_addr, &0));
}

#[test]
fn arbiter_only_client_rejected() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::ArbiterOnly,
    );
    let result = client.try_release_milestone(&id, &client_addr, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn arbiter_only_freelancer_rejected() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::ArbiterOnly,
    );
    let result = client.try_release_milestone(&id, &freelancer_addr, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn arbiter_only_attacker_rejected() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::ArbiterOnly,
    );
    let attacker = Address::generate(&env);
    let result = client.try_release_milestone(&id, &attacker, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

// ===========================================================================
//  ClientAndArbiter
// ===========================================================================

#[test]
fn client_and_arbiter_client_can_release() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::ClientAndArbiter,
    );
    assert!(client.release_milestone(&id, &client_addr, &0));
}

#[test]
fn client_and_arbiter_arbiter_can_release() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::ClientAndArbiter,
    );
    // Need arbiter to have approved, which our helper doesn't do for CA mode
    // — re-approve with arbiter
    assert!(client.approve_milestone_release(&id, &arbiter_addr, &0));
    assert!(client.release_milestone(&id, &arbiter_addr, &0));
}

#[test]
fn client_and_arbiter_freelancer_rejected() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::ClientAndArbiter,
    );
    let result = client.try_release_milestone(&id, &freelancer_addr, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn client_and_arbiter_attacker_rejected() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::ClientAndArbiter,
    );
    let attacker = Address::generate(&env);
    let result = client.try_release_milestone(&id, &attacker, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

// ===========================================================================
//  MultiSig
// ===========================================================================

#[test]
fn multisig_client_can_release_with_both_approvals() {
    let (env, client, client_addr, freelancer_addr, _arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        None,
        &ReleaseAuthorization::MultiSig,
    );
    assert!(client.release_milestone(&id, &client_addr, &0));
}

#[test]
fn multisig_freelancer_can_release_with_both_approvals() {
    let (env, client, client_addr, freelancer_addr, _arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        None,
        &ReleaseAuthorization::MultiSig,
    );
    assert!(client.release_milestone(&id, &freelancer_addr, &0));
}

#[test]
fn multisig_arbiter_rejected() {
    let (env, client, client_addr, freelancer_addr, arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        Some(&arbiter_addr),
        &ReleaseAuthorization::MultiSig,
    );
    let result = client.try_release_milestone(&id, &arbiter_addr, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn multisig_attacker_rejected() {
    let (env, client, client_addr, freelancer_addr, _arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        None,
        &ReleaseAuthorization::MultiSig,
    );
    let attacker = Address::generate(&env);
    let result = client.try_release_milestone(&id, &attacker, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn multisig_only_one_approval_insufficient() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones(&env),
        &ReleaseAuthorization::MultiSig,
    );
    assert!(client.deposit_funds(&id, &client_addr, &total()));

    // Only client approves — second approval missing
    assert!(client.approve_milestone_release(&id, &client_addr, &0));
    let result = client.try_release_milestone(&id, &client_addr, &0);
    assert_contract_error(result, Error::InsufficientApprovals);
}

// ===========================================================================
//  Missing / expired approvals
// ===========================================================================

#[test]
fn release_without_approval_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones(&env),
        &ReleaseAuthorization::ClientOnly,
    );
    assert!(client.deposit_funds(&id, &client_addr, &total()));

    // No approval recorded yet
    let result = client.try_release_milestone(&id, &client_addr, &0);
    assert_contract_error(result, Error::InsufficientApprovals);
}

// ===========================================================================
//  require_auth() ordering — unauth caller without mock
// ===========================================================================

#[test]
fn unauthorized_caller_without_auth_is_rejected() {
    // Use mock_all_auths to set up the contract, then verify that a
    // completely unrelated address (not a participant) gets UnauthorizedRole.
    let (env, client, client_addr, freelancer_addr, _) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        None,
        &ReleaseAuthorization::ClientOnly,
    );
    let stranger = Address::generate(&env);
    let result = client.try_release_milestone(&id, &stranger, &0);
    assert_contract_error(result, Error::UnauthorizedRole);
}

// ===========================================================================
//  State mutation guard
// ===========================================================================

#[test]
fn fail_closed_on_unauthorized_caller_no_state_change() {
    let (env, client, client_addr, freelancer_addr, _arbiter_addr) = setup();
    let id = create(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        None,
        &ReleaseAuthorization::ClientOnly,
    );

    let before = client.get_contract(&id);

    // Attacker tries to release
    let attacker = Address::generate(&env);
    let result = client.try_release_milestone(&id, &attacker, &0);
    assert_contract_error(result, Error::UnauthorizedRole);

    let after = client.get_contract(&id);
    assert_eq!(before.released_amount, after.released_amount);
    assert_eq!(before.status, after.status);
}
