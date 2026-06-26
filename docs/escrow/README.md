# Escrow Integration Guide

This guide documents the entrypoints currently implemented by the escrow
contract. Planned features are listed separately and linked to their tracking
issues so integrators can distinguish live API from roadmap.

## Module Map

- `contracts/escrow/src/lib.rs`: contract type, shared API surface, reads, controls, cancellation, reputation, and module wiring.
- `contracts/escrow/src/create_contract.rs`: `create_contract` lifecycle entrypoint.
- `contracts/escrow/src/deposit.rs`: `deposit_funds` lifecycle entrypoint.
- `contracts/escrow/src/release.rs`: `release_milestone` lifecycle entrypoint.
- `contracts/escrow/src/refund.rs`: `refund_unreleased_milestones` lifecycle entrypoint.

## Implemented API Surface

Lifecycle and reputation:

- `create_contract(client, freelancer, milestone_amounts, deposit_mode) -> u32`
- `deposit_funds(contract_id, amount) -> bool`
- `release_milestone(contract_id, caller, milestone_index) -> bool`
- `release_milestones(contract_id, caller, milestone_indices) -> i128`
- `issue_reputation(contract_id, caller, freelancer, rating) -> bool`
- `cancel_contract(contract_id, caller) -> bool`
- `finalize_contract(contract_id, finalizer) -> bool`

Read-only queries:

