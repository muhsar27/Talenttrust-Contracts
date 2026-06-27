#![cfg(test)]
#![allow(dead_code)]

use soroban_sdk::{testutils::Address as _, vec, Address, Env, Vec};

use crate::{Contract, ContractStatus, Escrow, EscrowClient, Error, ReleaseAuthorization};

// --- Submodules ---

mod access_control;
mod accounting_invariants;
mod admin_auth_helper;
mod approval_expiry;
mod authorization_matrix_validation;
mod cancel_contract;
mod client_migration;
mod deposit;
mod dispute;
mod emergency_controls;
mod flows;
// mod governance; // requires unimplemented cancel_governance_admin_proposal
mod governance_events;
mod hello;
mod input_sanitization_amounts;
mod input_sanitization_identities;
mod lifecycle;
mod mainnet_readiness;
// mod milestone_schedule; // requires unimplemented get/set_milestone_schedule
mod pagination_participant_index;
// mod participant_index_pagination; // requires unimplemented list_contracts_by_participant
mod pause_controls;
// mod performance; // references .cancel()/.refund()/.dispute() short-name methods
mod persistence;
mod protocol_fees;
mod refund;
mod release;
mod release_authorization;
mod reputation;
mod resolution_payouts_prop;
mod sac_custody;
mod security;
mod storage;
mod summary;
// mod timeout_tests; // requires unimplemented evaluate_milestone_timeout
mod treasury_rotation_timelock;
mod ttl_tests;

// --- Shared constants ---

pub const MILESTONE_ONE: i128 = 200_0000000;
pub const MILESTONE_TWO: i128 = 400_0000000;
pub const MILESTONE_THREE: i128 = 600_0000000;

// --- Shared helpers ---

pub fn register_client(env: &Env) -> EscrowClient<'_> {
    let id = env.register(Escrow, ());
    let client = EscrowClient::new(env, &id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    client
}

/// Registers the escrow contract without initializing it.
pub fn register_escrow(env: &Env) -> EscrowClient<'_> {
    let id = env.register(Escrow, ());
    EscrowClient::new(env, &id)
}

/// Returns a default test environment with all auths mocked.
pub fn setup_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

pub fn default_milestones(env: &Env) -> soroban_sdk::Vec<i128> {
    vec![env, MILESTONE_ONE, MILESTONE_TWO, MILESTONE_THREE]
}

pub fn total_milestone_amount() -> i128 {
    MILESTONE_ONE + MILESTONE_TWO + MILESTONE_THREE
}

#[allow(dead_code)]
pub fn total_milestones() -> i128 {
    total_milestone_amount()
}

#[allow(dead_code)]
pub fn generated_participants(env: &Env) -> (Address, Address) {
    (Address::generate(env), Address::generate(env))
}

/// Create a contract and return (client_addr, freelancer_addr, contract_id).
pub fn create_contract(env: &Env, client: &EscrowClient) -> (Address, Address, u32) {
    let client_addr = Address::generate(env);
    let freelancer_addr = Address::generate(env);
    let milestones = default_milestones(env);
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
    );
    (client_addr, freelancer_addr, id)
}

/// Create a contract with an arbiter and return (client_addr, freelancer_addr, arbiter, contract_id).
pub fn create_contract_with_arbiter(
    env: &Env,
    client: &EscrowClient,
) -> (Address, Address, Address, u32) {
    let client_addr = Address::generate(env);
    let freelancer_addr = Address::generate(env);
    let arbiter_addr = Address::generate(env);
    let milestones = default_milestones(env);
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &Some(arbiter_addr.clone()),
        &milestones,
        &ReleaseAuthorization::ClientAndArbiter,
    );
    (client_addr, freelancer_addr, arbiter_addr, id)
}

