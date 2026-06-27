//! Property-based tests for the escrow accounting invariant.
//!
//! Drives random sequences of `deposit_funds`, `approve_milestone_release`,
//! `release_milestone`, and `refund_unreleased_milestones` against the live
//! Soroban test environment and asserts after every operation that:
//!
//!   `funded_amount - released_amount - refunded_amount >= 0`
//!
//! Also asserts that:
//! - `funded_amount` is never exceeded by `released + refunded`
//! - Status transitions are monotone and eventually reach a terminal state
//!
//! ## Running
//!
//! ```sh
//! # Default 256 cases per property:
//! cargo test -p escrow proptest
//!
//! # More cases:
//! PROPTEST_CASES=1024 cargo test -p escrow proptest
//!
//! # Reproduce a specific failure:
//! PROPTEST_SEED=<hex> cargo test -p escrow proptest
//! ```
//!
//! Failing seeds are auto-saved to `proptest-regressions/proptest.txt`.

#![cfg(test)]

extern crate std;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::vec::Vec as StdVec;

use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec as SorobanVec};

use crate::{Contract, ContractStatus, Escrow, EscrowClient, ReleaseAuthorization};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_MS: usize = 6;
const MAX_AMOUNT: i128 = 1_000_000_000;
const MAX_OPS: usize = 30;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Generate a list of positive milestone amounts (1 .. MAX_AMOUNT).
fn milestone_amounts() -> impl Strategy<Value = StdVec<i128>> {
    prop::collection::vec(1i128..=MAX_AMOUNT, 1..=MAX_MS)
}

/// The set of operations the proptest can generate.
#[derive(Clone, Debug)]
enum Op {
    /// Deposit `amount` into the contract (caller: client).
    Deposit(i128),
    /// Approve milestone `index` for release (caller: client).
    Approve(u32),
    /// Release milestone `index` (caller: client, requires prior approval).
    Release(u32),
    /// Refund the given set of milestone indices (caller: client).
    Refund(StdVec<u32>),
}

/// Build an operation strategy that knows how many milestones exist and
/// the total milestone sum so it can generate sensible deposit amounts.
fn op_strategy(n_ms: usize, total: i128) -> impl Strategy<Value = Op> {
    let n = n_ms as u32;
    // Deposit amounts anywhere from 1 to 2x the total (some will overshoot).
    let overshoot = total.saturating_mul(2).max(1);
    prop_oneof![
        (1i128..=overshoot).prop_map(Op::Deposit),
        (0u32..n).prop_map(Op::Approve),
        (0u32..n).prop_map(Op::Release),
        prop::collection::vec(0u32..n, 1..=n).prop_map(Op::Refund),
    ]
}

/// Generate a random sequence of operations.
fn ops_strategy(n_ms: usize, total: i128) -> impl Strategy<Value = StdVec<Op>> {
    prop::collection::vec(op_strategy(n_ms, total), 0..=MAX_OPS)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sum(amounts: &[i128]) -> i128 {
    amounts.iter().copied().sum()
}

struct Harness {
    env: Env,
    client_addr: Address,
    freelancer_addr: Address,
}

impl Harness {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let client_addr = Address::generate(&env);
        let freelancer_addr = Address::generate(&env);
        Harness {
            env,
            client_addr,
            freelancer_addr,
        }
    }

    fn escrow_client(&self) -> EscrowClient<'_> {
        let id = self.env.register(Escrow, ());
        EscrowClient::new(&self.env, &id)
    }
}

// ---------------------------------------------------------------------------
// Safe wrappers — run an operation and return whether it succeeded.
// ---------------------------------------------------------------------------

fn try_deposit(client: &EscrowClient, id: u32, caller: &Address, amount: i128) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        client.deposit_funds(&id, caller, &amount);
    }))
    .is_ok()
}

fn try_approve(client: &EscrowClient, id: u32, caller: &Address, ms_idx: u32) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        client.approve_milestone_release(&id, caller, &ms_idx);
    }))
    .is_ok()
}

