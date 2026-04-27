use super::{complete_contract, default_milestones, register_client, total_milestone_amount};
use crate::EscrowError;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn multiple_contracts_for_same_freelancer() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);

    let (_, freelancer_addr, first_id) = complete_contract(&env, &client);
    assert!(client.issue_reputation(&first_id, &5, &None));

    let client_addr = Address::generate(&env);
    let milestones = default_milestones(&env);
    let second_id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &milestones,
        &None,
        &None,
    );

    assert!(client.deposit_funds(&second_id, &total_milestone_amount()));
    assert!(client.release_milestone(&second_id, &0));
    assert!(client.release_milestone(&second_id, &1));
    assert!(client.release_milestone(&second_id, &2));
    assert!(client.issue_reputation(&second_id, &4, &None));

    let record = client.get_reputation_record(&freelancer_addr);
    assert_eq!(record.completed_contracts, 2);
    assert_eq!(record.total_rating, 9);
}

#[test]
fn scenario_reputation_invalid_rating_zero_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);

    let (_, _, contract_id) = complete_contract(&env, &client);

    let result = client.try_issue_reputation(&contract_id, &0, &None);
    super::assert_contract_error(result, EscrowError::InvalidRating);
}

#[test]
fn scenario_reputation_invalid_rating_six_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);

    let (_, _, contract_id) = complete_contract(&env, &client);

    let result = client.try_issue_reputation(&contract_id, &6, &None);
    super::assert_contract_error(result, EscrowError::InvalidRating);
}
