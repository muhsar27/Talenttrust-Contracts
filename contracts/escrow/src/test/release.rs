use soroban_sdk::{symbol_short, testutils::Address as _, testutils::Events, vec, Address, Env, String, Symbol, TryFromVal, Val};

use super::{assert_contract_error, create_contract, register_client, total_milestone_amount, MILESTONE_ONE};
use crate::{ContractStatus, Error, ReleaseAuthorization};

fn evidence(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

// ---------------------------------------------------------------------------
// Release flow tests
// ---------------------------------------------------------------------------

#[test]
fn releases_funded_milestones_and_completes_when_all_released() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));

    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Funded);
    assert_eq!(contract.released_amount, MILESTONE_ONE);

    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    assert!(client.release_milestone(&contract_id, &client_addr, &1));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &2));
    assert!(client.release_milestone(&contract_id, &client_addr, &2));

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Completed);
    assert_eq!(contract.released_amount, total_milestone_amount());
    assert_eq!(client.get_refundable_balance(&contract_id), 0);
}

#[test]
fn rejects_release_without_sufficient_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &100_i128));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    let result = client.try_release_milestone(&contract_id, &client_addr, &0);
    assert_contract_error(result, Error::InsufficientFunds);
}

#[test]
fn rejects_release_of_invalid_milestone_index() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    let result = client.try_release_milestone(&contract_id, &client_addr, &99);
    assert_contract_error(result, Error::InvalidMilestone);
}

#[test]
fn rejects_releasing_refunded_milestone() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    client.refund_unreleased_milestones(&contract_id, &vec![&env, 1_u32]);

    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    let result = client.try_release_milestone(&contract_id, &client_addr, &1);
    assert_contract_error(result, Error::AlreadyRefunded);
}

#[test]
fn rejects_releasing_same_milestone_twice() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));

    let result = client.try_release_milestone(&contract_id, &client_addr, &0);
    assert_contract_error(result, Error::AlreadyReleased);
}

// ---------------------------------------------------------------------------
// submit_work_evidence tests
// ---------------------------------------------------------------------------

#[test]
fn work_evidence_stored_on_milestone() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    let ev = evidence(&env, "ipfs://QmExampleCid");
    assert!(escrow.submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev));

    let milestones = escrow.get_milestones(&contract_id);
    assert_eq!(milestones.get(0).unwrap().work_evidence, Some(ev));
}

#[test]
fn work_evidence_can_be_overwritten_before_release() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    let v1 = evidence(&env, "ipfs://first");
    let v2 = evidence(&env, "ipfs://second");
    assert!(escrow.submit_work_evidence(&contract_id, &freelancer_addr, &0, &v1));
    assert!(escrow.submit_work_evidence(&contract_id, &freelancer_addr, &0, &v2));

    let milestones = escrow.get_milestones(&contract_id);
    assert_eq!(milestones.get(0).unwrap().work_evidence, Some(v2));
}

#[test]
fn work_evidence_does_not_affect_other_milestones() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    let ev = evidence(&env, "ipfs://QmOnlyMilestone0");
    assert!(escrow.submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev));

    let milestones = escrow.get_milestones(&contract_id);
    assert!(milestones.get(1).unwrap().work_evidence.is_none());
    assert!(milestones.get(2).unwrap().work_evidence.is_none());
}

#[test]
fn work_evidence_emits_evidence_event() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    let ev = evidence(&env, "ipfs://QmTest");
    assert!(escrow.submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev));

    let events = env.events().all();
    use soroban_sdk::{Symbol, TryFromVal};
    assert!(events.iter().any(|e| {
        e.1.get(0)
            .and_then(|v| Symbol::try_from_val(&env, &v).ok())
            .as_ref()
            == Some(&symbol_short!("evidence"))
    }));
}

#[test]
fn work_evidence_rejects_non_freelancer_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    let ev = evidence(&env, "ipfs://QmTest");
    // client is not the freelancer
    let result = escrow.try_submit_work_evidence(&contract_id, &client_addr, &0, &ev);
    assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn work_evidence_rejects_stranger() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    let stranger = Address::generate(&env);
    let ev = evidence(&env, "ipfs://QmTest");
    let result = escrow.try_submit_work_evidence(&contract_id, &stranger, &0, &ev);
    assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn work_evidence_rejects_unfunded_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (_client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    // no deposit — contract stays in Created status

    let ev = evidence(&env, "ipfs://QmTest");
    let result = escrow.try_submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev);
    assert_contract_error(result, Error::InvalidState);
}

#[test]
fn work_evidence_rejects_released_milestone() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());
    escrow.approve_milestone_release(&contract_id, &client_addr, &0);
    escrow.release_milestone(&contract_id, &client_addr, &0);

    let ev = evidence(&env, "ipfs://QmTest");
    let result = escrow.try_submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev);
    assert_contract_error(result, Error::MilestoneAlreadyReleased);
}

