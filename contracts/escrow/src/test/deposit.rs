//! Tests for `deposit_funds` state-machine and event behavior.
//!
//! Issue #441: deposits in installments must transition to
//! [`crate::ContractStatus::PartiallyFunded`] until the milestone total is
//! reached; once the total is met, the contract transitions to
//! [`crate::ContractStatus::Funded`]. Over-funding is rejected before any
//! state mutation. Each successful deposit emits a structured `deposited`
//! event with the resulting status and `funded_amount`.

use super::{
    assert_contract_error, create_contract, register_client, total_milestone_amount,
    MILESTONE_ONE, MILESTONE_TWO, MILESTONE_THREE,
};
use crate::{ContractStatus, EscrowError};
use soroban_sdk::{testutils::{Address as _, Events as _}, Address, Env};

// ─── Single-deposit paths ────────────────────────────────────────────────────

/// `Created -> Funded` when the entire milestone total is deposited in one call.
#[test]
fn deposit_single_full_amount_transitions_created_to_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);
    let total = total_milestone_amount();

    assert!(client.deposit_funds(&contract_id, &client_addr, &total));

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Funded);
    assert_eq!(contract.funded_amount, total);
}

/// `Created -> PartiallyFunded` when only part of the milestone total is deposited.
#[test]
fn deposit_single_partial_amount_transitions_created_to_partially_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    let partial = MILESTONE_ONE; // 1/6 of the milestone total
    assert!(client.deposit_funds(&contract_id, &client_addr, &partial));

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::PartiallyFunded);
    assert_eq!(contract.funded_amount, partial);
    assert!(contract.funded_amount > 0);
    assert!(contract.funded_amount < total_milestone_amount());
}

/// An exact-total-equal deposit (one call equal to the milestone total)
/// transitions to Funded without leaving any partial state and without
/// tripping the over-funding check.
#[test]
fn deposit_exact_total_in_single_call_transitions_to_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    let total = total_milestone_amount();
    assert!(client.deposit_funds(&contract_id, &client_addr, &total));

    let contract = client.get_contract(&contract_id);
    assert_eq!(contract.status, ContractStatus::Funded);
    assert_eq!(contract.funded_amount, total);
}

// ─── Installment (multi-deposit) progression ──────────────────────────────────

/// Created -> PartiallyFunded -> Funded across three installment deposits.
#[test]
fn deposit_installment_progression_traverses_created_partially_funded_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    // First installment: 1/6 of total -> PartiallyFunded.
    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_ONE));
    let after_one = client.get_contract(&contract_id);
    assert_eq!(after_one.status, ContractStatus::PartiallyFunded);
    assert_eq!(after_one.funded_amount, MILESTONE_ONE);

    // Second installment: 3/6 cumulative -> PartiallyFunded.
    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_TWO));
    let after_two = client.get_contract(&contract_id);
    assert_eq!(after_two.status, ContractStatus::PartiallyFunded);
    assert_eq!(
        after_two.funded_amount,
        MILESTONE_ONE.checked_add(MILESTONE_TWO).unwrap()
    );

    // Third installment: 6/6 cumulative -> Funded.
    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_THREE));
    let after_three = client.get_contract(&contract_id);
    assert_eq!(after_three.status, ContractStatus::Funded);
    assert_eq!(after_three.funded_amount, total_milestone_amount());
}

/// `PartiallyFunded` accepts further deposits that remain below the total.
#[test]
fn deposit_on_partially_funded_keeps_partially_funded_when_below_total() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    let first = MILESTONE_ONE;
    assert!(client.deposit_funds(&contract_id, &client_addr, &first));
    let after_first = client.get_contract(&contract_id);
    assert_eq!(after_first.status, ContractStatus::PartiallyFunded);

    let second = MILESTONE_ONE; // strictly less than remaining capacity
    assert!(client.deposit_funds(&contract_id, &client_addr, &second));
    let after_second = client.get_contract(&contract_id);
    assert_eq!(after_second.status, ContractStatus::PartiallyFunded);
    assert_eq!(
        after_second.funded_amount,
        first.checked_add(second).unwrap()
    );
}

/// Many dust-sized deposits stay in `PartiallyFunded` until the final deposit
/// completes the funding.
#[test]
fn deposit_many_partial_installments_each_keep_partially_funded_until_final() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    let dust = 1_i128;
    let total = total_milestone_amount();

    // Make enough dust deposits that `funded_amount == total - 1`.
    let n_partials = total - 1;
    for _ in 0..n_partials {
        assert!(client.deposit_funds(&contract_id, &client_addr, &dust));
        let snapshot = client.get_contract(&contract_id);
        assert_eq!(snapshot.status, ContractStatus::PartiallyFunded);
        assert!(snapshot.funded_amount < total);
    }

    // One more dust deposit -> total -> Funded.
    assert!(client.deposit_funds(&contract_id, &client_addr, &dust));
    let final_state = client.get_contract(&contract_id);
    assert_eq!(final_state.status, ContractStatus::Funded);
    assert_eq!(final_state.funded_amount, total);
}

