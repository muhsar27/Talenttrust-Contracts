# Funding Accounting Invariants

The live escrow contract tracks balances in `EscrowContractData`; SAC token
custody is managed via `bind_settlement_token` and real `token::Client::transfer`
calls at deposit, release, and fee-withdrawal time.

## Implemented Invariants

- `amount > 0` for every deposit.
- Every milestone amount must be positive at creation time.
- Total milestone value must not exceed `MAX_TOTAL_ESCROW_STROOPS`.
- `ExactTotal` deposits must equal the full milestone sum and can happen only
  once.
- `Incremental` deposits can accumulate up to, but not beyond, the milestone
  sum.
- `release_milestone` requires enough available balance:
  `total_deposited - released_amount - refunded_amount >= milestone_amount`.
- Released milestones are recorded under `MilestoneReleased(contract_id, index)`
  and cannot be released twice.
- After balance-changing operations, the contract checks that available balance
  is non-negative and that:
  `total_deposited == released_amount + refunded_amount + available_balance`.

## Protocol Fee Accounting

Protocol fees are deducted on each `release_milestone` at a configurable rate
(set by admin via `set_protocol_fee_bps`, capped at 1000 bps / 10%):

```
fee     = milestone_amount * fee_bps / 10_000  (round down)
net     = milestone_amount - fee
fee + net == milestone_amount  (invariant)
```

Fees are retained inside the contract and accumulated under
`DataKey::AccumulatedProtocolFees`. The default rate is 0 bps (no fee).

### Protocol Fee Withdrawal

The admin can withdraw accumulated fees using `withdraw_protocol_fees(amount, to)`:

- Requires the stored admin's `require_auth()`.
- Blocked while the contract is paused or in emergency mode.
- Rejects `amount <= 0` (`AmountMustBePositive`).
- Rejects `amount > AccumulatedProtocolFees` (`InsufficientAccumulatedFees`).
- Decrements `AccumulatedProtocolFees` atomically and extends its persistent TTL.
- Transfers `amount` tokens via `token::Client::transfer` from the contract to `to`.
- Emits a `("fee", "withdraw")` event with `(admin, to, amount, timestamp)`.

**Lifetime invariant:** `sum(all withdrawn amounts) <= sum(all accrued fees)`.
This holds because each withdrawal debits the accumulator before the transfer.

