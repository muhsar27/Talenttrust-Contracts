# Escrow Integration Guide

This guide provides a precise, deterministic, and example-driven overview of the TalentTrust Escrow system. It is intended for integrators, auditors, and operators.

## 1. 🔁 Canonical Happy Path (PRIMARY FLOW)

The full lifecycle of a successful escrow contract follows this sequence:

### Step: create
**Function:** `create_contract`  
**Caller:** Client  
**Pre-state:** N/A  
**Post-state:** `Created`  
**Event:** `created { contract_id, client, freelancer, total_amount }`  
**Example:**
```rust
escrow.create_contract(
    &client_addr,
    &freelancer_addr,
    &Some(arbiter_addr),
    &vec![&env, 500_0000000, 500_0000000], // 2 milestones
    &None, // terms_hash
    &Some(3600) // grace_period
);
```

### Step: deposit
**Function:** `deposit_funds`  
**Caller:** Client  
**Pre-state:** `Created`  
**Post-state:** `Funded`  
**Event:** `deposited { contract_id, amount, payer }`  
**Example:**
```rust
escrow.deposit_funds(&contract_id, &1000_0000000);
```

### Step: approve
**Function:** `approve_milestone`  
**Caller:** Client  
**Pre-state:** `Funded`  
**Post-state:** `Funded` (Milestone marked as approved)  
**Event:** `approved { contract_id, milestone_index }`  
**Example:**
```rust
escrow.approve_milestone(&contract_id, &0);
```

### Step: release
**Function:** `release_milestone`  
**Caller:** Client / Arbiter  
**Pre-state:** `Funded` (and approved)  
**Post-state:** `Funded` or `Completed` (if last milestone)  
**Event:** `released { contract_id, milestone_index, amount }`  
**Example:**
```rust
escrow.release_milestone(&contract_id, &0);
```

### Step: complete
**Trigger:** Final `release_milestone` or `refund_unreleased_milestones`  
**Caller:** N/A (Internal transition)  
**Pre-state:** `Funded`  
**Post-state:** `Completed` or `Refunded`  
**Event:** `completed { contract_id }` or `refunded { contract_id, amount }`  

### Step: reputation
**Function:** `issue_reputation`  
**Caller:** Client  
**Pre-state:** `Completed`  
**Post-state:** `Completed` (Reputation credit consumed)  
**Event:** `rated { contract_id, freelancer, rating }`  
**Example:**
```rust
escrow.issue_reputation(&contract_id, &5);
```

---

## 2. 🔐 Authorization Modes

| Function | Authorized Caller(s) | Rejection Behavior |
|----------|----------------------|-------------------|
| `create_contract` | Any (becomes Client) | N/A |
| `deposit_funds` | Client | `UnauthorizedRole` |
| `approve_milestone` | Client | `UnauthorizedRole` |
| `release_milestone` | Client, Arbiter | `UnauthorizedRole` |
| `cancel_contract` | Client, Freelancer, Arbiter | `UnauthorizedRole` (depends on state) |
| `refund_unreleased_milestones` | Arbiter | `UnauthorizedRole` |
| `finalize_contract` | Client | `UnauthorizedRole` |
| `withdraw_leftover` | Client | `UnauthorizedRole` |
| `issue_reputation` | Client | `UnauthorizedRole` |

**Arbiter Override:** The arbiter can call `release_milestone` or `refund_unreleased_milestones` to resolve disputes or unstick funds.

---

## 3. 📣 Event Model

Events are critical for off-chain indexers to track the state of escrow contracts.

| Event Name | Payload Fields | Interpretation |
|------------|----------------|----------------|
| `created` | `contract_id, client, freelancer, total_amount` | New contract initialized in `Created` state. |
| `deposited` | `contract_id, amount, payer` | Funds successfully moved into escrow. |
| `approved` | `contract_id, milestone_index` | Work verified by client. |
| `released` | `contract_id, milestone_index, amount` | Funds moved from escrow to freelancer. |
| `completed` | `contract_id` | All milestones paid; reputation credit available. |
| `refunded` | `contract_id, amount` | Funds returned to client by arbiter. |
| `rated` | `contract_id, freelancer, rating` | Rating recorded; credit consumed. |
| `cancelled` | `contract_id, caller, status, timestamp` | Contract terminated; remaining funds returned. |
| `finalized` | `contract_id` | Contract closed for leftover withdrawals. |
| `withdrawn` | `contract_id, amount, caller` | Leftover funds withdrawn by client. |

