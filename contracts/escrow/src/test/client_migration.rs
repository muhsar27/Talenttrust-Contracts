#![cfg(test)]

use crate::{
    types::DepositMode, Escrow, EscrowClient, EscrowError, PendingClientMigration,
    PENDING_MIGRATION_TTL_LEDGERS,
};
use soroban_sdk::{
    testutils::Address as _, testutils::Ledger as _, testutils::LedgerInfo, vec, Address, Env,
};

use super::{assert_contract_error, default_milestones, register_client};

#[test]
fn propose_and_accept_client_migration() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let new_client = Address::generate(&env);

    let milestones = default_milestones(&env);
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &milestones,
        &DepositMode::ExactTotal,
    );

    assert!(client.propose_client_migration(&id, &client_addr, &new_client));
    assert!(client.has_pending_client_migration(&id));

    let pending: PendingClientMigration = client.get_pending_client_migration(&id);
    assert_eq!(pending.current_client, client_addr);
    assert_eq!(pending.proposed_client, new_client);
    assert!(pending.expires_at_ledger > env.ledger().sequence());

    assert!(client.accept_client_migration(&id, &new_client));

    let contract = client.get_contract(&id);
    assert_eq!(contract.client, new_client);
    assert!(!client.has_pending_client_migration(&id));
}

#[test]
fn unauthorized_accept_client_migration_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let new_client = Address::generate(&env);
    let attacker = Address::generate(&env);

    let milestones = default_milestones(&env);
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &milestones,
        &DepositMode::ExactTotal,
    );

    assert!(client.propose_client_migration(&id, &client_addr, &new_client));
    let result = client.try_accept_client_migration(&id, &attacker);
    assert_contract_error(result, EscrowError::UnauthorizedRole);
}

#[test]
fn propose_client_migration_rejects_freelancer_address() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let milestones = default_milestones(&env);
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &milestones,
        &DepositMode::ExactTotal,
    );

    let result = client.try_propose_client_migration(&id, &client_addr, &freelancer_addr);
    assert_contract_error(result, EscrowError::InvalidParticipant);
}

#[test]
fn accept_client_migration_rejects_expired_pending_migration() {
    let env = Env::default();
    env.mock_all_auths();
    let current_ledger = env.ledger().get();
    env.ledger().set(LedgerInfo {
        sequence_number: current_ledger.sequence_number,
        timestamp: current_ledger.timestamp,
        protocol_version: current_ledger.protocol_version,
        network_id: current_ledger.network_id.clone(),
        base_reserve: current_ledger.base_reserve,
        min_temp_entry_ttl: 1,
        min_persistent_entry_ttl: PENDING_MIGRATION_TTL_LEDGERS * 4,
        max_entry_ttl: PENDING_MIGRATION_TTL_LEDGERS * 4,
    });
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let new_client = Address::generate(&env);

    let milestones = default_milestones(&env);
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &milestones,
        &DepositMode::ExactTotal,
    );

    assert!(client.propose_client_migration(&id, &client_addr, &new_client));
    env.ledger().set(LedgerInfo {
        sequence_number: env.ledger().get().sequence_number + PENDING_MIGRATION_TTL_LEDGERS + 1,
        timestamp: env.ledger().get().timestamp + 10_000,
        protocol_version: env.ledger().get().protocol_version,
        network_id: [0; 32].into(),
        base_reserve: 100,
        min_temp_entry_ttl: 1,
        min_persistent_entry_ttl: 1,
        max_entry_ttl: 65536,
    });

    let result = client.try_accept_client_migration(&id, &new_client);
    assert_contract_error(result, EscrowError::InvalidState);
    assert!(!client.has_pending_client_migration(&id));
}

#[test]
fn propose_client_migration_rejects_after_contract_completed() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    let client = EscrowClient::new(&env, &contract_id);

    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let new_client = Address::generate(&env);

    let milestones = default_milestones(&env);
    let id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &milestones,
        &DepositMode::ExactTotal,
    );

    assert!(client.deposit_funds(&id, &super::total_milestone_amount()));
    assert!(client.release_milestone(&id, &0));
    assert!(client.release_milestone(&id, &1));
    assert!(client.release_milestone(&id, &2));

    let result = client.try_propose_client_migration(&id, &client_addr, &new_client);
    assert_contract_error(result, EscrowError::InvalidStatusTransition);
}