#[test]
fn work_evidence_rejects_refunded_milestone() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());
    escrow.refund_unreleased_milestones(&contract_id, &vec![&env, 0_u32]);

    let ev = evidence(&env, "ipfs://QmTest");
    let result = escrow.try_submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev);
    assert_contract_error(result, Error::AlreadyRefunded);
}

#[test]
fn work_evidence_rejects_out_of_bounds_index() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    let ev = evidence(&env, "ipfs://QmTest");
    let result = escrow.try_submit_work_evidence(&contract_id, &freelancer_addr, &99, &ev);
    assert_contract_error(result, Error::IndexOutOfBounds);
}

#[test]
fn work_evidence_rejects_oversized_string() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    // 257 chars > 256-byte limit
    let ev = String::from_str(&env, &"a".repeat(257));
    let result = escrow.try_submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev);
    assert_contract_error(result, Error::EvidenceTooLong);
}

#[test]
fn work_evidence_accepts_exactly_256_byte_string() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    let ev = String::from_str(&env, &"a".repeat(256));
    assert!(escrow.submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev));
}

#[test]
fn work_evidence_rejects_paused_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);

    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());
    escrow.pause();

    let ev = evidence(&env, "ipfs://QmTest");
    let result = escrow.try_submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev);
    assert_contract_error(result, Error::ContractPaused);
}

#[test]
fn work_evidence_rejects_finalized_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &escrow);
    escrow.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    // Release all milestones → Completed → finalize
    escrow.approve_milestone_release(&contract_id, &client_addr, &0);
    escrow.release_milestone(&contract_id, &client_addr, &0);
    escrow.approve_milestone_release(&contract_id, &client_addr, &1);
    escrow.release_milestone(&contract_id, &client_addr, &1);
    escrow.approve_milestone_release(&contract_id, &client_addr, &2);
    escrow.release_milestone(&contract_id, &client_addr, &2);
    escrow.finalize_contract(&contract_id, &client_addr);

    let ev = evidence(&env, "ipfs://QmAfterFinalize");
    let result = escrow.try_submit_work_evidence(&contract_id, &freelancer_addr, &0, &ev);
    assert_contract_error(result, Error::AlreadyFinalized);
}

#[test]
fn work_evidence_rejects_unknown_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let escrow = register_client(&env);
    let freelancer = Address::generate(&env);

    let ev = evidence(&env, "ipfs://QmTest");
    let result = escrow.try_submit_work_evidence(&9999, &freelancer, &0, &ev);
    assert_contract_error(result, Error::ContractNotFound);
}

// ---------------------------------------------------------------------------
// Per-milestone accounting on release
// ---------------------------------------------------------------------------

#[test]
fn release_sets_milestone_funded_amount_to_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    // Release milestone 0: funded_amount should equal amount
    client.approve_milestone_release(&contract_id, &client_addr, &0);
    client.release_milestone(&contract_id, &client_addr, &0);
    let ms = client.get_milestones(&contract_id);
    assert_eq!(ms.get(0).unwrap().funded_amount, MILESTONE_ONE);
    assert_eq!(ms.get(0).unwrap().released, true);

    // Release milestone 1
    client.approve_milestone_release(&contract_id, &client_addr, &1);
    client.release_milestone(&contract_id, &client_addr, &1);
    let ms = client.get_milestones(&contract_id);
    assert_eq!(ms.get(1).unwrap().funded_amount, 400_0000000_i128);
    assert_eq!(ms.get(1).unwrap().released, true);
}

#[test]
fn refund_sets_milestone_refunded_amount_on_unreleased() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    // Refund milestone 1 only
    client.refund_unreleased_milestones(&contract_id, &vec![&env, 1_u32]);
    let ms = client.get_milestones(&contract_id);
    assert_eq!(ms.get(0).unwrap().refunded_amount, 0);
    assert_eq!(ms.get(0).unwrap().refunded, false);
    assert_eq!(ms.get(1).unwrap().refunded_amount, 400_0000000_i128);
    assert_eq!(ms.get(1).unwrap().refunded, true);
    assert_eq!(ms.get(2).unwrap().refunded_amount, 0);
    assert_eq!(ms.get(2).unwrap().refunded, false);
}

#[test]
fn mixed_release_refund_maintains_per_milestone_invariant() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount());

    // Release milestone 0
    client.approve_milestone_release(&contract_id, &client_addr, &0);
    client.release_milestone(&contract_id, &client_addr, &0);

    // Refund milestone 2 (unreleased)
    client.refund_unreleased_milestones(&contract_id, &vec![&env, 2_u32]);

    let ms = client.get_milestones(&contract_id);
    assert_eq!(ms.get(0).unwrap().funded_amount, MILESTONE_ONE);
    assert_eq!(ms.get(0).unwrap().released, true);
    assert_eq!(ms.get(1).unwrap().funded_amount, 400_0000000_i128);
    assert_eq!(ms.get(1).unwrap().refunded_amount, 0);
    assert_eq!(ms.get(1).unwrap().released, false);
    assert_eq!(ms.get(2).unwrap().refunded_amount, 600_0000000_i128);
    assert_eq!(ms.get(2).unwrap().refunded, true);

    // Invariant: per-milestone sums match contract totals
    let contract = client.get_contract(&contract_id);
    let ms = client.get_milestones(&contract_id);
    let funded_sum: i128 = ms.iter().map(|m| m.funded_amount).sum();
    let refunded_sum: i128 = ms.iter().map(|m| m.refunded_amount).sum();
    assert_eq!(funded_sum, contract.funded_amount);
    assert_eq!(refunded_sum, contract.refunded_amount);
}