// ─── Rejections and failure modes ─────────────────────────────────────────────

/// Over-funding (deposit that would push `funded_amount` past `total_amount`)
/// is rejected with `InvalidDepositAmount` and the contract state is unchanged.
#[test]
fn deposit_rejects_amount_pushing_funded_past_total() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    let over = total_milestone_amount().checked_add(1).unwrap();
    assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &over),
        EscrowError::InvalidDepositAmount,
    );

    // State unchanged: still Created, funded_amount == 0.
    let after = client.get_contract(&contract_id);
    assert_eq!(after.status, ContractStatus::Created);
    assert_eq!(after.funded_amount, 0);
}

/// Over-funding while in `PartiallyFunded` is rejected before mutating state.
#[test]
fn deposit_rejects_overfunding_while_partially_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    let partial = MILESTONE_ONE.checked_add(MILESTONE_TWO).unwrap();
    assert!(client.deposit_funds(&contract_id, &client_addr, &partial));

    let after_partial = client.get_contract(&contract_id);
    assert_eq!(after_partial.status, ContractStatus::PartiallyFunded);
    assert_eq!(after_partial.funded_amount, partial);

    let remaining = total_milestone_amount().checked_sub(partial).unwrap();
    let over = remaining.checked_add(1).unwrap();
    assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &over),
        EscrowError::InvalidDepositAmount,
    );

    let after_attempt = client.get_contract(&contract_id);
    assert_eq!(after_attempt.status, ContractStatus::PartiallyFunded);
    assert_eq!(after_attempt.funded_amount, partial);
}

/// An exact-equal partial deposit leaving the contract partial must NOT trip
/// the over-funding check.
#[test]
fn deposit_exactly_remaining_capacity_does_not_trip_overfunding() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    let partial = MILESTONE_TWO; // 1/3 of total
    assert!(client.deposit_funds(&contract_id, &client_addr, &partial));

    let remaining = total_milestone_amount().checked_sub(partial).unwrap();
    assert!(client.deposit_funds(&contract_id, &client_addr, &remaining));

    let after = client.get_contract(&contract_id);
    assert_eq!(after.status, ContractStatus::Funded);
    assert_eq!(after.funded_amount, total_milestone_amount());
}

/// Zero deposits are rejected with `AmountMustBePositive` and the contract
/// remains unchanged. (Dust-attack guard.)
#[test]
fn deposit_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &0),
        EscrowError::AmountMustBePositive,
    );
    let after = client.get_contract(&contract_id);
    assert_eq!(after.status, ContractStatus::Created);
    assert_eq!(after.funded_amount, 0);
}

/// Negative deposits are rejected with `AmountMustBePositive`.
#[test]
fn deposit_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &-1_i128),
        EscrowError::AmountMustBePositive,
    );
    let after = client.get_contract(&contract_id);
    assert_eq!(after.status, ContractStatus::Created);
    assert_eq!(after.funded_amount, 0);
}

/// Calling `deposit_funds` after the contract has reached `Funded` is
/// rejected with `InvalidState`. Subsequent funding is no longer possible.
#[test]
fn deposit_in_funded_state_rejected_with_invalid_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    let funded = client.get_contract(&contract_id);
    assert_eq!(funded.status, ContractStatus::Funded);

    assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &MILESTONE_ONE),
        EscrowError::InvalidState,
    );

    let post_attempt = client.get_contract(&contract_id);
    assert_eq!(post_attempt.status, ContractStatus::Funded);
    assert_eq!(post_attempt.funded_amount, total_milestone_amount());
}

/// Calling `deposit_funds` after a release has begun is rejected with
/// `InvalidState`. (The release flow flips the contract to `Completed` once
/// the final milestone is released.)
#[test]
fn deposit_after_release_rejected_with_invalid_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    // Fully fund and release all three milestones.
    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));
    for i in 0..3u32 {
        assert!(client.approve_milestone_release(&contract_id, &client_addr, &i));
        assert!(client.release_milestone(&contract_id, &client_addr, &i));
    }
    let after_release = client.get_contract(&contract_id);
    assert!(matches!(
        after_release.status,
        ContractStatus::Completed | ContractStatus::Refunded
    ));

    assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &MILESTONE_ONE),
        EscrowError::InvalidState,
    );
}

