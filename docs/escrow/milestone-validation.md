# Milestone Validation

Describes every input guard enforced by `create_contract` and where each is tested.

## Guards (in execution order)

| # | Condition | Error | Test |
|---|-----------|-------|------|
| 1 | `client == freelancer` | `InvalidParticipant` | `rejects_same_client_and_freelancer` |
| 2 | `milestone_amounts.is_empty()` | `EmptyMilestones` | `rejects_empty_milestones` |
| 3 | `len > MAX_MILESTONES` (10) | `TooManyMilestones` | `rejects_one_over_max_milestones` |
| 4 | `len == MAX_MILESTONES` | *(success)* | `accepts_exactly_max_milestones` |
| 5 | any `amount <= 0` | `InvalidMilestoneAmount` | `rejects_zero_milestone_amount`, `rejects_negative_milestone_amount` |
| 6 | `safe_add_amounts` overflow | `PotentialOverflow` | `rejects_amounts_that_would_overflow_i128` |
| 7 | `total > MAX_TOTAL_ESCROW_STROOPS` | `InvalidMilestoneAmount` | `rejects_total_one_over_cap`, `rejects_multi_milestone_total_over_cap` |

## Constants

- `MAX_MILESTONES = 10` — hard cap on milestone count per contract
- `MAX_TOTAL_ESCROW_STROOPS = 10_000_000_000_000` — 1 000 000 XLM in stroops

## Overflow safety

Amounts are accumulated with `safe_add_amounts`, which wraps `i128::checked_add`. If the running total would overflow `i128`, the call returns `None` and the contract panics with `PotentialOverflow` before the cap comparison is ever reached. This means `i128::MAX` inputs and near-overflow pairs are caught cleanly without silent wrapping.

## Guard ordering

Count is checked before amounts. When a caller passes more than 10 milestones *and* a total above the cap, `TooManyMilestones` is returned — verified by `count_guard_fires_before_amount_guard`.

## Tests

All guards are covered in `contracts/escrow/src/test/create_contract_bounds.rs`, wired via `mod.rs`.
