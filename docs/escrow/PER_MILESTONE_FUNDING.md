# Per-Milestone Funding and Refund Accounting

Each `Milestone` struct carries its own `funded_amount` and `refunded_amount`
fields so that indexers and dispute logic can read a milestone's financial state
without cross-referencing contract-level aggregates.

## How per-milestone amounts are populated

### `deposit_funds` — distributing deposits

When a client deposits funds, the deposited amount is distributed **in milestone
order** (earliest to latest). For each milestone whose `funded_amount` is less
than its `amount`, the remaining need is filled from the deposit:

```
remaining = amount
for each milestone in order:
    if milestone.funded_amount < milestone.amount:
        to_add = min(remaining, milestone.amount - milestone.funded_amount)
        milestone.funded_amount += to_add
        remaining -= to_add
    if remaining == 0: break
```

This ensures that `sum(milestones.funded_amount) == contract.funded_amount`
at all times.

### `release_milestone` — recording coverage on release

When a milestone is released, its `funded_amount` is also set to its `amount`.
The deposit path should have already distributed enough to cover this milestone,
but the release path sets it as a safety guarantee.

### `refund_unreleased_milestones` — recording refunded amounts

When an unreleased milestone is refunded, its `refunded_amount` is set to its
`amount`. Multiple milestones can be refunded in a single call.

This guarantees that `sum(milestones.refunded_amount) == contract.refunded_amount`
at all times.

## Guaranteed invariants

1. `sum(milestones.funded_amount) == contract.funded_amount`
2. `sum(milestones.refunded_amount) == contract.refunded_amount`
3. `sum(milestones where released == true).funded_amount == contract.released_amount`
4. Checked arithmetic (`.checked_add` / `.checked_sub`) is used for all
   per-milestone amount updates, so overflow panics safely.

## Indexer usage

Indexers can now directly read individual milestones from `get_milestones`
and obtain each milestone's financial contribution without needing to compute
it from contract-level aggregates:

| Field             | Meaning                                          |
|-------------------|--------------------------------------------------|
| `funded_amount`   | How much of this milestone's `amount` is covered |
| `refunded_amount` | Set to `amount` if refunded, otherwise 0         |
| `released`        | Whether the milestone was paid out               |
| `refunded`        | Whether the milestone was refunded               |
