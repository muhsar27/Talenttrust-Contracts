# Escrow Contract

Rust/Soroban escrow contract for TalentTrust freelancer milestones.

## Implemented Features

- Create a contract between a client and a freelancer.
- Define milestone amounts at creation time.
- Track exact-total or incremental deposits in contract state.
- Release milestones from funded balance.
- Mark the contract `Completed` after the final milestone release.
- Issue one reputation rating for the freelancer after completion.
- Cancel non-completed contracts by the stored client or freelancer.
- Finalize completed or disputed contracts with immutable close metadata.
- Pause and emergency controls managed by a single initialized admin.

## Current Public Entrypoints

- `initialize(admin) -> bool`
- `get_admin() -> Option<Address>`
- `pause() -> bool`
- `unpause() -> bool`
- `is_paused() -> bool`
- `activate_emergency_pause() -> bool`
- `resolve_emergency() -> bool`
- `is_emergency() -> bool`
- `get_mainnet_readiness_info() -> MainnetReadinessInfo`
- `create_contract(client, freelancer, milestone_amounts, deposit_mode) -> u32`
- `deposit_funds(contract_id, amount) -> bool`
- `release_milestone(contract_id, milestone_index) -> bool`
- `issue_reputation(contract_id, caller, freelancer, rating) -> bool`
- `cancel_contract(contract_id, caller) -> bool`
- `finalize_contract(contract_id, finalizer) -> bool`
- `get_contract(contract_id) -> EscrowContractData`
- `get_finalization_record(contract_id) -> Option<FinalizationRecord>`
- `get_reputation(freelancer) -> Option<ReputationRecord>`
- `get_pending_reputation_credits(freelancer) -> u32`

### Protocol Fee Read API

- `get_protocol_fee_bps() -> u32` — Returns the current protocol fee rate in basis points (0–10 000).  
  Written by `set_protocol_fee_bps`. No authentication required. Returns `0` when unset. Bumps persistent TTL on access.

- `get_accumulated_protocol_fees() -> i128` — Returns total protocol fees accumulated across all released milestones, in stroops.  
  No authentication required. Returns `0` when no fees have been accumulated. Bumps persistent TTL on access.

These entrypoints let off-chain dashboards and indexers read the current fee configuration and accrued revenue without scraping raw ledger entries.

## Important Integration Notes

- `release_milestone` currently validates milestone state and available balance,
  but it does not authenticate a client or arbiter caller.
- The contract tracks escrow balances in state only. Token custody and transfers
  are not implemented in `contracts/escrow/src/lib.rs`.
- `withdraw_leftover`, protocol fee accounting, protocol
  fee withdrawal, two-step admin transfer, dispute/refund flows, and
  `migrate_state` are not live entrypoints.

## Events

All events follow the Soroban `env.events().publish(topics, data)` pattern.
Topics are `symbol_short!` values (≤ 8 chars) so they are cheap to index.

| Event topic[0]     | topic[1]      | Data fields (in order)                                                                                         | When emitted                                     |
|--------------------|---------------|----------------------------------------------------------------------------------------------------------------|--------------------------------------------------|
| `mlstn_rls`        | `contract_id` | `(milestone_index: u32, amount: i128, fee: i128, new_released_amount: i128, caller: Address, timestamp: u64)` | Every successful `release_milestone` call        |
| `ctrct_cmp`       | `contract_id` | `(caller: Address, timestamp: u64)`                                                                            | When a release transitions status to `Completed` |
| `created`          | `contract_id` | `(client: Address, freelancer: Address, timestamp: u64)`                                                       | `create_contract`                                |
| `evidence`         | `contract_id` | `(milestone_index: u32, freelancer: Address, timestamp: u64)`                                                  | `submit_work_evidence`                           |
| `pause`            | `timestamp`   | `(admin: Address,)`                                                                                            | `pause`                                          |
| `unpaused`         | `timestamp`   | `(admin: Address,)`                                                                                            | `unpause`                                        |
| `emergency`        | `activated`   | `(admin: Address, timestamp: u64)`                                                                             | `activate_emergency_pause`                       |
| `emergency`        | `resolved`    | `(admin: Address, timestamp: u64)`                                                                             | `resolve_emergency`                              |
| `dispute`          | `opened`      | `(contract_id: u32, caller: Address)`                                                                          | `raise_dispute`                                  |
| `dispute`          | `resolved`    | `(contract_id: u32, resolution_code: u32)`                                                                     | `resolve_dispute`                                |
| `admin`            | `proposed`    | `(old_admin: Address, proposed: Address, timestamp: u64)`                                                      | `propose_governance_admin`                       |
| `admin`            | `accepted`    | `(old_admin: Address, new_admin: Address, timestamp: u64)`                                                     | `accept_governance_admin`                        |
| `protocol_fee_bps` | —             | `(old_bps: u32, new_bps: u32, admin: Address, timestamp: u64)`                                                 | `set_protocol_fee_bps`                           |
| `init`             | `admin_set`   | `(admin: Address, timestamp: u64)`                                                                             | `initialize`                                     |

