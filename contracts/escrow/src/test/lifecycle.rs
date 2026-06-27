use crate::{ContractStatus, DepositMode, DisputeResolution, Escrow, EscrowClient, Error, ReleaseAuthorization};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, vec, Address, Env};

fn setup() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Escrow, ());
    (env, contract_id)
}

fn escrow_client<'a>(env: &'a Env, contract_id: &Address) -> EscrowClient<'a> {
    EscrowClient::new(env, contract_id)
}

fn completed_contract(env: &Env, client: &EscrowClient<'_>) -> (Address, Address, u32) {
    let client_addr = Address::generate(env);
    let freelancer_addr = Address::generate(env);
    let contract_id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &vec![env, 100_i128],
        &ReleaseAuthorization::ClientOnly,
    );
    assert!(client.deposit_funds(&contract_id, &client_addr, &100_i128));
    assert!(client.release_milestone(&contract_id, &client_addr, &0));
    assert_eq!(
        client.get_contract(&contract_id).status,
        ContractStatus::Completed
    );
    (client_addr, freelancer_addr, contract_id)
}

fn disputed_contract(env: &Env, client: &EscrowClient<'_>) -> (Address, Address, Address, u32) {
    let client_addr = Address::generate(env);
    let freelancer_addr = Address::generate(env);
    let arbiter = Address::generate(env);
    let contract_id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &Some(arbiter.clone()),
        &vec![env, 100_i128],
        &ReleaseAuthorization::ClientOnly,
    );
    assert!(client.deposit_funds(&contract_id, &client_addr, &100_i128));
    assert!(client.raise_dispute(&contract_id, &client_addr));
    assert_eq!(
        client.get_contract(&contract_id).status,
        ContractStatus::Disputed
    );
    (client_addr, freelancer_addr, arbiter, contract_id)
}

#[test]
fn finalize_completed_contract_persists_immutable_close_record() {
    let (env, client) = setup();
    let client = escrow_client(&env, &client);
    env.ledger().with_mut(|li| li.timestamp = 1_717_171_717);
    let (client_addr, freelancer_addr, contract_id) = completed_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");
    assert_eq!(record.finalizer, client_addr);
    assert_eq!(record.timestamp, 1_717_171_717);
    assert_eq!(record.summary.client, record.finalizer);
    assert_eq!(record.summary.freelancer, freelancer_addr);
    assert_eq!(record.summary.status, ContractStatus::Completed);
    assert_eq!(record.summary.total_amount, 100);
    assert_eq!(record.summary.funded_amount, 100);
    assert_eq!(record.summary.released_amount, 100);
    assert_eq!(record.summary.refundable_balance, 0);
    assert_eq!(record.summary.released_milestone_count, 1);
}

#[test]
fn finalize_disputed_contract_can_be_called_by_assigned_arbiter() {
    let (env, client) = setup();
    let client = escrow_client(&env, &client);
    let (_, _, arbiter, contract_id) = disputed_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &arbiter));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");
    assert_eq!(record.finalizer, arbiter);
    assert_eq!(record.summary.status, ContractStatus::Disputed);
    assert_eq!(record.summary.refundable_balance, 100);
}

#[test]
fn finalize_completed_contract_can_be_called_by_freelancer() {
    let (env, client) = setup();
    let client = escrow_client(&env, &client);
    let (_, freelancer_addr, contract_id) = completed_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &freelancer_addr));

    let record = client
        .get_finalization_record(&contract_id)
        .expect("finalization record should exist");
    assert_eq!(record.finalizer, freelancer_addr);
    assert_eq!(record.summary.status, ContractStatus::Completed);
}

#[test]
fn finalize_rejects_unauthorized_finalizer() {
    let (env, client) = setup();
    let client = escrow_client(&env, &client);
    let (_, _, contract_id) = completed_contract(&env, &client);
    let outsider = Address::generate(&env);

    super::assert_contract_error(
        client.try_finalize_contract(&contract_id, &outsider),
        Error::UnauthorizedRole,
    );
    assert!(client.get_finalization_record(&contract_id).is_none());
}

#[test]
fn finalize_rejects_non_terminal_contract() {
    let (env, client) = setup();
    let client = escrow_client(&env, &client);
    let client_addr = Address::generate(&env);
    let freelancer_addr = Address::generate(&env);
    let contract_id = client.create_contract(
        &client_addr,
        &freelancer_addr,
        &None,
        &vec![&env, 100_i128],
        &ReleaseAuthorization::ClientOnly,
    );

    super::assert_contract_error(
        client.try_finalize_contract(&contract_id, &client_addr),
        Error::InvalidStatusTransition,
    );
    assert!(client.get_finalization_record(&contract_id).is_none());
}

#[test]
fn finalize_is_idempotent_guarded() {
    let (env, client) = setup();
    let client = escrow_client(&env, &client);
    let (client_addr, _, contract_id) = completed_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));
    super::assert_contract_error(
        client.try_finalize_contract(&contract_id, &client_addr),
        Error::AlreadyFinalized,
    );
}

#[test]
fn finalized_contract_rejects_subsequent_mutations() {
    let (env, client) = setup();
    let client = escrow_client(&env, &client);
    let (client_addr, freelancer_addr, contract_id) = completed_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));

    super::assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &1_i128),
        Error::AlreadyFinalized,
    );
    super::assert_contract_error(
        client.try_release_milestone(&contract_id, &client_addr, &0),
        Error::AlreadyFinalized,
    );
    super::assert_contract_error(
        client.try_issue_reputation(&contract_id, &client_addr, &5_u32, &soroban_sdk::String::from_str(&env, "Great")),
        Error::AlreadyFinalized,
    );
    super::assert_contract_error(
        client.try_cancel_contract(&contract_id, &client_addr),
        Error::AlreadyFinalized,
    );
}

#[test]
fn finalized_dispute_rejects_resolution() {
    let (env, client) = setup();
    let client = escrow_client(&env, &client);
    let (client_addr, _, arbiter, contract_id) = disputed_contract(&env, &client);

    assert!(client.finalize_contract(&contract_id, &client_addr));

    super::assert_contract_error(
        client.try_resolve_dispute(&contract_id, &arbiter, &DisputeResolution::FullRefund),
        Error::AlreadyFinalized,
    );
    super::assert_contract_error(
        client.try_raise_dispute(&contract_id, &client_addr),
        Error::AlreadyFinalized,
    );
}

#[test]
fn pause_blocks_finalization() {
    let (env, client) = setup();
    let client = escrow_client(&env, &client);
    let admin = Address::generate(&env);
    assert!(client.initialize(&admin));
    let (client_addr, _, contract_id) = completed_contract(&env, &client);
    assert!(client.pause());

    super::assert_contract_error(
        client.try_finalize_contract(&contract_id, &client_addr),
        Error::ContractPaused,
    );
    assert!(client.get_finalization_record(&contract_id).is_none());
}
