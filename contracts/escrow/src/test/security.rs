use super::{create_contract, default_milestones, generated_participants, register_client, total_milestone_amount};
use crate::{Error, ReleaseAuthorization};
use soroban_sdk::{testutils::Address as _, vec, Env, Vec};

#[test]
fn create_rejects_same_participants() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (addr, _) = generated_participants(&env);

    let result =
        client.try_create_contract(&addr, &addr, &None, &default_milestones(&env), &ReleaseAuthorization::ClientOnly);
    super::assert_contract_error(result, Error::InvalidParticipant);
}

#[test]
fn create_rejects_empty_milestone_list() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr) = generated_participants(&env);
    let empty = Vec::<i128>::new(&env);

    let result =
        client.try_create_contract(&client_addr, &freelancer_addr, &None, &empty, &ReleaseAuthorization::ClientOnly);
    super::assert_contract_error(result, Error::EmptyMilestones);
}

#[test]
fn create_rejects_non_positive_milestone_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr) = generated_participants(&env);
    let milestones = vec![&env, 100_i128, 0_i128];

    let result = client.try_create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &ReleaseAuthorization::ClientOnly,
    );
    super::assert_contract_error(result, Error::InvalidMilestoneAmount);
}

#[test]
#[should_panic]
fn create_requires_client_authorization() {
    let env = Env::default();
    let client = register_client(&env);
    let (client_addr, freelancer_addr) = generated_participants(&env);

    let _ = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &default_milestones(&env),
        &ReleaseAuthorization::ClientOnly,
    );
}

#[test]
fn deposit_rejects_non_positive_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    let result = client.try_deposit_funds(&contract_id, &client_addr, &0);
    super::assert_contract_error(result, Error::InvalidDepositAmount);
}

#[test]
fn release_rejects_when_contract_not_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    let result = client.try_release_milestone(&contract_id, &client_addr, &0);
    super::assert_contract_error(result, Error::InsufficientFunds);
}

#[test]
fn release_rejects_invalid_milestone_id() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &super::total_milestone_amount()));
    let result = client.try_release_milestone(&contract_id, &client_addr, &99);
    super::assert_contract_error(result, Error::InvalidMilestone);
}

#[test]
fn release_rejects_double_release() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &super::total_milestone_amount()));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));

    let result = client.try_release_milestone(&contract_id, &client_addr, &0);
    super::assert_contract_error(result, Error::AlreadyReleased);
}

#[test]
fn issue_reputation_rejects_unfinished_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = create_contract(&env, &client);
    let comment = soroban_sdk::String::from_str(&env, "Good job");

    let result = client.try_issue_reputation(&contract_id, &client_addr, &5_u32, &soroban_sdk::String::from_str(&env, "Great"));
    super::assert_contract_error(result, Error::NotCompleted);
}

#[test]
fn issue_reputation_rejects_invalid_rating() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);
    let comment = soroban_sdk::String::from_str(&env, "Good job");

    let result = client.try_issue_reputation(&contract_id, &client_addr, &5_u32, &soroban_sdk::String::from_str(&env, "Great"));
    super::assert_contract_error(result, Error::InvalidRating);
}

#[test]
fn issue_reputation_once_per_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = super::complete_contract(&env, &client);

    assert!(client.issue_reputation(&contract_id, &client_addr, &5_u32, &soroban_sdk::String::from_str(&env, "Great")));
    let result = client.try_issue_reputation(&contract_id, &client_addr, &5_u32, &soroban_sdk::String::from_str(&env, "Great"));
    super::assert_contract_error(result, Error::ReputationAlreadyIssued);
}

#[test]
fn issue_reputation_rejects_freelancer_mismatch() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);
    let comment = soroban_sdk::String::from_str(&env, "Good job");

    let result = client.try_issue_reputation(&contract_id, &client_addr, &5_u32, &soroban_sdk::String::from_str(&env, "Great"));
    super::assert_contract_error(result, Error::FreelancerMismatch);
}

#[test]
fn issue_reputation_rejects_unauthorized_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (_client_addr, _freelancer_addr, contract_id) = super::complete_contract(&env, &client);
    let unauthorized = soroban_sdk::Address::generate(&env);
    let comment = soroban_sdk::String::from_str(&env, "Good job");

    let result = client.try_issue_reputation(&contract_id, &unauthorized, &5_u32, &soroban_sdk::String::from_str(&env, "Great"));
    super::assert_contract_error(result, Error::UnauthorizedRole);
}

#[test]
fn test_error_code_stability() {
    assert_eq!(Error::IndexOutOfBounds as u32, 3);
    assert_eq!(Error::AlreadyReleased as u32, 4);
    assert_eq!(Error::EmptyRefundRequest as u32, 6);
    assert_eq!(Error::DuplicateMilestoneInRefund as u32, 7);
    assert_eq!(Error::AlreadyRefunded as u32, 8);
    assert_eq!(Error::InsufficientFunds as u32, 9);
    assert_eq!(Error::ContractNotFound as u32, 10);
    assert_eq!(Error::UnauthorizedRole as u32, 11);
    assert_eq!(Error::InvalidParticipants as u32, 14);
    assert_eq!(Error::AmountMustBePositive as u32, 15);
    assert_eq!(Error::InvalidState as u32, 16);
    assert_eq!(Error::EmptyMilestones as u32, 25);
    assert_eq!(Error::InvalidMilestoneAmount as u32, 26);
    assert_eq!(Error::CommentTooLong as u32, 30);
    assert_eq!(Error::InvalidParticipant as u32, 31);
    assert_eq!(Error::InvalidDepositAmount as u32, 32);
    assert_eq!(Error::AlreadyInitialized as u32, 34);
    assert_eq!(Error::NotInitialized as u32, 36);
    assert_eq!(Error::ContractPaused as u32, 37);
    assert_eq!(Error::EmergencyActive as u32, 38);
    assert_eq!(Error::InvalidStatusTransition as u32, 41);
    assert_eq!(Error::AccountingInvariantViolated as u32, 44);
    assert_eq!(Error::AlreadyFinalized as u32, 46);
    assert_eq!(Error::EvidenceTooLong as u32, 47);
    assert_eq!(Error::TimelockNotElapsed as u32, 48);
    assert_eq!(Error::InvalidProtocolParameters as u32, 49);
}