*Note: All events include a ledger timestamp for ordering.*

---

## 4. ❌ Failure Modes & Edge Cases

| Scenario | Behavior | Error Returned |
|----------|----------|----------------|
| Double Deposit | Allowed (increments balance) | N/A |
| Double Release | Blocked (milestone already released) | `AlreadyReleased` |
| Unauthorized Release | Blocked (caller is not client/arbiter) | `UnauthorizedRole` |
| Release in `Created` | Blocked (insufficient funds) | `ContractNotFound` (if wrong ID) |
| Release in `Cancelled` | Blocked (terminal state) | `InvalidStatusTransition` |
| Cancellation after Release| Allowed only for unreleased milestones | `MilestonesAlreadyReleased` (for client) |
| Over-funding | Allowed (excess can be withdrawn after finalization) | N/A |

---

## 5. 🔄 Alternative Flows

### A. Cancellation Flow
**Paths:**
1. `Created` → `Cancelled`: Either Client or Freelancer can trigger.
2. `Funded` → `Cancelled`:
   - Client (if zero milestones released)
   - Freelancer (anytime, funds return to client)
   - Arbiter (dispute resolution)
**Funds:** All unreleased funds are returned to the client (accounting updated).

### B. Refund Flow
**Trigger:** `refund_unreleased_milestones` (Arbiter only)  
**Condition:** Contract in `Funded` or `Disputed` state.  
**Effect:** Specified milestones marked as `refunded`; funds marked as refundable to client.

### C. Dispute Flow
**Sequence:** `Funded` → `Disputed` → `Arbiter Decision` → `Release/Refund`  
**Initiation:** Either party calls `dispute_contract`.  
**Arbiter Authority:** In `Disputed` state, the Arbiter has full authority to release or refund milestones.

---

## 6. 🧠 State Machine Summary

| From | To | Trigger |
|------|----|---------|
| `Created` | `Funded` | `deposit_funds` |
| `Created` | `Cancelled` | `cancel_contract` |
| `Funded` | `Completed` | Final `release_milestone` |
| `Funded` | `Disputed` | `dispute_contract` |
| `Funded` | `Cancelled` | `cancel_contract` |
| `Funded` | `Refunded` | Final `refund_unreleased_milestones` |
| `Disputed`| `Completed` | Arbiter `release_milestone` |
| `Disputed`| `Cancelled` | Arbiter `cancel_contract` |

---

## 7. 🔍 Integration Examples

### Full Lifecycle Example (Pseudo-code)
```javascript
// 1. Create
const contractId = await escrow.create_contract(client, freelancer, null, [100, 200]);

// 2. Deposit
await escrow.deposit_funds(contractId, 300);

// 3. Work done... Approve & Release Milestone 1
await escrow.approve_milestone(contractId, 0);
await escrow.release_milestone(contractId, 0);

// 4. Work done... Release Milestone 2 (auto-completes)
await escrow.release_milestone(contractId, 1);

// 5. Issue Reputation
await escrow.issue_reputation(contractId, 5);
```

---

## 8. 🔐 Security Notes

- Only the `protocol_fee_account` can adjust fee rate or withdraw accrued fees.
- Fee account is authenticated with `caller.require_auth()`.
- Fee bounds enforced at 0..=10000.
- All protocol fee operations use persisted state and safe integer arithmetic.

## Behaviour on release

On each milestone release:
- Compute fee: `milestone.amount * protocol_fee_bps / 10000`.
- Save fee to milestone object.
- Increment `protocol_fee_accrued`.
- Mark milestone released and contract status completed when all milestones done.
# Escrow Contract Documentation

**Mainnet readiness (limits, events, risks):** [mainnet-readiness.md](mainnet-readiness.md)

This document summarizes the reviewer-facing architecture for `contracts/escrow`.

## Scope

The contract persists:

- escrow lifecycle state for each contract
- participant metadata for the client and freelancer
- milestone release state
- funded and released accounting
- pending and issued reputation aggregates
- protocol governance parameters
- pause and emergency flags