fn try_release(client: &EscrowClient, id: u32, caller: &Address, ms_idx: u32) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        client.release_milestone(&id, caller, &ms_idx);
    }))
    .is_ok()
}

fn try_refund(
    client: &EscrowClient,
    env: &Env,
    id: u32,
    indices: &[u32],
) -> Result<i128, ()> {
    let v: SorobanVec<u32> = {
        let mut tmp = SorobanVec::new(env);
        for &i in indices {
            tmp.push_back(i);
        }
        tmp
    };
    catch_unwind(AssertUnwindSafe(|| {
        client.refund_unreleased_milestones(&id, &v)
    }))
    .map_or(Err(()), |r| Ok(r))
}

// ---------------------------------------------------------------------------
// Invariant checker
// ---------------------------------------------------------------------------

/// Assert the core accounting invariant:
/// `funded_amount - released_amount - refunded_amount >= 0`
fn assert_invariant(client: &EscrowClient, id: u32) {
    let d: Contract = client.get_contract(&id);
    let available = d.funded_amount - d.released_amount - d.refunded_amount;
    assert!(
        available >= 0,
        "invariant violated: funded={}, released={}, refunded={}, available={}",
        d.funded_amount,
        d.released_amount,
        d.refunded_amount,
        available,
    );
    // Also check that released + refunded does NOT exceed funded.
    assert!(
        d.released_amount + d.refunded_amount <= d.funded_amount,
        "released+refunded > funded: {} + {} > {}",
        d.released_amount,
        d.refunded_amount,
        d.funded_amount,
    );
}

// ---------------------------------------------------------------------------
// Status transition monotonicity helper
// ---------------------------------------------------------------------------

