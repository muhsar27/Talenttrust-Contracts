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

## Canonical Error Codes

The following table documents the authoritative, consolidated contract error codes (`Error` enum in `types.rs`) and their trigger conditions. To ensure client SDK stability, all error codes are treated as append-only.

| Error Code | Variant Name | Trigger Condition |
| :--- | :--- | :--- |
| 3 | `IndexOutOfBounds` | The specified milestone index is out of bounds. |
| 4 | `AlreadyReleased` | The milestone has already been released. |
| 6 | `EmptyRefundRequest` | The refund request contains no milestones. |
| 7 | `DuplicateMilestoneInRefund` | Duplicate milestone indices specified in the refund request. |
| 8 | `AlreadyRefunded` | The milestone has already been refunded. |
| 9 | `InsufficientFunds` | Insufficient funds available to perform the operation. |
| 10 | `ContractNotFound` | The requested contract ID was never allocated or does not exist. |
| 11 | `UnauthorizedRole` | The caller's authorization check failed for this operation. |
| 12 | `MissingArbiter` | The contract requires an arbiter address but none was provided. |
| 13 | `InvalidArbiter` | The provided arbiter address is same as client or freelancer. |
| 14 | `InvalidParticipants` | The client and freelancer addresses are identical or invalid. |
| 15 | `AmountMustBePositive` | The amount or milestone amount must be strictly greater than zero. |
| 16 | `InvalidState` | The contract is in an invalid lifecycle state for this operation. |
| 17 | `MilestoneAlreadyReleased` | The milestone is already released and cannot be released/refunded. |
| 18 | `AlreadyApproved` | The milestone has already been approved. |
| 20 | `InsufficientApprovals` | The milestone has not received sufficient approvals to release. |
| 21 | `FreelancerMismatch` | The freelancer address does not match the stored freelancer. |
| 22 | `InvalidRating` | The rating value is outside the allowed range (1 to 5). |
| 23 | `ReputationAlreadyIssued` | Reputation has already been issued for this contract. |
| 25 | `EmptyMilestones` | The milestone list cannot be empty when creating a contract. |
| 26 | `InvalidMilestoneAmount` | The milestone amount violates validity limits (e.g. non-positive). |
| 27 | `ContractIdCollision` | A contract with the specified ID already exists. |
| 28 | `ContractIdOverflow` | The contract ID has overflowed the maximum limit. |
| 29 | `EmptyComment` | The comment string is empty. |
| 30 | `CommentTooLong` | The comment string exceeds the maximum length limit. |
| 31 | `InvalidParticipant` | The participant address is invalid (e.g. self-matching). |
| 32 | `InvalidDepositAmount` | The deposit amount is invalid. |
| 33 | `InvalidMilestone` | The milestone configuration or index is invalid. |
| 34 | `AlreadyInitialized` | The contract has already been initialized. |
| 35 | `InsufficientAccumulatedFees` | Insufficient accumulated fees available for extraction. |
| 36 | `NotInitialized` | The contract has not been initialized. |
| 37 | `ContractPaused` | The contract is currently paused. |
| 38 | `EmergencyActive` | Emergency mode is currently active. |
| 39 | `SelfRating` | Self-rating is not allowed (client and freelancer addresses collapse). |
| 40 | `NotCompleted` | The contract has not been completed. |
| 41 | `InvalidStatusTransition` | The requested contract status transition is invalid. |
| 42 | `ArbiterRequired` | An arbiter is required for this operation. |
| 43 | `InvalidDisputeSplit` | The dispute split percentage is invalid. |
| 44 | `AccountingInvariantViolated` | The operation would violate the core accounting invariant. |
| 45 | `PotentialOverflow` | Checked arithmetic operation resulted in an overflow. |
| 46 | `AlreadyFinalized` | The contract has already been finalized. |
| 47 | `EvidenceTooLong` | The work evidence string exceeds the maximum length limit. |
| 48 | `TimelockNotElapsed` | The governance admin rotation timelock has not elapsed. |
| 49 | `InvalidProtocolParameters` | The provided protocol parameters are invalid. |