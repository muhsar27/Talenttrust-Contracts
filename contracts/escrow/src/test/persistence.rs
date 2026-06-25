use super::{
    create_contract, register_client, total_milestone_amount, MILESTONE_ONE, MILESTONE_THREE,
    MILESTONE_TWO,
};
use crate::{ContractStatus, EscrowError, ReleaseAuthorization, CONTRACT_SUMMARY_SCHEMA_VERSION};
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
        MILESTONE_ONE + MILESTONE_TWO
    );
    assert_eq!(record.summary.refundable_balance, 0);
    assert_eq!(record.summary.released_milestone_count, 2);
}

/// Verify that every finalized record carries the current
/// `CONTRACT_SUMMARY_SCHEMA_VERSION`.
#[test]
fn finalization_schema_version_matches_constant() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    assert!(client.release_milestone(&contract_id, &client_addr, &1));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &2));
    assert!(client.release_milestone(&contract_id, &client_addr, &2));

    assert!(client.finalize_contract(&contract_id, &client_addr));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");

    assert_eq!(
        record.summary.schema_version, CONTRACT_SUMMARY_SCHEMA_VERSION,
        "schema_version must match CONTRACT_SUMMARY_SCHEMA_VERSION"
    );
}

/// Verify that `refundable_balance` exactly matches the documented formula
/// `(funded_amount - released_amount) - refunded_amount` using the source
/// [`Contract`] accounting fields that existed before finalisation.
///
/// The summary does NOT carry a `refunded_amount` field, so we read it from
/// the on-chain [`Contract`] record.
#[test]
fn refundable_balance_matches_documented_derivation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));

    // Release milestones 0 and 1; refund milestone 2.
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    assert!(client.release_milestone(&contract_id, &client_addr, &1));
    assert!(client.refund_unreleased_milestones(&contract_id, &vec![&env, 2u32]));

    // Snapshot Contract accounting fields before finalization.
    let contract_before = client.get_contract(&contract_id);
    let funded = contract_before.funded_amount;
    let released = contract_before.released_amount;
    let refunded = contract_before.refunded_amount;
    let expected_balance = (funded - released) - refunded;

    assert!(client.finalize_contract(&contract_id, &client_addr));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");

    assert_eq!(
        record.summary.refundable_balance, expected_balance,
        "refundable_balance must equal (funded_amount - released_amount) - refunded_amount \
         from the source Contract record"
    );
    assert_eq!(
        record.summary.refundable_balance, 0,
        "with all milestones released or refunded, refundable_balance must be 0"
    );
}

/// Verify that `released_milestone_count` matches the documented derivation:
/// the number of milestones where `released == true`.
#[test]
fn released_milestone_count_matches_documented_derivation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));

    // Release only milestone 1; leave 0 and 2 unreleased.
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &1));
    assert!(client.release_milestone(&contract_id, &client_addr, &1));

    // Refund the remaining unreleased milestones to reach Completed.
    assert!(client.refund_unreleased_milestones(&contract_id, &vec![&env, 0u32, 2u32]));
    assert_eq!(
        client.get_contract(&contract_id).status,
        ContractStatus::Completed
    );

    assert!(client.finalize_contract(&contract_id, &client_addr));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");

    let summary = &record.summary;

    // released_milestone_count must equal the number of milestone summaries
    // with released == true.
    let actual_released_count = summary.milestones.iter().filter(|ms| ms.released).count() as u32;

    assert_eq!(
        summary.released_milestone_count, actual_released_count,
        "released_milestone_count must match actual count of released milestones"
    );
    assert_eq!(summary.released_milestone_count, 1);
    assert_eq!(summary.released_amount, MILESTONE_TWO);
}
