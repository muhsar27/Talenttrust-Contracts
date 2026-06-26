# Escrow Security Notes

This document reflects the escrow API currently implemented in `contracts/escrow/src/lib.rs`.

## Implemented Controls

- `initialize(admin)` is single-use and requires `admin.require_auth()`.
- Pause and emergency controls require the stored admin's authorization.
- Mutating lifecycle calls fail while paused or in emergency mode.
- `create_contract` requires client authorization, rejects identical
  client/freelancer addresses, rejects empty milestones, caps milestone count,
  caps total escrow value, and validates each milestone amount using centralized
  amount validation (enforcing positivity, minimum positive amount of 1 stroop,
  and a maximum single amount of 1,000,000,000,0000000 stroops/1M tokens).
- `deposit_funds` validates the deposit amount using centralized amount validation
  (enforcing positivity and maximum single amount limits), rejects repeat
  exact-total deposits, exact-total mismatches, and incremental overfunding.
- `release_milestone` requires `caller.require_auth()`, enforces the contract's
  `ReleaseAuthorization` mode (ClientOnly, ArbiterOnly, ClientAndArbiter, or
  MultiSig), and checks valid non-expired approvals before releasing funds.
  MultiSig requires both client and freelancer approvals via `check_approvals`,
  and release may be triggered only by the stored client or freelancer.
- `issue_reputation` requires the stored client as caller, matching freelancer,
  completed status, rating in `1..=5`, and no prior reputation issuance for the
  contract.
- `cancel_contract` requires client or freelancer authorization and rejects
  completed or already-cancelled contracts.
- `finalize_contract` requires client, freelancer, or assigned arbiter
  authorization, is allowed only from `Completed` or `Disputed`, and locks
  future contract-specific mutations with `AlreadyFinalized`.
- Aggregate amount math uses checked helpers where totals are accumulated.
- Balance-changing operations verify the core accounting invariant:
  `total_deposited == released_amount + refunded_amount + available_balance`.
- Finalization summaries use checked arithmetic and persistent storage. They do
  not expire through TTL and do not create, deduct, or withdraw protocol fees.

## Checked Arithmetic & Accounting Invariant

All balance mutations in `deposit_funds`, `release_milestone`, `refund_unreleased_milestones`, `resolve_dispute`, and `issue_reputation` route through `safe_add_amounts` / `safe_subtract_amounts` (which wrap `i128::checked_add` / `checked_sub`) and panic with `PotentialOverflow` on overflow or `AccountingInvariantViolated` on underflow.

After each mutation the core invariant is asserted:

```
funded_amount >= released_amount + refunded_amount
```

This invariant is enforced via the checked helpers in:
- `deposit.rs` — `funded_amount` and `total_deposited` use `safe_add_amounts`
- `lib.rs:release_milestone` — `released_amount` uses `safe_add_amounts`; available_balance uses `safe_subtract_amounts`
- `lib.rs:refund_unreleased_milestones` — `refunded_amount` uses `safe_add_amounts`; available_balance uses `safe_subtract_amounts`
- `lib.rs:get_refundable_balance` — returns checked subtraction result or panics
- `lib.rs:resolve_dispute` — both `refunded_amount` and `released_amount` use `safe_add_amounts`; invariant `funded_amount == released + refunded` is checked
- `lib.rs:issue_reputation` — pending credits and rating totals use checked arithmetic
- `finalize.rs:summarize_contract` — milestone totals and refundable balance use checked arithmetic
- `dispute.rs:resolution_payouts` — uses `checked_sub` / `checked_mul` / `checked_div` throughout

No silent wraparound is possible; any overflow or invariant violation causes an immediate panic with the appropriate `EscrowError` variant.

## Known Live Gaps

- The contract records escrow accounting only. Token custody, token transfers, and atomic asset movement are managed outside `lib.rs` and must be handled by a separate audited integration contract or protocol suite.
- Secure two-step admin state transfer and standalone public protocol fee extraction/withdrawal are not implemented as public entrypoints.
- `ReadinessChecklist.governed_params_set` exists, but no live governance parameter setter entrypoint updates it to `true`.

## Planned Security Work

- Two-step admin transfer: [#318](https://github.com/Talenttrust/Talenttrust-Contracts/issues/318)
- Protocol fee extraction/withdrawal interface: [#314](https://github.com/Talenttrust/Talenttrust-Contracts/issues/314)
- Governed parameter setter/readiness wiring: [#323](https://github.com/Talenttrust/Talenttrust-Contracts/issues/323)
- Structured deposit and fee events: [#336](https://github.com/Talenttrust/Talenttrust-Contracts/issues/336)
- Canonical storage-key reference: [#342](https://github.com/Talenttrust/Talenttrust-Contracts/issues/342)

## Reviewer Checklist

1. Verify no integration guide treats planned entrypoints as live API.
2. Verify pause/emergency blocks every mutating lifecycle call.
3. Verify duplicate release, duplicate reputation issuance, overfunding, and
   invalid amount paths fail closed.
4. Verify off-chain token transfer integrations are atomic or idempotent with
   respect to escrow state changes.
## Refund Gating

`refund_unreleased_milestones` rejects calls when:
- A finalization record exists for the contract (`AlreadyFinalized`).
- The contract status is not `Created`, `Funded`, or `Disputed` (`InvalidState`).

This prevents a client from requesting refunds against a cancelled, completed,
or already-finalized contract.