/// Create and fully complete a contract (all milestones released).
/// Caller is the client address for deposit and release operations.
pub fn complete_contract(env: &Env, client: &EscrowClient) -> (Address, Address, u32) {
    let (client_addr, freelancer_addr, id) = create_contract(env, client);
    assert!(client.deposit_funds(&id, &client_addr, &total_milestone_amount()));
    assert!(client.approve_milestone_release(&id, &client_addr, &0));
    assert!(client.release_milestone(&id, &client_addr, &0));
    assert!(client.approve_milestone_release(&id, &client_addr, &1));
    assert!(client.release_milestone(&id, &client_addr, &1));
    assert!(client.approve_milestone_release(&id, &client_addr, &2));
    assert!(client.release_milestone(&id, &client_addr, &2));
    (client_addr, freelancer_addr, id)
}

/// Assert that a `try_*` call returns the expected contract error.
///
/// Soroban `try_*` methods return:
///   `Result<Result<T, IE>, Result<soroban_sdk::Error, InvokeError>>`
/// A contract-level `panic_with_error` surfaces as `Err(Ok(soroban_sdk::Error))`.
/// The `expected` argument can be any type convertible to `soroban_sdk::Error`,
/// including both `Error` and the canonical `Error` from `types.rs`.
pub fn assert_contract_error<
    T: core::fmt::Debug,
    IE: core::fmt::Debug,
    E: Into<soroban_sdk::Error> + core::fmt::Debug,
>(
    result: Result<Result<T, IE>, Result<soroban_sdk::Error, soroban_sdk::InvokeError>>,
    expected: E,
) {
    match result {
        Err(Ok(e)) => {
            let expected_err: soroban_sdk::Error = expected.into();
            assert_eq!(e, expected_err, "contract error code mismatch");
        }
        _other => panic!(
            "expected contract error {:?}, got unexpected result variant: {:?}",
            expected, _other
        ),
    }
}

pub fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    (env, client_addr, freelancer_addr)
}

pub fn create_client(env: &Env) -> EscrowClient<'_> {
    register_client(env)
}

pub fn create_default_contract(
    env: &Env,
    client: &EscrowClient<'_>,
    client_addr: &Address,
    freelancer_addr: &Address,
) -> u32 {
    // 1. Initialize contract if not already initialized
    if !env.storage().persistent().has(&crate::DataKey::Initialized) {
        let admin = Address::generate(env);
        client.initialize(&admin);
    }
    
    // 2. Set settlement token if not already set
    if !env.storage().persistent().has(&crate::DataKey::SettlementToken) {
        let token_admin = Address::generate(env);
        let token_address = env.register_stellar_asset_contract(token_admin);
        client.set_settlement_token(&token_address);
    }

    // 3. Mint tokens to client_addr
    let token_address = client.get_settlement_token();
    let token_client = soroban_sdk::token::StellarAssetClient::new(env, &token_address);
    token_client.mint(client_addr, &100_000_0000000_i128); // mint a large balance

    let milestones = vec![env, 200_0000000_i128, 400_0000000_i128, 600_0000000_i128];
    client.create_contract(
        client_addr,
        freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
    )
}

pub fn complete_contract(env: &Env, client: &EscrowClient) -> (Address, Address, u32) {
    let client_addr = Address::generate(env);
    let freelancer_addr = Address::generate(env);
    let milestones = vec![env, 200_0000000_i128, 400_0000000_i128, 600_0000000_i128];
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
    );
    assert!(client.deposit_funds(&id, &client_addr, &1_200_0000000_i128));
    assert!(client.approve_milestone_release(&id, &client_addr, &0));
    assert!(client.release_milestone(&id, &0, &client_addr));
    assert!(client.approve_milestone_release(&id, &client_addr, &1));
    assert!(client.release_milestone(&id, &1, &client_addr));
    assert!(client.approve_milestone_release(&id, &client_addr, &2));
    assert!(client.release_milestone(&id, &2, &client_addr));
    (client_addr, freelancer_addr, id)
}

pub fn assert_contract_state(
    contract: Contract,
    expected_status: ContractStatus,
    expected_funded: i128,
    expected_released: i128,
    expected_refunded: i128,
) {
    assert_eq!(contract.status, expected_status);
    assert_eq!(contract.funded_amount, expected_funded);
    assert_eq!(contract.released_amount, expected_released);
    assert_eq!(contract.refunded_amount, expected_refunded);
}