/// Non-client callers are rejected; even when the contract is partially
/// funded. The auth check happens AFTER the status gate so a deposit attempt
/// against a `Funded`/`Completed` contract does not waste an auth call.
#[test]
fn deposit_non_client_caller_rejected_with_unauthorized_role() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) =
        create_contract(&env, &client);
    let outsider = Address::generate(&env);

    assert_contract_error(
        client.try_deposit_funds(&contract_id, &outsider, &MILESTONE_ONE),
        EscrowError::UnauthorizedRole,
    );
    assert_contract_error(
        client.try_deposit_funds(&contract_id, &freelancer_addr, &MILESTONE_ONE),
        EscrowError::UnauthorizedRole,
    );
    // Even when partially funded, the caller must still be the stored client.
    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_ONE));
    assert_contract_error(
        client.try_deposit_funds(&contract_id, &outsider, &MILESTONE_ONE),
        EscrowError::UnauthorizedRole,
    );

    let after = client.get_contract(&contract_id);
    assert_eq!(after.status, ContractStatus::PartiallyFunded);
    assert_eq!(after.funded_amount, MILESTONE_ONE);
}

/// `deposit_funds` against an unknown `contract_id` is rejected with
/// `ContractNotFound`.
#[test]
fn deposit_unknown_contract_id_rejected_with_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let client_addr = Address::generate(&env);

    assert_contract_error(
        client.try_deposit_funds(&9999_u32, &client_addr, &MILESTONE_ONE),
        EscrowError::ContractNotFound,
    );
}

// ─── Balance persistence and TTL bumps ───────────────────────────────────────

/// Repeated successful deposits track `funded_amount` exactly;
/// `released_amount` and `refunded_amount` remain zero because no release or
/// refund has occurred.
#[test]
fn deposit_does_not_mutate_released_or_refunded_amounts() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_ONE));
    let after_one = client.get_contract(&contract_id);
    assert_eq!(after_one.released_amount, 0);
    assert_eq!(after_one.refunded_amount, 0);

    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_TWO));
    let after_two = client.get_contract(&contract_id);
    assert_eq!(after_two.released_amount, 0);
    assert_eq!(after_two.refunded_amount, 0);

    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_THREE));
    let after_three = client.get_contract(&contract_id);
    assert_eq!(after_three.released_amount, 0);
    assert_eq!(after_three.refunded_amount, 0);
    assert_eq!(after_three.funded_amount, total_milestone_amount());
}

/// `deposit_funds` extends the persistent TTL of the contract so an
/// in-progress installment schedule does not let the contract expire between
/// deposits.
#[test]
fn deposit_extends_persistent_ttl_of_contract_entry() {
    let env = Env::default();
    env.ledger().with_mut(|li| {
        li.max_entry_ttl = crate::ttl::LEDGERS_PER_DAY * 60;
        li.min_persistent_entry_ttl = crate::ttl::LEDGERS_PER_DAY * 60;
        li.sequence_number = 1_000;
    });
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    // Advance so the contract entry sits within `PERSISTENT_BUMP_THRESHOLD`
    // of expiry before the deposit, ensuring the deposit wedges the TTL.
    let bump_threshold = crate::ttl::PERSISTENT_BUMP_THRESHOLD as u32;
    let initial_ttl: u32 = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get_ttl(&crate::DataKey::Contract(contract_id))
    });
    env.ledger().with_mut(|li| {
        li.sequence_number =
            li.sequence_number.saturating_add(initial_ttl.saturating_sub(bump_threshold) + 1);
    });

    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_ONE));

    let ttl_after: u32 = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get_ttl(&crate::DataKey::Contract(contract_id))
    });
    assert!(
        ttl_after >= bump_threshold,
        "deposit must extend TTL to at least the bump threshold (got {})",
        ttl_after
    );
}

// ─── Event publication ───────────────────────────────────────────────────────

/// Each successful deposit publishes exactly one event.
///
/// Count-based assertion keeps the test independent of the exact soroban-sdk
/// event-value decoding APIs across SDK versions, while still proving that
/// the contract emits an indexable event on every installment.
#[test]
fn deposit_publishes_one_event_per_successful_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    let initial_count = env.events().all().len();

    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_ONE));
    assert_eq!(
        env.events().all().len(),
        initial_count + 1,
        "first deposit must publish one event"
    );

    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_ONE));
    assert_eq!(
        env.events().all().len(),
        initial_count + 2,
        "second deposit must publish another event"
    );

    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_TWO));
    assert_eq!(
        env.events().all().len(),
        initial_count + 3,
        "third deposit must publish another event"
    );

    // Final-funding deposit still publishes an event; indexers can distinguish
    // partial vs. final via `funded_amount == total_amount` invariant in the
    // emitted payload.
    assert!(client.deposit_funds(&contract_id, &client_addr, &MILESTONE_TWO));
    assert_eq!(
        env.events().all().len(),
        initial_count + 4,
        "final-funding deposit must publish an event"
    );
}

