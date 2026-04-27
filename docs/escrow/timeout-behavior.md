# Escrow Timeout Behavior

This document describes the deadline policy enforced by `contracts/escrow`.

## Scope

Timeout enforcement applies to milestone approval, milestone release, and
timeout-triggered dispute resolution.

## Deadline source

1. Every milestone carries `deadline_at: Option<u64>` on-chain.
2. `deadline_at` is derived from milestone schedule metadata:
   - `create_contract(..., schedules)` seeds it from `MilestoneSchedule.due_date`
   - `set_milestone_schedule(...)` updates both the stored schedule record and the
     milestone's runtime deadline field
3. All comparisons use `env.ledger().timestamp()` so the behavior is fully deterministic.

## Boundary rules

- `timestamp <= deadline_at`: the milestone is still actionable
- `timestamp > deadline_at`: the milestone is expired
- milestones with `deadline_at = None` do not time out automatically

## Timeout transitions

When an unreleased milestone is expired and the contract is still `Funded`:

1. `approve_milestone_release` panics and transitions the contract to `Disputed`
2. `release_milestone` panics and transitions the contract to `Disputed`
3. `evaluate_milestone_timeout` returns `true` and transitions the contract to `Disputed`

This transition is single-source and deterministic: it is based only on current
ledger time and the stored milestone deadline.

## Dispute resolver policy

Timeout disputes can be resolved only after all unreleased milestones are no
longer expired.

- If the contract has an arbiter, only the arbiter may call `resolve_dispute`
- If the contract has no arbiter, the client is the resolver of last resort

In practice this means the client can first update the milestone schedule to a
future `due_date`, after which the designated resolver can move the contract
from `Disputed` back to `Funded`.

## Security notes

- Deterministic ledger time prevents caller-controlled deadline checks
- Inclusive deadline handling avoids ambiguity at the exact deadline boundary
- Auto-dispute blocks stale approvals or releases from quietly succeeding
- Resolver gating ensures dispute exits are explicit and role-bound

## Test coverage

`contracts/escrow/src/test/timeout_tests.rs` covers:

- exact-deadline approval success
- post-deadline approval failure
- explicit timeout evaluation causing `Funded -> Disputed`
- post-deadline release failure after a valid approval
- arbiter-driven dispute resolution after deadline extension
- client fallback resolution when no arbiter exists
