# Escrow Security Notes

This document reflects the escrow API currently implemented in `contracts/escrow/src/lib.rs`.

## Implemented Controls

- `initialize(admin)` is single-use and requires `admin.require_auth()`.
- Pause and emergency controls require the stored admin's authorization via explicit `admin.require_auth()` invocations.
- Mutating lifecycle calls check the initialization state and fail immediately if boundaries are breached.
- `create_contract` requires client authorization, rejects identical client/freelancer addresses, rejects empty or non-positive milestones, caps milestone count, and caps total escrow value.
- `deposit_funds` rejects non-positive amounts, repeat exact-total deposits, exact-total mismatches, and incremental overfunding.
- `release_milestone` requires explicit `caller.require_auth()` at the entry layer, enforces the contract's `ReleaseAuthorization` mode (ClientOnly, ArbiterOnly, ClientAndArbiter, or MultiSig), and checks valid non-expired temporary approvals before modifying balances. MultiSig requires both client and freelancer approvals via `check_approvals`.
- `issue_reputation` requires the stored client as caller (with explicit authorization checks), matching freelancer, completed status, rating in `1..=5`, and strictly blocks duplicate issuances for the contract instance.
- `cancel_contract` requires valid client or freelancer authorization and rejects completed, refunded, or already-inactive contracts.
- `finalize_contract` requires client, freelancer, or assigned arbiter authorization, is allowed only from finalized contract states, and locks future modifications via `AlreadyFinalized`.
- Aggregate amount math tracks asset mutations safely. Balance-changing operations verify the core accounting invariant:
  $$total\_deposited == released\_amount + refunded\_amount + available\_balance$$

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