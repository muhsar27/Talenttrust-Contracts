use super::{create_contract, register_client, total_milestone_amount};
use crate::{ContractStatus, EscrowError, ReleaseAuthorization};
use soroban_sdk::{testutils::Address as _, vec, Address, Env};

/// Finalization succeeds from Completed status; record snapshot matches contract state.
#[test]
fn finalize_completed_contract_persists_immutable_close_record() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = super::complete_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");
    assert_eq!(record.finalizer, client_addr);
    assert_eq!(record.summary.client, client_addr);
    assert_eq!(record.summary.freelancer, freelancer_addr);
    assert_eq!(record.summary.status, ContractStatus::Completed);
    assert_eq!(record.summary.total_amount, super::total_milestone_amount());
    assert_eq!(
        record.summary.funded_amount,
        super::total_milestone_amount()
    );
    assert_eq!(
        record.summary.released_amount,
        super::total_milestone_amount()
    );
    assert_eq!(record.summary.refundable_balance, 0);
    assert_eq!(record.summary.released_milestone_count, 3);
}

/// Finalization succeeds from Disputed status; arbiter can finalize.
#[test]
fn finalize_disputed_contract_allows_arbiter_finalizer() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, arbiter_addr, contract_id) =
        super::create_contract_with_arbiter(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &super::total_milestone_amount()));
    assert!(client.raise_dispute(&contract_id, &client_addr));
    assert_eq!(
        client.get_contract(&contract_id).status,
        ContractStatus::Disputed
    );

    assert!(client.finalize_contract(&contract_id, &arbiter_addr));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");
    assert_eq!(record.finalizer, arbiter_addr);
    assert_eq!(record.summary.status, ContractStatus::Disputed);
    assert_eq!(
        record.summary.funded_amount,
        super::total_milestone_amount()
    );
    assert_eq!(record.summary.released_amount, 0);
    assert_eq!(
        record.summary.refundable_balance,
        super::total_milestone_amount()
    );
}

#[test]
fn participant_metadata_and_pending_credits_persist_until_reputation_is_issued() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);

    let (client_addr, freelancer_addr, contract_id) = super::complete_contract(&env, &client);
    let completed = client.get_contract(&contract_id);
    assert_eq!(completed.client, client_addr);
    assert_eq!(completed.freelancer, freelancer_addr);
    assert_eq!(completed.status, ContractStatus::Completed);
    assert_eq!(client.get_pending_reputation_credits(&freelancer_addr), 1);

    assert!(client.issue_reputation(&contract_id, &client_addr, &freelancer_addr, &5));
    assert_eq!(client.get_pending_reputation_credits(&freelancer_addr), 0);
}

#[test]
fn try_get_contract_reports_missing_state_without_mutating_storage() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);

    super::assert_contract_error(client.try_get_contract(&777), EscrowError::ContractNotFound);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let milestones = vec![&env, 10_i128];
    let created = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
    );
}

/// Freelancer may also finalize a Completed contract.
#[test]
fn finalize_allows_freelancer_finalizer() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = super::complete_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &freelancer_addr));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");
    assert_eq!(record.finalizer, freelancer_addr);
}

/// Non-participant (outsider) cannot finalize.
#[test]
fn finalize_rejects_unauthorized_finalizer() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (_client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);
    let outsider = Address::generate(&env);

    super::assert_contract_error(
        client.try_finalize_contract(&contract_id, &outsider),
        EscrowError::UnauthorizedRole,
    );
    assert!(client.get_finalization_record(&contract_id).is_none());
}

/// Finalization from non-terminal status (Created) is rejected.
#[test]
fn finalize_rejects_created_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    super::assert_contract_error(
        client.try_finalize_contract(&contract_id, &client_addr),
        EscrowError::InvalidStatusTransition,
    );
    assert!(client.get_finalization_record(&contract_id).is_none());
}

/// Finalization from Funded status is rejected.
#[test]
fn finalize_rejects_funded_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);
    assert!(client.deposit_funds(&contract_id, &client_addr, &super::total_milestone_amount()));
    assert_eq!(
        client.get_contract(&contract_id).status,
        ContractStatus::Funded
    );

    super::assert_contract_error(
        client.try_finalize_contract(&contract_id, &client_addr),
        EscrowError::InvalidStatusTransition,
    );
    assert!(client.get_finalization_record(&contract_id).is_none());
}

/// Double finalization is rejected with AlreadyFinalized.
#[test]
fn finalize_is_idempotent_guarded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));
    super::assert_contract_error(
        client.try_finalize_contract(&contract_id, &client_addr),
        EscrowError::AlreadyFinalized,
    );
}

/// release_milestone is rejected after finalization.
#[test]
fn release_milestone_rejects_after_finalization() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));

    super::assert_contract_error(
        client.try_release_milestone(&contract_id, &client_addr, &0),
        EscrowError::AlreadyFinalized,
    );
}

/// refund_unreleased_milestones is rejected after finalization.
#[test]
fn refund_unreleased_milestones_rejects_after_finalization() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));

    super::assert_contract_error(
        client.try_refund_unreleased_milestones(&contract_id, &vec![&env, 0u32]),
        EscrowError::AlreadyFinalized,
    );
}

/// deposit_funds is rejected after finalization.
#[test]
fn deposit_funds_rejects_after_finalization() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));

    super::assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &1_i128),
        EscrowError::AlreadyFinalized,
    );
}

/// approve_milestone_release is rejected after finalization.
#[test]
fn approve_milestone_release_rejects_after_finalization() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));

    super::assert_contract_error(
        client.try_approve_milestone_release(&contract_id, &client_addr, &0),
        EscrowError::AlreadyFinalized,
    );
}

/// get_finalization_record returns None for an unfinalized contract.
#[test]
fn get_finalization_record_returns_none_for_unfinalized() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (_client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);

    assert!(client.get_finalization_record(&contract_id).is_none());
}

/// Finalization record is absent for a non-existent contract.
#[test]
fn get_finalization_record_returns_none_for_missing_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);

    assert!(client.get_finalization_record(&999).is_none());
}

/// Pause blocks finalization.
#[test]
fn pause_blocks_finalization() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let admin = Address::generate(&env);
    assert!(client.initialize(&admin));
    let (client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);
    assert!(client.pause());

    super::assert_contract_error(
        client.try_finalize_contract(&contract_id, &client_addr),
        EscrowError::ContractPaused,
    );
    assert!(client.get_finalization_record(&contract_id).is_none());
}

/// Test finalization on a contract refunded to Completion (mixed release/refund).
#[test]
fn finalize_completed_with_mixed_releases_and_refunds() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &super::total_milestone_amount()));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    assert!(client.release_milestone(&contract_id, &client_addr, &1));

    assert!(client.refund_unreleased_milestones(&contract_id, &vec![&env, 2u32]));
    assert_eq!(
        client.get_contract(&contract_id).status,
        ContractStatus::Completed
    );

    assert!(client.finalize_contract(&contract_id, &client_addr));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");
    assert_eq!(record.summary.status, ContractStatus::Completed);
    assert_eq!(
        record.summary.released_amount,
        super::MILESTONE_ONE + super::MILESTONE_TWO
    );
    assert_eq!(record.summary.refundable_balance, 0);
    assert_eq!(record.summary.released_milestone_count, 2);
}