### `mlstn_rls` — Milestone Released

```
topics : (symbol_short!("mlstn_rls"), contract_id: u32)
data   : (
    milestone_index     : u32,     // zero-based index of the released milestone
    amount              : i128,    // gross milestone amount in stroops
    fee                 : i128,    // protocol fee deducted (0 when fee_bps == 0)
    new_released_amount : i128,    // cumulative released_amount after this release
    caller              : Address, // authorized caller who triggered the release
    timestamp           : u64,     // ledger timestamp at execution
)
```

### `ctrct_cmp` — Contract Completed

Emitted in the same transaction as the final `mlstn_rls` event, immediately after it.

```
topics : (symbol_short!("ctrct_cmp"), contract_id: u32)
data   : (
    caller    : Address, // caller who triggered the completing release
    timestamp : u64,     // ledger timestamp at execution
)
```

**Security properties**

- Both events are emitted **only after** all state mutations succeed — there is no observable event on any error path.
- Events contain no secret data: all fields are already public contract state or caller-supplied arguments that are on-chain by definition.
- `fee` is always `≥ 0`; it is `0` when `protocol_fee_bps` is unset or zero.

## Deposit Promotion Property

`deposit_funds` accumulates deposits and promotes the contract from `Created`
to `Funded` as soon as `funded_amount >= sum(milestones)`. The property suite
in `contracts/escrow/src/proptest.rs` verifies this invariant exhaustively:

| Property | What it asserts |
|---|---|
| `prop_deposit_promotion_boundary_across_splits` | After each split-deposit: `status == Created` iff `funded < total`, `status == Funded` iff `funded >= total`. Once `Funded`, all further deposits are rejected. |
| `prop_exact_total_single_deposit_promotes_to_funded` | A single deposit of exactly `total` always promotes the contract. |
| `prop_underfunded_deposits_never_promote` | Any sequence of deposits whose sum stays strictly below `total` never triggers promotion. |
| `prop_overshoot_deposit_promotes_to_funded` | A deposit that exceeds the remaining gap promotes the contract; the overshoot amount is stored as-is in `funded_amount`. |
| `prop_funded_amount_equals_cumulative_deposits` | `funded_amount` equals the exact running sum of all accepted deposits — no stroop created or lost. |
| `prop_promotion_is_order_independent` | The same set of chunks arrives at `Funded` with the same `funded_amount` regardless of deposit order (forward vs reversed). |

**Security guarantee**: no deposit ordering or split can cause premature promotion
(funded < total → Funded) or missed promotion (funded >= total → still Created).

## Planned Features

- Two-step admin transfer:
  [#318](https://github.com/Talenttrust/Talenttrust-Contracts/issues/318)
- Protocol fee deduction and withdrawal:
  [#313](https://github.com/Talenttrust/Talenttrust-Contracts/issues/313),
  [#314](https://github.com/Talenttrust/Talenttrust-Contracts/issues/314)
- Final contract closure metadata:
  [#320](https://github.com/Talenttrust/Talenttrust-Contracts/issues/320)
- `migrate_state` / `StateV1` / `StateV2` flow:
  [#341](https://github.com/Talenttrust/Talenttrust-Contracts/issues/341)
