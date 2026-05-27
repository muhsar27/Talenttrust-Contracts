//! # Release Authorization Modes Test Suite
//!
//! Tests for multi-party milestone release authorization:
//! - Client-only approval mode
//! - Client and freelancer dual approval mode
//! - Arbiter-only approval mode
//! - Approval event emission
//! - Duplicate approval prevention
//! - Unauthorized approval rejection

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, testutils::Events as _, vec, Address, Env};

use super::assert_contract_error;
use crate::{
    ContractStatus, Escrow, EscrowClient, EscrowError, ReleaseAuthorizationMode,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Register the contract and return a client.
fn register_client(env: &Env) -> EscrowClient {
    let id = env.register(Escrow, ());
    EscrowClient::new(env, &id)
}

/// Create a contract with specified authorization mode.
fn create_contract_with_mode(
    env: &Env,
    client: &EscrowClient,
    client_addr: &Address,
    freelancer_addr: &Address,
    arbiter_addr: &Option<Address>,
    mode: &ReleaseAuthorizationMode,
) -> u32 {
    let milestones = vec![env, 100_i128, 200_i128, 300_i128];
    client.create_contract(
        client_addr,
        freelancer_addr,
        arbiter_addr,
        &milestones,
        mode,
        &None,
        &None,
    )
}

/// Fund a contract with the full milestone amount (600 total).
fn fund_contract(_env: &Env, client: &EscrowClient, contract_id: &u32) {
    client.deposit_funds(contract_id, &600_i128);
}

// ---------------------------------------------------------------------------
// Client-only authorization tests
// ---------------------------------------------------------------------------

#[test]
fn client_only_mode_allows_direct_release() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &None,
        &ReleaseAuthorizationMode::ClientOnly,
    );

    fund_contract(&env, &client, &contract_id);

    // Client can release directly without approval
    assert!(client.release_milestone(&contract_id, &0, &client_addr));
}

#[test]
fn client_only_mode_rejects_approval_calls() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &None,
        &ReleaseAuthorizationMode::ClientOnly,
    );

    fund_contract(&env, &client, &contract_id);

    // Approval calls should be rejected for client-only mode
    let result = client.try_approve_milestone_release(&contract_id, &0, &client_addr);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Dual approval tests
// ---------------------------------------------------------------------------

#[test]
fn dual_approval_mode_requires_both_parties() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &None,
        &ReleaseAuthorizationMode::ClientAndFreelancer,
    );

    fund_contract(&env, &client, &contract_id);

    // Client approves
    assert!(client.approve_milestone_release(&contract_id, &0, &client_addr));

    // Release should fail without freelancer approval
    let result = client.try_release_milestone(&contract_id, &0, &client_addr);
    assert!(result.is_err());

    // Freelancer approves
    assert!(client.approve_milestone_release(&contract_id, &0, &freelancer_addr));

    // Now release should succeed
    assert!(client.release_milestone(&contract_id, &0, &client_addr));
}

#[test]
fn dual_approval_mode_prevents_duplicate_approvals() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &None,
        &ReleaseAuthorizationMode::ClientAndFreelancer,
    );

    fund_contract(&env, &client, &contract_id);

    // Client approves
    assert!(client.approve_milestone_release(&contract_id, &0, &client_addr));

    // Duplicate client approval should fail
    let result = client.try_approve_milestone_release(&contract_id, &0, &client_addr);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Arbiter-only tests
// ---------------------------------------------------------------------------

#[test]
fn arbiter_only_mode_requires_arbiter_approval() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let arbiter_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &Some(arbiter_addr.clone()),
        &ReleaseAuthorizationMode::ArbiterOnly,
    );

    fund_contract(&env, &client, &contract_id);

    // Arbiter approves
    assert!(client.approve_milestone_release(&contract_id, &0, &arbiter_addr));

    // Release should succeed
    assert!(client.release_milestone(&contract_id, &0, &arbiter_addr));
}

#[test]
fn arbiter_only_mode_rejects_without_arbiter() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &None, // No arbiter
        &ReleaseAuthorizationMode::ArbiterOnly,
    );

    fund_contract(&env, &client, &contract_id);

    // Approval should fail without arbiter
    let result = client.try_approve_milestone_release(&contract_id, &0, &client_addr);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Event emission tests
// ---------------------------------------------------------------------------

#[test]
fn approval_emits_events() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &None,
        &ReleaseAuthorizationMode::ClientAndFreelancer,
    );

    fund_contract(&env, &client, &contract_id);

    // Client approves
    client.approve_milestone_release(&contract_id, &0, &client_addr);

    // Check approval event was emitted
    let events = env.events().all();
    assert!(events.len() > 0);

    // Find the approval event
    let approval_event = events.iter().find(|event| {
        event.0 == soroban_sdk::symbol_short!("milestone_approved")
    });
    assert!(approval_event.is_some());
}

#[test]
fn release_emits_events() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &None,
        &ReleaseAuthorizationMode::ClientOnly,
    );

    fund_contract(&env, &client, &contract_id);

    // Release milestone
    client.release_milestone(&contract_id, &0, &client_addr);

    // Check release event was emitted
    let events = env.events().all();
    assert!(events.len() > 0);

    // Find the release event
    let release_event = events.iter().find(|event| {
        event.0 == soroban_sdk::symbol_short!("milestone_released")
    });
    assert!(release_event.is_some());
}

#[test]
fn rejects_double_release_and_completes_contract() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &None,
        &ReleaseAuthorizationMode::ClientOnly,
    );
    fund_contract(&env, &client, &contract_id);

    assert!(client.release_milestone(&contract_id, &0, &client_addr));

    let result = client.try_release_milestone(&contract_id, &0, &client_addr);
    assert_contract_error(result, EscrowError::AlreadyReleased);

    assert!(client.release_milestone(&contract_id, &1, &client_addr));
    assert!(client.release_milestone(&contract_id, &2, &client_addr));

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Completed);
    assert_eq!(client.get_pending_reputation_credits(&freelancer_addr), 1);
}

#[test]
fn rejects_refund_after_release_and_release_after_refund() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);

    let contract_id = create_contract_with_mode(
        &env,
        &client,
        &client_addr,
        &freelancer_addr,
        &None,
        &ReleaseAuthorizationMode::ClientOnly,
    );
    fund_contract(&env, &client, &contract_id);

    assert!(client.release_milestone(&contract_id, &0, &client_addr));
    let refund_ids = vec![&env, 0_u32];
    let refund_result = client.try_refund_unreleased_milestones(&contract_id, &refund_ids);
    assert_contract_error(refund_result, EscrowError::AlreadyReleased);

    let refund_ids = vec![&env, 1_u32];
    assert!(client.refund_unreleased_milestones(&contract_id, &refund_ids));

    let result = client.try_release_milestone(&contract_id, &1, &client_addr);
    assert_contract_error(result, EscrowError::Refunded);
}
