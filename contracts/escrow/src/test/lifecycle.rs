use super::{create_contract, register_client};
use crate::ContractStatus;
use soroban_sdk::Env;

#[test]
fn successful_contract_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);

    let (_, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Initial state
    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Created);

    // Deposit
    assert!(client.deposit_funds(&contract_id, &super::total_milestone_amount()));
    assert_eq!(
        client.get_contract(&contract_id).status,
        ContractStatus::Funded
    );

    // Release milestones
    assert!(client.release_milestone(&contract_id, &0));
    assert!(client.release_milestone(&contract_id, &1));
    assert!(client.release_milestone(&contract_id, &2));

    let finalized = client.get_contract(&contract_id);
    assert_eq!(finalized.status, ContractStatus::Completed);
    // finalized field is set by finalize_contract, not automatically
    assert!(client.finalize_contract(&contract_id));
    assert!(client.get_contract(&contract_id).finalized);

    // Reputation
    assert_eq!(client.get_pending_reputation_credits(&freelancer_addr), 1);
    assert!(client.issue_reputation(&contract_id, &5, &None));

    let reputation = client.get_reputation_record(&freelancer_addr);
    assert_eq!(reputation.completed_contracts, 1);
    assert_eq!(reputation.total_rating, 5);
}

#[test]
fn contract_refund_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);

    let (_, _, contract_id) = create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &super::total_milestone_amount()));

    // Refund remaining
    assert!(client.refund_remaining_funds(&contract_id));

    let refunded = client.get_contract(&contract_id);
    assert_eq!(refunded.status, ContractStatus::Refunded);
    assert!(refunded.finalized);
}