/// Returns `true` if `next` is a valid monotonic transition from `prev`.
/// Terminal states (Completed, Refunded, Cancelled) should never be left.
fn is_valid_transition(prev: ContractStatus, next: ContractStatus) -> bool {
    use ContractStatus::*;
    match (prev, next) {
        // Terminal states are absorbing.
        (Completed, Completed)
        | (Refunded, Refunded)
        | (Cancelled, Cancelled) => true,
        (Completed, _) | (Refunded, _) | (Cancelled, _) => false,
        // Forward transitions.
        (Created, Created)
        | (Created, Funded)
        | (Created, Cancelled) => true,
        (Funded, Funded)
        | (Funded, Completed)
        | (Funded, Refunded)
        | (Funded, Cancelled) => true,
        (PartiallyFunded, PartiallyFunded)
        | (PartiallyFunded, Funded)
        | (PartiallyFunded, Cancelled) => true,
        (Accepted, Accepted)
        | (Accepted, Funded)
        | (Accepted, Cancelled) => true,
        (_, Disputed) => true,
        // Everything else is invalid.
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

const DEFAULT_CASES: u32 = 256;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: DEFAULT_CASES,
        ..ProptestConfig::default()
    })]

    /// After every deposit / approve / release / refund operation the
    /// accounting invariant must hold and available balance must never
    /// go negative.  Operation failures are tolerated — the invariant
    /// must hold regardless.
    #[test]
    fn prop_accounting_invariant_holds_under_random_ops(
        (amounts, ops) in milestone_amounts().prop_flat_map(|amounts| {
            let total = sum(&amounts);
            let n = amounts.len();
            (Just(amounts), ops_strategy(n, total))
        })
    ) {
        let h = Harness::new();
        let client = h.escrow_client();
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        assert_invariant(&client, id);

        let ms_count = amounts.len() as u32;

        for op in &ops {
            match op {
                Op::Deposit(amount) => {
                    let _ = try_deposit(&client, id, &h.client_addr, *amount);
                }
                Op::Approve(ms_idx) => {
                    if *ms_idx < ms_count {
                        let _ = try_approve(&client, id, &h.client_addr, *ms_idx);
                    }
                }
                Op::Release(ms_idx) => {
                    if *ms_idx < ms_count {
                        let _ = try_release(&client, id, &h.client_addr, *ms_idx);
                    }
                }
                Op::Refund(indices) => {
                    // Only try if there are uniquely valid indices.
                    let mut dedup: StdVec<u32> = indices.clone();
                    dedup.sort_unstable();
                    dedup.dedup();
                    dedup.retain(|&i| i < ms_count);
                    if !dedup.is_empty() {
                        let _ = try_refund(&client, &h.env, id, &dedup);
                    }
                }
            }
            // Invariant must hold regardless of whether the op succeeded.
            assert_invariant(&client, id);
        }

        // Final invariant check.
        assert_invariant(&client, id);
    }

    /// Full cycle: deposit the exact total, approve each milestone, then
    /// release each.  After every operation the invariant holds, and at
    /// the end status is Completed.
    #[test]
    fn prop_full_release_sequence_invariant(amounts in milestone_amounts()) {
        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        assert_invariant(&client, id);

        // Deposit the exact total.
        assert!(try_deposit(&client, id, &h.client_addr, total));
        assert_invariant(&client, id);

        let n_ms = amounts.len() as u32;
        for i in 0..n_ms {
            assert!(try_approve(&client, id, &h.client_addr, i));
            assert_invariant(&client, id);
            assert!(try_release(&client, id, &h.client_addr, i));
            assert_invariant(&client, id);
        }

        let data = client.get_contract(&id);
        prop_assert_eq!(data.status, ContractStatus::Completed);
        prop_assert_eq!(data.released_amount, total);
        prop_assert_eq!(data.refunded_amount, 0);
        prop_assert_eq!(data.funded_amount, total);
    }

    /// Full refund cycle: deposit the exact total then refund all
    /// milestones.  The invariant must hold after every step and the
    /// final status must be Refunded.
    #[test]
    fn prop_full_refund_sequence_invariant(amounts in milestone_amounts()) {
        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        assert!(try_deposit(&client, id, &h.client_addr, total));
        assert_invariant(&client, id);

        let all_indices: StdVec<u32> = (0..amounts.len() as u32).collect();
        let refunded = try_refund(&client, &h.env, id, &all_indices);
        prop_assert_eq!(refunded, Ok(total));
        assert_invariant(&client, id);

        let data = client.get_contract(&id);
        prop_assert_eq!(data.status, ContractStatus::Refunded);
        prop_assert_eq!(data.released_amount, 0);
        prop_assert_eq!(data.refunded_amount, total);
        prop_assert_eq!(data.funded_amount, total);
    }

    /// Mixed release-then-refund: release some milestones, refund the
    /// rest.  The final status should be `Completed` (mixed outcome).
    #[test]
    fn prop_mixed_release_refund_invariant(
        amounts in milestone_amounts(),
        split in 0u32..10u32,
    ) {
        let n = amounts.len();
        prop_assume!(n >= 2);
        let split_point = (split as usize) % (n - 1) + 1; // 1 .. n-1

        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        assert!(try_deposit(&client, id, &h.client_addr, total));
        assert_invariant(&client, id);

        // Release first `split_point` milestones.
        let mut released_sum: i128 = 0;
        for i in 0..split_point as u32 {
            assert!(try_approve(&client, id, &h.client_addr, i));
            assert!(try_release(&client, id, &h.client_addr, i));
            released_sum += amounts[i as usize];
            assert_invariant(&client, id);
        }

        // Refund the remaining milestones.
        let refund_indices: StdVec<u32> = (split_point as u32..n as u32).collect();
        let refunded = try_refund(&client, &h.env, id, &refund_indices);
        prop_assert!(refunded.is_ok());
        assert_invariant(&client, id);

        let data = client.get_contract(&id);
        // If all milestones are now released-or-refunded, status is Completed.
        prop_assert_eq!(data.status, ContractStatus::Completed);
        prop_assert_eq!(data.released_amount, released_sum);
        assert_invariant(&client, id);
    }

    /// Double-release of the same milestone must be rejected and must
    /// never corrupt the invariant.
    #[test]
    fn prop_double_release_rejected_invariant_preserved(
        amounts in milestone_amounts(),
        target_raw in 0u32..MAX_MS as u32,
    ) {
        let n = amounts.len() as u32;
        prop_assume!(n > 0);
        let target = target_raw % n;

        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        assert!(try_deposit(&client, id, &h.client_addr, total));
        assert!(try_approve(&client, id, &h.client_addr, target));
        assert!(try_release(&client, id, &h.client_addr, target));
        assert_invariant(&client, id);

        let before = client.get_contract(&id);
        // Second release of the same milestone must fail.
        prop_assert!(!try_release(&client, id, &h.client_addr, target));
        let after = client.get_contract(&id);
        prop_assert_eq!(before.released_amount, after.released_amount);
        prop_assert_eq!(before.funded_amount, after.funded_amount);
        assert_invariant(&client, id);
    }

    /// Over-deposit: depositing more than the contract total must be
    /// rejected and must not corrupt the invariant.
    #[test]
    fn prop_overdeposit_rejected_invariant_preserved(amounts in milestone_amounts()) {
        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        // Deposit the exact total.
        assert!(try_deposit(&client, id, &h.client_addr, total));
        assert_invariant(&client, id);

        // Any further deposit (even 1 stroop) must be rejected because
        // the contract moves out of Created state once fully funded.
        prop_assert!(!try_deposit(&client, id, &h.client_addr, 1));
        assert_invariant(&client, id);
    }

    /// Empty operation sequence — the invariant must hold with no ops
    /// at all (just contract creation).
    #[test]
    fn prop_empty_sequence_invariant(amounts in milestone_amounts()) {
        let h = Harness::new();
        let client = h.escrow_client();
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let _id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );
        assert_invariant(&client, 1u32);
    }

    /// Adversarial: try to release a milestone that has not been approved.
    /// This should be rejected, and the invariant must hold.
    #[test]
    fn prop_release_without_approval_rejected(
        amounts in milestone_amounts(),
        raw_idx in 0u32..MAX_MS as u32,
    ) {
        let n = amounts.len() as u32;
        prop_assume!(n > 0);
        let idx = raw_idx % n;

        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        assert!(try_deposit(&client, id, &h.client_addr, total));
        assert_invariant(&client, id);

        // Release WITHOUT prior approval must fail.
        prop_assert!(!try_release(&client, id, &h.client_addr, idx));
        assert_invariant(&client, id);
    }

    /// Status must be monotone toward terminal states — once Completed,
    /// Refunded, or Cancelled, no further changes should be possible.
    #[test]
    fn prop_status_transitions_monotone(amounts in milestone_amounts()) {
        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        let mut prev_status = client.get_contract(&id).status;

        // Deposit, approve and release all milestones.
        assert!(try_deposit(&client, id, &h.client_addr, total));
        let cur = client.get_contract(&id).status;
        prop_assert!(is_valid_transition(prev_status, cur));
        prev_status = cur;

        let n_ms = amounts.len() as u32;
        for i in 0..n_ms {
            assert!(try_approve(&client, id, &h.client_addr, i));
            // Approve does not change status.
            let cur = client.get_contract(&id).status;
            prop_assert!(is_valid_transition(prev_status, cur));
            prev_status = cur;

            assert!(try_release(&client, id, &h.client_addr, i));
            let cur = client.get_contract(&id).status;
            prop_assert!(is_valid_transition(prev_status, cur));
            prev_status = cur;
        }

        // Terminal: Completed.
        prop_assert_eq!(prev_status, ContractStatus::Completed);

        // Any further operation must keep status as Completed.
        prop_assert!(!try_release(&client, id, &h.client_addr, 0));
        prop_assert_eq!(client.get_contract(&id).status, ContractStatus::Completed);
        assert_invariant(&client, id);
    }

    /// Max-value milestone amounts (i128::MAX / small count) must not
    /// cause arithmetic overflow and invariant must hold.
    #[test]
    fn prop_large_amounts_invariant_preserved(
        small_count in 1u32..=3u32,
    ) {
        // Use amounts in the i128::MAX / 3 range to avoid multiplicative overflow.
        let max_safe = i128::MAX / 3;
        let amounts: StdVec<i128> = (0..small_count)
            .map(|i| (max_safe / (small_count as i128)) * (i + 1))
            .collect();
        // Avoid zero amounts.
        let amounts: StdVec<i128> = amounts.into_iter().map(|a| if a <= 0 { 1 } else { a }).collect();

        let h = Harness::new();
        let client = h.escrow_client();
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        // Deposit a tiny fraction to keep arithmetic safe in test env.
        let tiny = 1_000i128;
        assert!(try_deposit(&client, id, &h.client_addr, tiny));
        assert_invariant(&client, id);
    }

    // -----------------------------------------------------------------------
    // Deposit-promotion property tests
    //
    // These properties exercise the promotion boundary in `deposit_funds`:
    //
    //   if funded_amount >= sum(milestones) && status == Created
    //       then status = Funded
    //
    // Across all random splits, orderings, and overshoots.
    // -----------------------------------------------------------------------

    /// **Promotion threshold property (split deposits)**
    ///
    /// Given a random milestone vector with total `T` and a random sequence of
    /// positive deposit chunks, after each chunk:
    ///
    /// - `funded_amount` equals the running sum of accepted deposits.
    /// - `status == Created`  iff  `funded_amount < T`.
    /// - `status == Funded`   iff  `funded_amount >= T`.
    /// - Once `Funded`, all subsequent deposits are rejected (status is no
    ///   longer `Created`) and `funded_amount` does not change.
    ///
    /// This validates that promotion happens **precisely** at the crossing
    /// deposit — no premature promotion and no missed promotion.
    #[test]
    fn prop_deposit_promotion_boundary_across_splits(
        (amounts, chunks) in milestone_amounts().prop_flat_map(|amounts| {
            let total = sum(&amounts);
            // Generate 1–8 positive chunk sizes.  They need not sum to total;
            // we will stop depositing once the contract is funded.
            let chunks = prop::collection::vec(1i128..=total.max(1), 1..=8usize);
            (Just(amounts), chunks)
        })
    ) {
        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        // Contract starts Created with zero funded.
        {
            let d = client.get_contract(&id);
            prop_assert_eq!(d.status, ContractStatus::Created);
            prop_assert_eq!(d.funded_amount, 0i128);
        }

        let mut cumulative: i128 = 0;

        for chunk in &chunks {
            let before = client.get_contract(&id);

            if before.status != ContractStatus::Created {
                // Already promoted — further deposits must be rejected.
                let ok = try_deposit(&client, id, &h.client_addr, *chunk);
                prop_assert!(!ok, "deposit accepted on a non-Created contract");
                // State must be unchanged.
                let after = client.get_contract(&id);
                prop_assert_eq!(after.funded_amount, before.funded_amount);
                prop_assert_eq!(after.status, before.status);
                continue;
            }

            // Contract is still Created — deposit must succeed.
            let ok = try_deposit(&client, id, &h.client_addr, *chunk);
            prop_assert!(ok, "deposit rejected while contract is still Created");
            cumulative += chunk;

            let after = client.get_contract(&id);

            // funded_amount must equal cumulative sum.
            prop_assert_eq!(
                after.funded_amount, cumulative,
                "funded_amount mismatch: expected {}, got {}",
                cumulative, after.funded_amount,
            );

            // Promotion rule: Funded iff cumulative >= total.
            if cumulative >= total {
                prop_assert_eq!(
                    after.status,
                    ContractStatus::Funded,
                    "contract not promoted despite funded_amount ({}) >= total ({})",
                    cumulative, total,
                );
            } else {
                prop_assert_eq!(
                    after.status,
                    ContractStatus::Created,
                    "premature promotion: funded_amount ({}) < total ({})",
                    cumulative, total,
                );
            }
        }
    }

    /// **Exact-total single-shot promotion**
    ///
    /// Depositing exactly `sum(milestones)` in one call must always
    /// transition status from `Created` to `Funded` and set
    /// `funded_amount == total`.
    #[test]
    fn prop_exact_total_single_deposit_promotes_to_funded(
        amounts in milestone_amounts(),
    ) {
        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        prop_assert_eq!(
            client.get_contract(&id).status,
            ContractStatus::Created,
        );

        let ok = try_deposit(&client, id, &h.client_addr, total);
        prop_assert!(ok, "exact-total deposit was rejected");

        let d = client.get_contract(&id);
        prop_assert_eq!(d.status, ContractStatus::Funded);
        prop_assert_eq!(d.funded_amount, total);
    }

    /// **Under-funded never promotes**
    ///
    /// Any sequence of deposits whose cumulative sum is strictly less than
    /// `sum(milestones)` must leave the contract in `Created` state.
    /// `funded_amount` must equal the cumulative sum exactly.
    #[test]
    fn prop_underfunded_deposits_never_promote(
        (amounts, chunks) in milestone_amounts().prop_flat_map(|amounts| {
            let total = sum(&amounts);
            prop_assume!(total > 1);
            // Chunks sum to at most total-1.  Generate individual sizes
            // ≤ total/2 so we can safely accumulate without crossing.
            let max_chunk = (total / 2).max(1);
            let chunks = prop::collection::vec(1i128..=max_chunk, 1..=4usize);
            (Just(amounts), chunks)
        })
    ) {
        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        let mut cumulative: i128 = 0;

        for chunk in &chunks {
            // Only deposit while cumulative would stay below total.
            if cumulative + chunk >= total {
                break;
            }
            let ok = try_deposit(&client, id, &h.client_addr, *chunk);
            prop_assert!(ok);
            cumulative += chunk;

            let d = client.get_contract(&id);
            prop_assert_eq!(d.funded_amount, cumulative);
            prop_assert_eq!(
                d.status,
                ContractStatus::Created,
                "premature promotion at funded_amount={} total={}",
                cumulative, total,
            );
        }
    }

    /// **Overshoot deposit promotes at the crossing chunk**
    ///
    /// If the deposit amount is strictly greater than the remaining
    /// balance needed (`total - pre_funded`), the contract must still
    /// transition to `Funded` — the overshoot is accepted and stored in
    /// `funded_amount`, and the status becomes `Funded`.
    ///
    /// This confirms the implementation's "accept overshoot" semantics:
    /// a deposit is not capped at the remaining amount, it is stored as-is.
    #[test]
    fn prop_overshoot_deposit_promotes_to_funded(
        (amounts, prefund_raw) in milestone_amounts().prop_flat_map(|amounts| {
            let total = sum(&amounts);
            // Pre-fund with 0 .. total-1 stroops so there is always a gap.
            let pre = if total > 1 { 0i128..total } else { 0i128..1i128 };
            (Just(amounts), pre)
        })
    ) {
        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        // Deposit the pre-fund amount if non-zero.
        if prefund_raw > 0 {
            prop_assume!(prefund_raw < total);
            let ok = try_deposit(&client, id, &h.client_addr, prefund_raw);
            prop_assert!(ok);
            prop_assert_eq!(
                client.get_contract(&id).status,
                ContractStatus::Created,
            );
        }

        // Now overshoot: deposit total+1 more than the gap.
        let gap = total - prefund_raw;
        let overshoot = gap + 1;
        // overshoot > 0 is guaranteed since gap >= 1.
        let ok = try_deposit(&client, id, &h.client_addr, overshoot);
        prop_assert!(ok, "overshoot deposit was rejected");

        let d = client.get_contract(&id);
        prop_assert_eq!(
            d.status,
            ContractStatus::Funded,
            "overshoot deposit did not promote: funded={} total={}",
            d.funded_amount, total,
        );
        prop_assert_eq!(d.funded_amount, prefund_raw + overshoot);
        prop_assert!(
            d.funded_amount >= total,
            "funded_amount ({}) should be >= total ({}) after overshoot",
            d.funded_amount, total,
        );
    }

    /// **funded_amount tracks cumulative deposits exactly**
    ///
    /// For any number of sequential deposits (each accepted), the
    /// contract's `funded_amount` must equal the running sum.  No stroop
    /// is created or destroyed.
    #[test]
    fn prop_funded_amount_equals_cumulative_deposits(
        (amounts, chunks) in milestone_amounts().prop_flat_map(|amounts| {
            let total = sum(&amounts);
            // Chunks strictly below total so contract stays Created throughout,
            // allowing multiple deposits.
            let chunk_max = (total / 4).max(1);
            let chunks = prop::collection::vec(1i128..=chunk_max, 1..=4usize);
            (Just(amounts), chunks)
        })
    ) {
        let h = Harness::new();
        let client = h.escrow_client();
        let total = sum(&amounts);
        let ms: SorobanVec<i128> = {
            let mut v = SorobanVec::new(&h.env);
            for &a in &amounts {
                v.push_back(a);
            }
            v
        };
        let id = client.create_contract(
            &h.client_addr,
            &h.freelancer_addr,
            &None,
            &ms,
            &ReleaseAuthorization::ClientOnly,
        );

        let mut cumulative: i128 = 0;

        for chunk in &chunks {
            if cumulative + chunk >= total {
                // Would promote — stop here to keep the focus on pre-promotion
                // funded_amount tracking.
                break;
            }
            let ok = try_deposit(&client, id, &h.client_addr, *chunk);
            prop_assert!(ok);
            cumulative += chunk;

            let d = client.get_contract(&id);
            prop_assert_eq!(
                d.funded_amount,
                cumulative,
                "funded_amount ({}) != cumulative deposits ({})",
                d.funded_amount,
                cumulative,
            );
            prop_assert_eq!(d.released_amount, 0i128);
            prop_assert_eq!(d.refunded_amount, 0i128);
        }
    }

    /// **Promotion is permutation-invariant**
    ///
    /// Given a set of deposits chunks `[c1, c2, ... cn]` that together
    /// sum to exactly the milestone total, the contract must reach
    /// `Funded` regardless of which order the chunks arrive.
    ///
    /// Tests two orderings: forward and reversed.  Both must end in
    /// `Funded` with the same `funded_amount`.
    #[test]
    fn prop_promotion_is_order_independent(
        amounts in milestone_amounts(),
        n_chunks in 1u32..=5u32,
    ) {
        let total = sum(&amounts);
        prop_assume!(total >= n_chunks as i128);

        // Split total into n_chunks equal-ish pieces (last takes remainder).
        let base = total / n_chunks as i128;
        let rem  = total % n_chunks as i128;
        let mut chunks: StdVec<i128> = (0..n_chunks as i128)
            .map(|i| if i == n_chunks as i128 - 1 { base + rem } else { base })
            .filter(|&c| c > 0)
            .collect();
        prop_assume!(!chunks.is_empty());
        prop_assume!(chunks.iter().sum::<i128>() == total);

        // --- Forward order ---
        let h1 = Harness::new();
        let c1 = h1.escrow_client();
        {
            let ms: SorobanVec<i128> = {
                let mut v = SorobanVec::new(&h1.env);
                for &a in &amounts { v.push_back(a); }
                v
            };
            let id = c1.create_contract(
                &h1.client_addr, &h1.freelancer_addr, &None, &ms,
                &ReleaseAuthorization::ClientOnly,
            );
            for &chunk in &chunks {
                let ok = try_deposit(&c1, id, &h1.client_addr, chunk);
                prop_assert!(ok);
            }
            let d = c1.get_contract(&id);
            prop_assert_eq!(d.status, ContractStatus::Funded);
            prop_assert_eq!(d.funded_amount, total);
        }

        // --- Reversed order ---
        chunks.reverse();
        let h2 = Harness::new();
        let c2 = h2.escrow_client();
        {
            let ms: SorobanVec<i128> = {
                let mut v = SorobanVec::new(&h2.env);
                for &a in &amounts { v.push_back(a); }
                v
            };
            let id = c2.create_contract(
                &h2.client_addr, &h2.freelancer_addr, &None, &ms,
                &ReleaseAuthorization::ClientOnly,
            );
            for &chunk in &chunks {
                let ok = try_deposit(&c2, id, &h2.client_addr, chunk);
                prop_assert!(ok);
            }
            let d = c2.get_contract(&id);
            prop_assert_eq!(d.status, ContractStatus::Funded);
            prop_assert_eq!(d.funded_amount, total);
        }
    }
}