- `get_contract(contract_id) -> EscrowContractData`
- `get_milestones(contract_id) -> Vec<Milestone>`
- `get_refundable_balance(contract_id) -> i128`
- `get_milestone_approvals(contract_id, milestone_index) -> Option<MilestoneApprovals>`
- `get_finalization_record(contract_id) -> Option<FinalizationRecord>`
- `get_reputation(freelancer) -> Option<ReputationRecord>`
- `get_average_rating(freelancer) -> Option<i128>` — scaled average (see [Average Rating](#average-rating))
- `get_pending_reputation_credits(freelancer) -> u32`
- `get_admin() -> Option<Address>`
- `is_paused() -> bool`
- `is_emergency() -> bool`
- `get_mainnet_readiness_info() -> MainnetReadinessInfo`
- `get_protocol_fee_bps() -> u32`
- `get_accumulated_protocol_fees() -> i128`

### Read-only getter semantics

The read getters below are stable, side-effect-free paths that indexers and
off-chain callers rely on. They share three properties:

1. **Not-found**: every getter that takes a `contract_id` panics the contract
   with `ContractNotFound` when `contract_id` was never allocated. The
   Soroban-generated `try_*` wrappers surface this as
   `Err(Ok(ContractNotFound))` for off-chain callers and do not mutate any
   other persistent state.
2. **Pure read**: invoking any of these getters on a valid `contract_id` does
   not mutate balances, status, milestones, or per-milestone flags.
   Accounting-only fields (`funded_amount`, `released_amount`,
   `refunded_amount`) and per-milestone `released`/`refunded` flags are
   bitwise-stable across arbitrary numbers of repeated calls.
3. **TTL on read (persistent only)**: on a successful read the contract
   extends the persistent TTL of the entry being read from (`Contract(id)`,
   `(Contract(id), "milestones")`) to `PERSISTENT_TTL_LEDGERS` (30 days).
   This keeps idle but live contracts in storage without rebuilding them. The
   `get_milestone_approvals` getter reads from temporary storage and is
   therefore exempt from this rule; it is governed by
   `PENDING_APPROVAL_TTL_LEDGERS` and the host's auto-eviction.

Per-getter details:

- `get_contract(contract_id)` returns the full `EscrowContractData`
  (participants, arbiter, status, funded/released/refunded amounts,
  release_authorization). Reads persist the contract entry's TTL. Panics
  `ContractNotFound` for an unknown id.
- `get_milestones(contract_id)` returns the milestones vector in creation
  order. Reads persist the milestones entry's TTL. Panics `ContractNotFound`
  for an unknown id.
- `get_refundable_balance(contract_id)` returns
  `funded_amount - released_amount - refunded_amount`. The result must be
  non-negative by construction; panic-on-overflow is enforced on
  contributing arithmetic at every mutating entrypoint. Reads persist the
  contract entry's TTL. Panics `ContractNotFound` for an unknown id.
- `get_milestone_approvals(contract_id, milestone_index)` returns `Some`
  only if a non-expired approval record for that milestone exists in
  temporary storage. Returns `None` when no approval has been recorded or
  when the contract id is unknown. Does not extend persistent TTL because
  approvals live in temporary storage bounded by
  `PENDING_APPROVAL_TTL_LEDGERS`.

These properties are locked in by tests under
`contracts/escrow/src/test/persistence.rs` (issue #475).

Operational controls:

- `initialize(admin) -> bool`
- `pause() -> bool`
- `unpause() -> bool`
- `activate_emergency_pause() -> bool`
- `resolve_emergency() -> bool`

Governance admin transfer (two-step):

- `propose_governance_admin(proposed) -> bool`
- `accept_governance_admin() -> bool`
- `cancel_governance_admin_proposal() -> bool`
- `get_governance_admin() -> Option<Address>`
- `get_pending_governance_admin() -> Option<Address>`

## Canonical Happy Path

### 1. Initialize Operational Admin

```rust
escrow.initialize(&admin);
```

`initialize` is single-use, requires `admin.require_auth()`, and stores the
admin used by pause and emergency controls.

### 2. Create Contract

```rust
let contract_id = escrow.create_contract(
    &client_addr,
    &freelancer_addr,
    &vec![&env, 500_0000000_i128, 500_0000000_i128],
    &DepositMode::ExactTotal,
);
```

Creation requires `client.require_auth()`, rejects identical client/freelancer
addresses, rejects empty or non-positive milestones, caps milestone count at
`MAX_MILESTONES` (rejecting with `TooManyMilestones`), and caps total escrow value
against governed `max_escrow_total_stroops` (rejecting with `TotalCapExceeded`).

### 3. Deposit Funds

```rust
escrow.deposit_funds(&contract_id, &1000_0000000_i128);
```

`deposit_funds` accepts deposits while the contract is in
`Created` or `PartiallyFunded` status:

- `Created -> PartiallyFunded` when `0 < funded_amount < total_amount` (any
  partial installment).
- `Created | PartiallyFunded -> Funded` once `funded_amount >= total_amount`
  (either a single full deposit or the final-installment topping-up).
- Any other status (Completed, Refunded, Cancelled, ...) is rejected with
  `InvalidState` so post-resolution contracts cannot be re-funded.
- `AmountMustBePositive` rejects zero or negative deposits (preserves the
  dust-attack guard).
- `InvalidDepositAmount` rejects any deposit whose addition would push
  `funded_amount` past `total_amount` AND any deposit that would overflow
  `i128`. Both checks happen **before** any state mutation.
- `UnauthorizedRole` rejects any caller that is not the stored client.

Positivity, status gate, milestone total, overflow check, and over-funding
check are all evaluated **before** writing to storage. The status gate runs
before caller-auth so a non-authenticated caller against a
`Funded`/`Completed`/`Cancelled` contract does not reveal the existence of
a valid funded slot beyond the public read path. Each successful deposit
emits a `(deposited, contract_id)` event with
`(caller, amount, funded_amount, total_amount, status)` so indexers can
distinguish partial installments from a final-funding deposit.

`deposit_funds` extends the persistent TTL of the contract and milestones
entries on every read and write path so installment schedules do not expire
the contract between deposits.

### 4. Release Milestones

#### Single Milestone Release
```rust
escrow.release_milestone(&contract_id, &client_addr, &0);
```

#### Batch Milestone Release
```rust
let milestone_indices = vec![&env, 0_u32, 1_u32];
let total_released = escrow.release_milestones(&contract_id, &client_addr, &milestone_indices);
```

Releases multiple milestones atomically. Validates all indices, approvals, and
available balance before mutating state. Fails closed if any validation fails,
ensuring no partial releases.

Requires valid, non-expired approvals based on the contract's ReleaseAuthorization mode.
When the final milestone is released, status becomes `Completed` and one pending
reputation credit is added for the freelancer.

`PendingReputationCredits` is a non-negative counter that tracks completed
contracts awaiting client-issued reputation for a freelancer. `issue_reputation`
consumes one pending credit and records the rating.

### 5. Issue Reputation

```rust
escrow.issue_reputation(&contract_id, &client_addr, &freelancer_addr, &5_i128);
```

Reputation requires `caller.require_auth()`, the caller must be the stored
client, the freelancer argument must match the contract freelancer, the contract
must be `Completed`, rating must be `1..=5`, and each contract can issue
reputation once.

## Average Rating

`get_average_rating(freelancer) -> Option<i128>` returns the freelancer's
average rating scaled to **basis points (×10 000)**, or `None` if no reputation
record exists or `completed_contracts == 0`.

Formula:

```
result = total_rating * 10_000 / completed_contracts
```

To convert back to the 1–5 decimal scale, divide by `10_000`:

| `total_rating` | `completed_contracts` | `get_average_rating` | Decimal |
|---|---|---|---|
| 5 | 1 | 50 000 | 5.0000 |
| 8 | 2 | 40 000 | 4.0000 |
| 3 | 2 | 15 000 | 1.5000 |

Checked arithmetic is used; overflow and division-by-zero cannot occur.

## Cancellation

```rust
escrow.cancel_contract(&contract_id, &caller);
```

Cancellation requires `caller.require_auth()`. The caller must be the stored
client or freelancer. It is blocked after `Completed` and blocked if the
contract is already `Cancelled`.

## Finalization

```rust
escrow.finalize_contract(&contract_id, &finalizer);
```

Finalization requires `finalizer.require_auth()`. The finalizer must be the
stored client, freelancer, or assigned arbiter. It is allowed only while the
contract status is `Completed` or `Disputed`.

The contract writes one immutable `FinalizationRecord` containing the finalizer,
ledger timestamp, and a `ContractSummary` snapshot. After the record exists,
contract-specific mutating calls reject with `AlreadyFinalized`.

## Two-Step Governance Admin Transfer

The admin transfer uses a propose-accept (two-step) pattern:

```rust
// Step 1: current admin proposes next admin
escrow.propose_governance_admin(&next_admin);

// Step 2: next admin accepts (requires next_admin.require_auth())
escrow.accept_governance_admin();
```

### Rules
- **Self-proposal is rejected**: proposing the current admin as the new admin
  panics with `CannotProposeSelf`.
- **Re-proposing overwrites**: calling `propose_governance_admin` while a
  pending proposal exists silently replaces it (no explicit cancellation
  required).
- **Cancellation**: `cancel_governance_admin_proposal` is admin-gated (only
  the current admin may cancel). It clears the pending admin and emits a
  `("admin", "cancelled")` event.
- **No stale acceptance**: accepting after cancellation panics with
  `InvalidState` because the pending proposal has been removed.
- All operations require the contract to be initialized.

### Events
| Topic | Data | Trigger |
|---|---|---|
| `("admin", "proposed")` | `(admin, proposed, timestamp)` | `propose_governance_admin` |
| `("admin", "accepted")` | `(old_admin, new_admin, timestamp)` | `accept_governance_admin` |
| `("admin", "cancelled")` | `(admin, cancelled_proposal, timestamp)` | `cancel_governance_admin_proposal` |

## Pause and Emergency Controls

`pause`, `unpause`, `activate_emergency_pause`, and `resolve_emergency` require
the stored admin's authorization. While paused or in emergency, mutating
lifecycle calls fail with `ContractPaused`; read-only queries remain available.
`unpause` fails while emergency mode is active.

## Events

Implemented events:

- `("init", "admin_set")` on `initialize`
- `("paused", timestamp)` on `pause`
- `("unpaused", timestamp)` on `unpause`
- `("emergency", "activated")` and `("emergency", "resolved")`
- `("audit", contract_id)` for lifecycle state transitions
- `("created", contract_id)` on contract creation
- `("deposited", contract_id)` on every successful `deposit_funds` call,
  carrying `(caller, amount, funded_amount, total_amount, status)` so indexers
  can distinguish partial installments from a final-funding deposit (issue #441).
- `("released", contract_id, milestone_index)` on release
- `("rep_issd", contract_id)` on reputation issuance
- `("cancelled", contract_id)` on cancellation
- `("finalized", contract_id)` on finalization

Fee events and any remaining structured-deposit events continue tracking in
[#336](https://github.com/Talenttrust/Talenttrust-Contracts/issues/336); the
deposit event lane is now closed by issue #441.

## Implemented Security Assumptions

- Creation and reputation issue require explicit address authentication.
- Pause and emergency controls are admin-authenticated.
- Deposits cannot exceed the exact milestone total. Over-funding is rejected
  with `InvalidDepositAmount` **before** any state mutation (issue #441).
- Deposits in installments transition the contract through `Created →
  PartiallyFunded → Funded`; partial installments are observable to indexers
  via the `deposited` event (issue #441).
- Releases fail on duplicate milestone release, invalid milestone id, missing
  contract, paused state, and insufficient funded balance.
- Arithmetic for escrow totals, deposits, and releases uses checked helpers and
  panics with `InvalidDepositAmount` on overflow or over-funding. Other checked
  paths may surface `PotentialOverflow` from `amount_validation.rs`.
- Accounting is checked after balance-changing operations.
- The contract stores accounting state only; token custody and token transfers
  are not implemented in `lib.rs` and must be handled by an audited integration.
- Storage uses persistent keys. TTL constants exist for planned pending approval
  and migration flows, but no current public entrypoint writes those pending
  records.

## Refund Accounting Invariant

`get_refundable_balance` is the security-critical query that answers "how much
of the escrowed funds can still be returned or released?". Its definition is:

```
refundable_balance = funded_amount - released_amount - refunded_amount
```

This identity must hold at every point in the contract's lifecycle. Violating
it would mean the contract could either over-pay (release or refund more than
was funded) or leave funds permanently locked.

### Properties the invariant guarantees

| Property | Explanation |
|---|---|
| Non-negative | `refundable_balance >= 0` at all times — the contract never becomes insolvent. |
| Zero iff all milestones terminal | Balance reaches `0` only when every milestone is in `released` or `refunded` state. |
| Additive decomposition | `funded_amount == released_amount + refunded_amount + refundable_balance` always. |
| Status consistency | When balance is `0` and any milestone was released, status is `Completed`; when all were refunded, status is `Refunded`. |

### When balance changes

- `deposit_funds` increases `funded_amount`, so balance increases.
- `release_milestone` increases `released_amount` by the milestone amount, so balance decreases.
- `refund_unreleased_milestones` increases `refunded_amount` by the sum of the refunded milestones, so balance decreases.
- No other entrypoint mutates these three fields.

### Test coverage

`contracts/escrow/src/test/refund.rs` contains a dedicated suite of accounting
invariant tests (`assert_balance_invariant`) that verify this property after
every operation order:

- balance equals `funded_amount` before any release or refund
- release-then-refund sequence
- refund-then-release sequence
- all milestones released → balance zero, status `Completed`
- all milestones refunded → balance zero, status `Refunded`
- interleaved alternating operations at every step
- cross-check that `get_refundable_balance` matches `get_contract` fields exactly
- partial deposit with partial refund never goes negative

## Planned Features

These features are not implemented entrypoints today:

- Protocol fee deduction on release: planned in
  [#313](https://github.com/Talenttrust/Talenttrust-Contracts/issues/313).
- Protocol fee treasury withdrawal: planned in
  [#314](https://github.com/Talenttrust/Talenttrust-Contracts/issues/314).
- Governed parameter setter/readiness wiring: planned in
  [#323](https://github.com/Talenttrust/Talenttrust-Contracts/issues/323).
- Structured deposit and fee events: planned in
  [#336](https://github.com/Talenttrust/Talenttrust-Contracts/issues/336).
- Storage-key reference for declared-but-unused keys, including pending client
  migration and protocol fee keys: planned in
  [#342](https://github.com/Talenttrust/Talenttrust-Contracts/issues/342).
- `migrate_state` / `StateV1` / `StateV2` migration flow: not implemented;
  tracked by this reconciliation issue
  [#341](https://github.com/Talenttrust/Talenttrust-Contracts/issues/341)
  until a dedicated implementation issue exists.

Any documentation that describes one of these items as available should be
treated as roadmap text, not live integration guidance.


### Milestone Approval and Revocation

Participants can approve milestone items prior to fund distribution payouts. If an authorization mistake is discovered prior to complete disbursement release configurations, the approving party can rescind authority.

#### `revoke_approval(contract_id: Address, caller: Address, milestone_index: u32)`
- **Authorization Required:** `caller.require_auth()`
- **Behavior:** Explicitly removes individual state flags (`client_approved` | `freelancer_approved` | `arbiter_approved`). When all structural components drop to `false`, temporary records are scrubbed entirely to maximize gas savings.
- **Errors raised:** `Error::MilestoneAlreadyReleased`, `Error::ApprovalRecordNotFound`.