## Public Flows

Core escrow endpoints:

- `create_contract(client, freelancer, milestone_amounts) -> u32`
- `deposit_funds(contract_id, amount) -> bool`
- `release_milestone(contract_id, milestone_id) -> bool`
- `issue_reputation(contract_id, rating) -> bool`
- `get_contract(contract_id) -> EscrowContractData`
- `get_reputation(freelancer) -> Option<ReputationRecord>`
- `get_pending_reputation_credits(freelancer) -> u32`

Operational controls:

- `initialize(admin) -> bool`
- `pause() -> bool`
- `unpause() -> bool`
- `activate_emergency_pause() -> bool`
- `resolve_emergency() -> bool`
- `is_paused() -> bool`
- `is_emergency() -> bool`

Governance:

- `initialize_protocol_governance(admin, min_milestone_amount, max_milestones, min_reputation_rating, max_reputation_rating) -> bool`
- `update_protocol_parameters(...) -> bool`
- `propose_governance_admin(next_admin) -> bool`
- `accept_governance_admin() -> bool`
- `get_protocol_parameters() -> ProtocolParameters`
- `get_governance_admin() -> Option<Address>`
- `get_pending_governance_admin() -> Option<Address>`

The escrow tests are grouped into dedicated modules:

To prevent out-of-gas or infinite-loop denial of service attacks, the escrow contract enforces creation limits:

- maximum milestone count is capped by `ProtocolParameters.max_milestones` (defaults to 16)
- total escrow amount is bounded by the immutable mainnet cap (`MAINNET_MAX_TOTAL_ESCROW_PER_CONTRACT_STROOPS`)

## Lifecycle Model

Supported lifecycle transitions:

- `Created -> Accepted` after freelancer or arbiter accepts the contract terms
- `Accepted -> Funded` after any positive deposit
- `Funded -> Completed` after the final unreleased milestone is released

Operational invariants:

- client and freelancer addresses are immutable after creation
- milestone amounts are immutable after creation
- each milestone can transition from `released = false` to `released = true` exactly once
- `released_amount` is the sum of released milestone amounts
- `released_milestones` matches the number of released milestone flags
- `reputation_issued` can only become `true` after `Completed`

## Incident Response

### Emergency Response

1. Detect incident and call `activate_emergency_pause`.
2. Investigate and remediate root cause.
3. Validate mitigations in test/staging.
4. Call `resolve_emergency` to restore service.
5. Publish incident summary for ecosystem transparency.

## Persistence Notes

Each `EscrowContractData` record stores:

- participant addresses
- milestone vector and cached milestone count
- total escrow amount
- funded and released balances
- released milestone count
- contract status
- reputation issuance flag
- creation and update timestamps

Detailed storage-key coverage is documented in [state-persistence.md](state-persistence.md).

## Test Coverage

The escrow regression suite is split by concern:

- `flows.rs`: happy-path lifecycle and reputation aggregation
- `lifecycle.rs`: state transition persistence
- `persistence.rs`: storage round-trip assertions
- `security.rs`: failure paths and validation checks
- `governance.rs`: admin and parameter persistence
- `pause_controls.rs` and `emergency_controls.rs`: operational safety controls
- `performance.rs`: resource regression ceilings

## Deterministic Lifecycle Events (v1)

Lifecycle operations now emit a standardized event shape to simplify indexing and alerting.

- Topic tuple: `("escrow", "v1", operation, contract_id)`
- Data tuple: `(status, amount, milestone_index, actor, timestamp)`

Operation values:

- `create`
- `deposit`
- `approve`
- `release`
- `cancel`

Schema notes:

- `status`: post-operation `ContractStatus`
- `amount`: operation amount (or `0` when not applicable)
- `milestone_index`: milestone index (or `0` when not applicable)
- `actor`: `Some(Address)` when a caller identity is relevant, otherwise `None`
- `timestamp`: ledger timestamp at emission

Backwards compatibility:

- Previous ad-hoc topics such as `("contract_cancelled", contract_id)` are replaced by the v1 lifecycle schema.
- Indexers should migrate to the new topic/data tuples for deterministic parsing.
