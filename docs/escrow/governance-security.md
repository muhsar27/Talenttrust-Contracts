# Escrow Governance Security

The live escrow contract has a single operational admin initialized by
`initialize(admin)`. That admin can pause, unpause, activate emergency pause, and
resolve emergency mode.

## Implemented Admin Controls

- `initialize(admin) -> bool`
- `get_admin() -> Option<Address>`
- `pause() -> bool`
- `unpause() -> bool`
- `activate_emergency_pause() -> bool`
- `resolve_emergency() -> bool`
- `is_paused() -> bool`
- `is_emergency() -> bool`
- `propose_governance_admin(proposed) -> bool`
- `accept_governance_admin() -> bool`
- `get_pending_governance_admin() -> Option<Address>`
- `get_pending_governance_admin_proposed_at() -> Option<u32>`

All mutating admin controls require the stored admin's Soroban authorization.

### Admin Rotation (Two-Step Transfer)
To prevent accidental lock-outs, the contract implements a two-step admin rotation:
1. **Propose**: The current admin proposes a new address. This creates a `PendingAdminProposal` record containing the proposed address and the ledger sequence of the proposal.
2. **Accept**: The proposed admin must authorize the `accept_governance_admin` call after the `ADMIN_ROTATION_MIN_DELAY_LEDGERS` has elapsed.

**Storage Shape**:
The pending proposal is stored under `DataKey::PendingAdmin` as a `PendingAdminProposal` struct:
```rust
pub struct PendingAdminProposal {
    pub proposed: Address,
    pub proposed_at_ledger: u32,
}
```

There is no live admin transfer entrypoint.

## Planned Governance Work

- Two-step admin transfer:
  [#318](https://github.com/Talenttrust/Talenttrust-Contracts/issues/318)
- Governed parameter setter/readiness wiring:
  [#323](https://github.com/Talenttrust/Talenttrust-Contracts/issues/323)
- Audit events for future fee/admin changes:
  [#340](https://github.com/Talenttrust/Talenttrust-Contracts/issues/340)

Until those issues land, operational key management for the initialized admin is
an off-chain process.
