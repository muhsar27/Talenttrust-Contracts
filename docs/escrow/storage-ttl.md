# Storage TTL / Expiration Policy

This document defines the deterministic, auditable TTL (time-to-live) policy for **transient** storage entries in the escrow contract. It exists to prevent unbounded state growth from orphaned pending approvals and pending migrations that are never resolved by counterparties.

See also: [state-persistence.md](./state-persistence.md) for the persistent storage model; [upgradeable-storage.md](./upgradeable-storage.md) for upgrade semantics.

## Scope

Applies to keys stored in `env.storage().temporary()`. Persistent keys (e.g. `Contract(id)`, `NextId`) are unaffected — their TTL management is covered in [architecture.md](./architecture.md).

## Units

All TTL values are denominated in **ledgers**, the Soroban-native unit. One ledger is ~5 seconds on Stellar mainnet. This avoids any coupling to wall-clock timestamps and keeps expiry deterministic as a function of `env.ledger().sequence()`.

| Named constant | Ledgers | Rough duration |
| --- | ---: | --- |
| `LEDGERS_PER_DAY` | 17 280 | 1 day |
| `PENDING_APPROVAL_TTL_LEDGERS` | 120 960 | 7 days |
| `PENDING_APPROVAL_BUMP_THRESHOLD` | 17 280 | 1 day |
| `PENDING_MIGRATION_TTL_LEDGERS` | 362 880 | 21 days |
| `PENDING_MIGRATION_BUMP_THRESHOLD` | 51 840 | 3 days |

Constants live in [contracts/escrow/src/ttl.rs](../../contracts/escrow/src/ttl.rs).

## Transient Keys

| Key | Value type | TTL | Bump threshold | Rationale |
| --- | --- | ---: | ---: | --- |
| `PendingApproval(contract_id: u32)` | `PendingApproval` | 7 days | 1 day | Counterparties are expected to respond within one business week; short enough to reclaim state on abandonment, long enough to tolerate holidays. |
| `PendingMigration` | `PendingMigration` | 21 days | 3 days | Migrations are rarer and more consequential; reviewers need more lead time and explicit bump windows. |

`PendingMigration` is a **single-slot** key: at most one migration may be pending at any time, which is enforced by `PendingMigrationExists` (error code 8).

## Value Schema

Each transient value stores its own expiry metadata alongside Soroban's internal TTL. This is intentional redundancy so that on-chain readers and event indexers can audit expiry independently of Soroban's TTL ledger metadata.

```rust
pub struct PendingApproval {
    pub approver: Address,
    pub contract_id: u32,
    pub requested_at_ledger: u32,
    pub expires_at_ledger: u32,
}

pub struct PendingMigration {
    pub proposer: Address,
    pub new_wasm_hash: BytesN<32>,
    pub requested_at_ledger: u32,
    pub expires_at_ledger: u32,
}
```

## Determinism

Expiry is computed at write time as:

```
expires_at_ledger = requested_at_ledger + TTL
                  = env.ledger().sequence() + TTL
```

Given the same ledger sequence and the same TTL constant, two runs produce identical `expires_at_ledger` values. Covered by test `deterministic_expiry` in [contracts/escrow/src/test/ttl_tests.rs](../../contracts/escrow/src/test/ttl_tests.rs).

## Expiry Semantics

- Soroban auto-evicts temporary storage entries once their TTL has elapsed.
- `read_if_live` (used by `get_pending_approval` / `get_pending_migration`) returns `Option<_>`; after expiry it returns `None`.
- The contract **does not distinguish** "never set" from "expired on read". Consumers must treat `None` as "no active pending record" in both cases.
- No on-chain event is emitted at the moment of auto-eviction — Soroban does not expose an eviction hook. Off-chain indexers should compute eviction by comparing the stored `expires_at_ledger` against the current ledger sequence.

## Extending (Bumping) TTL

`extend_pending_approval` / `extend_pending_migration` wrap `env.storage().temporary().extend_ttl(key, threshold, extend_to)`:

- If remaining TTL is **below** the bump threshold, the entry's TTL is extended to the full policy value.
- If the entry is already fresh, the call is a no-op.
- If the entry is absent or already evicted, the helper returns `false` and performs no write.

Callers must still be authorised (`approver.require_auth()` / `proposer.require_auth()`).

## Events (Audit Trail)

All state-changing TTL operations publish a structured event:

| Topic 0 | Topic 1 | Data tuple |
| --- | --- | --- |
| `ttl` | `requested` | `(subject, identifier..., actor, requested_at_ledger, expires_at_ledger)` |
| `ttl` | `cancelled` | `(subject, identifier..., actor)` |
| `ttl` | `confirmed` | `(subject, identifier..., actor)` |

`subject` is `approval` or `migration`. For approvals, `identifier` is the `contract_id`; for migrations, `identifier` is the `new_wasm_hash` (on request) or absent (on cancel).

Auto-eviction emits no event. See *Expiry Semantics* above.

## Error Codes

| Variant | Code | Meaning |
| --- | ---: | --- |
| `PendingApprovalExists` | 6 | An approval is already pending for this contract. |
| `PendingApprovalNotFound` | 7 | No live approval to cancel. |
| `PendingMigrationExists` | 8 | A migration is already pending. |
| `PendingMigrationNotFound` | 9 | No live migration to cancel or confirm. |
| `Unauthorized` | 10 | Caller is not the original requester. |

## Testing

Expiry is exercised by advancing `LedgerInfo.sequence_number` via `env.ledger().with_mut(...)`. See [contracts/escrow/src/test/ttl_tests.rs](../../contracts/escrow/src/test/ttl_tests.rs) for the full matrix:

- `pending_{approval,migration}_readable_before_expiry`
- `pending_{approval,migration}_evicted_after_expiry`
- `extend_if_below_threshold_bumps_when_near_expiry`
- `extend_if_below_threshold_noop_when_fresh`
- `extend_returns_false_when_key_absent`
- `deterministic_expiry`
- `cancel_removes_pending_approval`
- `duplicate_request_{approval,migration}_rejects`
- `confirm_migration_clears_pending`

## Reviewer Checklist

1. Every new transient key has an entry in the table above.
2. Every write uses `ttl::store_with_ttl` (no direct `.temporary().set` bypass).
3. Every read path uses `ttl::read_if_live` and handles `None` as "absent or expired".
4. Expiry metadata on the value matches the constant applied at write time.
5. A corresponding TTL test exists when a new transient key is introduced.