#[test]
fn release_moves_funds_on_chain() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin);
    client.set_settlement_token(&token_address);
    
    let milestones = soroban_sdk::vec![&env, 100_i128, 200_i128];
    let contract_id = client.create_contract(&client_addr, &freelancer_addr, &None, &milestones, &crate::types::ReleaseAuthorization::ClientOnly);
    
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);
    token_client.mint(&client_addr, &300_i128);
    
    // Deposit total contract amount (300)
    assert!(client.deposit_funds(&contract_id, &client_addr, &300_i128));
    
    // Release milestone 0 (100)
    let token_query = soroban_sdk::token::Client::new(&env, &token_address);
    assert_eq!(token_query.balance(&freelancer_addr), 0_i128);
    assert_eq!(token_query.balance(&env.current_contract_address()), 300_i128);
    
    assert!(client.release_milestone(&contract_id, &0, &client_addr));
    
    // Verify freelancer received funds, contract balance decreased
    assert_eq!(token_query.balance(&freelancer_addr), 100_i128);
    assert_eq!(token_query.balance(&env.current_contract_address()), 200_i128);
}

#[test]
fn refund_moves_funds_on_chain() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin);
    client.set_settlement_token(&token_address);
    
    let milestones = soroban_sdk::vec![&env, 100_i128, 200_i128];
    let contract_id = client.create_contract(&client_addr, &freelancer_addr, &None, &milestones, &crate::types::ReleaseAuthorization::ClientOnly);
    
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);
    token_client.mint(&client_addr, &300_i128);
    
    assert!(client.deposit_funds(&contract_id, &client_addr, &300_i128));
    
    // Refund milestone 1 (200)
    let token_query = soroban_sdk::token::Client::new(&env, &token_address);
    let refund_ids = soroban_sdk::vec![&env, 1_u32];
    
    let refunded = client.refund_unreleased_milestones(&contract_id, &refund_ids);
    assert_eq!(refunded, 200_i128);
    
    // Verify client received refund back, contract balance decreased
    assert_eq!(token_query.balance(&client_addr), 200_i128);
    assert_eq!(token_query.balance(&env.current_contract_address()), 100_i128);
}

#[test]
fn dispute_resolution_moves_funds_on_chain() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin);
    client.set_settlement_token(&token_address);
    
    let arbiter_addr = Address::generate(&env);
    let milestones = soroban_sdk::vec![&env, 300_i128];
    let contract_id = client.create_contract_with_arbiter(&client_addr, &freelancer_addr, &arbiter_addr, &milestones, &crate::types::DepositMode::ExactTotal);
    
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);
    token_client.mint(&client_addr, &300_i128);
    
    assert!(client.deposit_funds(&contract_id, &client_addr, &300_i128));
    
    // Dispute and resolve split
    assert!(client.raise_dispute(&contract_id, &client_addr));
    
    let token_query = soroban_sdk::token::Client::new(&env, &token_address);
    let split = crate::types::SplitAmounts {
        client_amount: 100_i128,
        freelancer_amount: 200_i128,
    };
    assert!(client.resolve_dispute(&contract_id, &arbiter_addr, &crate::types::DisputeResolution::Split(split)));
    
    // Verify split payouts
    assert_eq!(token_query.balance(&client_addr), 100_i128);
    assert_eq!(token_query.balance(&freelancer_addr), 200_i128);
    assert_eq!(token_query.balance(&env.current_contract_address()), 0_i128);
}

#[test]
#[should_panic]
fn release_fails_underfunded() {
    let (env, client_addr, freelancer_addr) = setup();
    let client = create_client(&env);
    
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin);
    client.set_settlement_token(&token_address);
    
    let milestones = soroban_sdk::vec![&env, 100_i128, 200_i128];
    let contract_id = client.create_contract(&client_addr, &freelancer_addr, &None, &milestones, &crate::types::ReleaseAuthorization::ClientOnly);
    
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);
    token_client.mint(&client_addr, &300_i128);
    client.deposit_funds(&contract_id, &client_addr, &300_i128);
    
    // Set settlement token to unregistered_token (which holds 0 balance)
    let unregistered_token = env.register_stellar_asset_contract(Address::generate(&env));
    client.set_settlement_token(&unregistered_token);
    
    // Now trying to release should panic because the contract balance on unregistered_token is 0!
    client.release_milestone(&contract_id, &0, &client_addr);
}
