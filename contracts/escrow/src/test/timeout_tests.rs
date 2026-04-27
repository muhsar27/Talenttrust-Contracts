use soroban_sdk::{
    testutils::{Address as _, Ledger},
    vec, Address, Env, Vec,
};

use crate::{
    ContractStatus, Escrow, EscrowClient, MilestoneSchedule, ReleaseAuthorization,
};

fn register_client(env: &Env) -> EscrowClient<'_> {
    let contract_id = env.register(Escrow, ());
    EscrowClient::new(env, &contract_id)
}

fn one_schedule(env: &Env, due_date: u64) -> Vec<Option<MilestoneSchedule>> {
    let mut schedules = Vec::new(env);
    schedules.push_back(Some(MilestoneSchedule {
        due_date: Some(due_date),
        title: None,
        description: None,
        updated_at: 0,
    }));
    schedules
}

fn setup_funded_contract(
    env: &Env,
    arbiter: Option<Address>,
) -> (EscrowClient<'_>, Address, Address, Option<Address>, u32, u64) {
    env.mock_all_auths();

    let client = register_client(env);
    let client_addr = Address::generate(env);
    let freelancer_addr = Address::generate(env);
    let due_date = env.ledger().timestamp() + 100;
    let schedules = one_schedule(env, due_date);

    let contract_id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &arbiter,
        &vec![env, 1_0000000_i128],
        &ReleaseAuthorization::ClientOnly,
        &schedules,
    );
    assert!(client.deposit_funds(&contract_id, &client_addr, &1_0000000_i128));

    (
        client,
        client_addr,
        freelancer_addr,
        arbiter,
        contract_id,
        due_date,
    )
}

#[test]
fn approval_is_allowed_at_exact_deadline() {
    let env = Env::default();
    let (client, client_addr, _, _, contract_id, due_date) = setup_funded_contract(&env, None);

    env.ledger().with_mut(|li| {
        li.timestamp = due_date;
    });

    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));
}

#[test]
#[should_panic(expected = "Milestone deadline has expired; contract moved to Disputed")]
fn approval_past_deadline_is_rejected() {
    let env = Env::default();
    let (client, client_addr, _, _, contract_id, due_date) = setup_funded_contract(&env, None);

    env.ledger().with_mut(|li| {
        li.timestamp = due_date + 1;
    });

    client.approve_milestone_release(&contract_id, &client_addr, &0);
}

#[test]
fn evaluate_timeout_transitions_contract_to_disputed() {
    let env = Env::default();
    let (client, _, _, _, contract_id, due_date) = setup_funded_contract(&env, None);

    env.ledger().with_mut(|li| {
        li.timestamp = due_date + 1;
    });

    assert!(client.evaluate_milestone_timeout(&contract_id, &0));
    assert_eq!(client.get_contract(&contract_id).status, ContractStatus::Disputed);
}

#[test]
#[should_panic(expected = "Milestone deadline has expired; contract moved to Disputed")]
fn release_past_deadline_is_rejected() {
    let env = Env::default();
    let (client, client_addr, _, _, contract_id, due_date) = setup_funded_contract(&env, None);

    env.ledger().with_mut(|li| {
        li.timestamp = due_date;
    });
    assert!(client.approve_milestone_release(&contract_id, &client_addr, &0));

    env.ledger().with_mut(|li| {
        li.timestamp = due_date + 1;
    });
    client.release_milestone(&contract_id, &client_addr, &0);
}

#[test]
fn arbiter_resolves_timeout_dispute_after_deadline_extension() {
    let env = Env::default();
    let arbiter = Address::generate(&env);
    let (client, _client_addr, _, Some(arbiter_addr), contract_id, due_date) =
        setup_funded_contract(&env, Some(arbiter.clone()))
    else {
        panic!("arbiter should be present");
    };

    env.ledger().with_mut(|li| {
        li.timestamp = due_date + 1;
    });
    assert!(client.evaluate_milestone_timeout(&contract_id, &0));

    let new_due_date = due_date + 200;
    assert!(client.set_milestone_schedule(
        &contract_id,
        &0,
        &MilestoneSchedule {
            due_date: Some(new_due_date),
            title: None,
            description: None,
            updated_at: 0,
        },
    ));

    assert!(client.resolve_dispute(&contract_id, &arbiter_addr));
    assert_eq!(client.get_contract(&contract_id).status, ContractStatus::Funded);
}

#[test]
fn client_resolves_timeout_dispute_when_no_arbiter_exists() {
    let env = Env::default();
    let (client, client_addr, _, _, contract_id, due_date) = setup_funded_contract(&env, None);

    env.ledger().with_mut(|li| {
        li.timestamp = due_date + 1;
    });
    assert!(client.evaluate_milestone_timeout(&contract_id, &0));

    let new_due_date = due_date + 200;
    assert!(client.set_milestone_schedule(
        &contract_id,
        &0,
        &MilestoneSchedule {
            due_date: Some(new_due_date),
            title: None,
            description: None,
            updated_at: 0,
        },
    ));

    assert!(client.resolve_dispute(&contract_id, &client_addr));
    assert_eq!(client.get_contract(&contract_id).status, ContractStatus::Funded);
}