/// Rejected deposits must NOT publish a new event.
#[test]
fn deposit_publishes_no_event_on_rejection() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, _freelancer_addr, contract_id) =
        create_contract(&env, &client);

    let initial_count = env.events().all().len();

    assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &0),
        EscrowError::AmountMustBePositive,
    );
    assert_contract_error(
        client.try_deposit_funds(
            &contract_id,
            &client_addr,
            &total_milestone_amount().checked_add(1).unwrap(),
        ),
        EscrowError::InvalidDepositAmount,
    );
    assert_contract_error(
        client.try_deposit_funds(&contract_id, &client_addr, &-1_i128),
        EscrowError::AmountMustBePositive,
    );

    assert_eq!(
        env.events().all().len(),
        initial_count,
        "no deposit event should be published on rejection (zero amount, negative amount, or over-funding)"
    );
}

// =========================================================================
// NEGATIVE-PATH TESTS FOR Issue #405
// =========================================================================

/// Tests that deposit_funds panics with AmountMustBePositive when amount == 0.
///
/// Asserts the exact error code for zero-amount deposits.
/// 
/// # Security
/// - Prevents accounting anomalies from zero deposits
/// - Validates amount validation at entry point
#[test]
fn test_deposit_amount_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    let result = client.try_deposit_funds(&contract_id, &client_addr, &0_i128);
    assert_contract_error(result, EscrowError::AmountMustBePositive);
}

/// Tests that deposit_funds panics with AmountMustBePositive when amount < 0.
///
/// Asserts the exact error code for negative amounts.
/// 
/// # Security
/// - Prevents accounting anomalies from negative deposits
/// - Validates amount validation rejects all non-positive values
#[test]
fn test_deposit_amount_negative() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    let result = client.try_deposit_funds(&contract_id, &client_addr, &-1_i128);
    assert_contract_error(result, EscrowError::AmountMustBePositive);
}

/// Tests that deposit_funds panics with ContractNotFound for unknown contract id.
///
/// Asserts the exact error code when contract does not exist in storage.
/// 
/// # Security
/// - Prevents operations on non-existent contracts
/// - Ensures fail-closed behavior for invalid contract IDs
#[test]
fn test_deposit_contract_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let client_addr = Address::generate(&env);

    // Use a contract_id that was never created
    let invalid_contract_id = 9999_u32;
    let result = client.try_deposit_funds(&invalid_contract_id, &client_addr, &100_0000000_i128);
    assert_contract_error(result, EscrowError::ContractNotFound);
}

/// Tests that deposit_funds panics with UnauthorizedRole when caller is not the depositor.
///
/// Asserts the exact error code when an unauthorized address attempts to deposit.
/// 
/// # Security
/// - Prevents unauthorized fund deposits
/// - Enforces client-only deposit authorization
#[test]
fn test_deposit_unauthorized_role() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Attempt deposit from wrong caller (freelancer instead of client)
    let wrong_caller = Address::generate(&env);
    let result = client.try_deposit_funds(&contract_id, &wrong_caller, &100_0000000_i128);
    assert_contract_error(result, EscrowError::UnauthorizedRole);
}

/// Tests that deposit_funds panics with InvalidState when contract is not in Created state.
///
/// Asserts the exact error code when attempting to deposit after contract has been funded.
/// 
/// # Security
/// - Prevents state machine violations
/// - Ensures deposits only occur during contract setup phase
#[test]
fn test_deposit_invalid_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = register_client(&env);
    let (client_addr, freelancer_addr, contract_id) = create_contract(&env, &client);

    // Fully fund the contract first (transitions to Funded state)
    assert!(client.deposit_funds(&contract_id, &client_addr, &total_milestone_amount()));

    // Try to deposit again (contract is now Funded, not Created)
    let result = client.try_deposit_funds(&contract_id, &client_addr, &100_0000000_i128);
    assert_contract_error(result, EscrowError::InvalidState);
}

/// Tests that deposit_funds panics with InsufficientFunds when caller token balance is too low.
///
/// Note: In Soroban test environment with mocked auth, balance checks are typically bypassed.
/// This test documents the error branch but may not be directly testable without token contract integration.
/// 
/// # UNREACHABLE
/// InsufficientFunds in deposit_funds is currently unreachable because:
/// - The contract does not perform balance verification in the current implementation
/// - Token transfer is mocked in test environment
/// - Real balance checks occur only at the token contract level during actual transfers
///
/// Documented per Issue #405 requirements for completeness.
#[test]
#[ignore]
fn test_deposit_insufficient_funds() {
    // UNREACHABLE: deposit_funds does not check caller's token balance
    // in the current implementation. Balance validation occurs at token contract level.
    // This test is documented for completeness but cannot be triggered in unit tests.
}

