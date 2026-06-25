#![cfg(test)]

use super::register_client;
use soroban_sdk::{testutils::Address as _, testutils::Events as _, Address, Env};

#[test]
fn protocol_fee_bps_change_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);

    let admin = Address::generate(&env);
    // initialize sets the admin for the contract
    client.initialize(&admin);

    // Change protocol fee bps
    assert!(client.set_protocol_fee_bps(&100u32));

    let events = env.events().all();
    assert!(events.len() > 0);

    // Ensure an event with the protocol_fee_bps topic exists
    let found = events.iter().any(|event| event.0 == soroban_sdk::Symbol::new(&env, "protocol_fee_bps"));
    assert!(found);
}

#[test]
fn admin_propose_and_accept_emit_events() {
    let env = Env::default();
    env.mock_all_auths();

    let client = register_client(&env);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let next_admin = Address::generate(&env);
    assert!(client.propose_governance_admin(&next_admin));

    // Accept requires the proposed admin to authorize — mock_all_auths covers this.
    assert!(client.accept_governance_admin());

    let events = env.events().all();
    assert!(events.len() > 0);

    // Ensure admin-topic events exist (proposed / accepted)
    let found_admin_topic = events.iter().any(|event| event.0 == soroban_sdk::symbol_short!("admin"));
    assert!(found_admin_topic);
}
