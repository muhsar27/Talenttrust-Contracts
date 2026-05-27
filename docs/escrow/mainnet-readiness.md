# Mainnet Readiness — `get_mainnet_readiness_info`

**Issue:** #178  
**Scope:** `contracts/escrow` only.

This document describes the `get_mainnet_readiness_info` view function, the `MainnetReadinessInfo` struct it returns, and how deployers should use it to validate a contract instance before directing production traffic to it.

---

## Overview

`get_mainnet_readiness_info` is a read-only function that returns a single `MainnetReadinessInfo` snapshot capturing all deployment-critical conditions. It is safe to call from preflight scripts, monitoring tools, or dashboards at any time — it requires no authorization, emits no events, and writes nothing to storage.

```rust
pub fn get_mainnet_readiness_info(env: Env) -> MainnetReadinessInfo
```

---

## `MainnetReadinessInfo` Fields

| Field | Type | Meaning | Set by |
|---|---|---|---|
| `caps_set` | `bool` | `true` when `MAINNET_MAX_TOTAL_ESCROW_PER_CONTRACT_STROOPS > 0`. Derived inline from the compile-time constant — always `true` in any deployed build that has the constant defined. | Compile-time constant (not persisted) |
| `governed_params_set` | `bool` | `true` once protocol governance parameters have been configured on-chain. | `set_governed_params` |
| `emergency_controls_enabled` | `bool` | `true` once the emergency control mechanism has been exercised at least once (pause or resolve). | `activate_emergency_pause` or `resolve_emergency` |
| `initialized` | `bool` | `true` once the contract has been initialized via `initialize`. | `initialize` |
| `protocol_version` | `u32` | Always equals the `MAINNET_PROTOCOL_VERSION` compile-time constant (currently `1`). | Compile-time constant (not persisted) |
| `max_escrow_total_stroops` | `i128` | Always equals the `MAINNET_MAX_TOTAL_ESCROW_PER_CONTRACT_STROOPS` compile-time constant (currently `1_000_000_000_000_000` = 100 M XLM). | Compile-time constant (not persisted) |

### Field lifecycle details

- **`caps_set`** — computed as `MAINNET_MAX_TOTAL_ESCROW_PER_CONTRACT_STROOPS > 0` every time the view function is called. It is never stored in the `ReadinessChecklist`; it reflects the WASM binary itself.
- **`governed_params_set`** — persisted in instance storage. Set to `true` atomically within the same transaction as a successful `set_governed_params` call. Once `true`, it stays `true` (governance parameters can be updated again, but the flag is never reset to `false`).
- **`emergency_controls_enabled`** — persisted in instance storage. Set to `true` atomically within the same transaction as a successful `activate_emergency_pause` or `resolve_emergency` call. Indicates the emergency control path has been validated end-to-end.
- **`initialized`** — persisted in instance storage. Set to `true` atomically within the same transaction as a successful `initialize` call. The `initialize` function panics on a second call, so this flag is set exactly once.
- **`protocol_version`** — read directly from the `MAINNET_PROTOCOL_VERSION` constant at call time. Not affected by storage state.
- **`max_escrow_total_stroops`** — read directly from the `MAINNET_MAX_TOTAL_ESCROW_PER_CONTRACT_STROOPS` constant at call time. Not affected by storage state. This cap is enforced on-chain and cannot be changed without a WASM upgrade.

---

## Example Output

A fully-ready contract returns:

```json
{
  "caps_set": true,
  "governed_params_set": true,
  "emergency_controls_enabled": true,
  "initialized": true,
  "protocol_version": 1,
  "max_escrow_total_stroops": 1000000000000000
}
```

A freshly deployed contract with no lifecycle operations completed returns:

```json
{
  "caps_set": true,
  "governed_params_set": false,
  "emergency_controls_enabled": false,
  "initialized": false,
  "protocol_version": 1,
  "max_escrow_total_stroops": 1000000000000000
}
```

---

## Deployer Validation Guide

Before directing production traffic to a contract instance, verify all of the following conditions using `get_mainnet_readiness_info`:

1. **`caps_set` must be `true`** — confirms the WASM binary was compiled with a non-zero escrow cap. If `false`, the binary is misconfigured and must not be used.
2. **`governed_params_set` must be `true`** — confirms that `set_governed_params` has been called and protocol parameters are active on-chain.
3. **`emergency_controls_enabled` must be `true`** — confirms the emergency pause/resolve path has been exercised at least once, validating that the admin key can operate the emergency controls.
4. **`initialized` must be `true`** — confirms `initialize` has been called and the contract admin is set.
5. **`protocol_version` must match the expected version** — compare against the version your deployment tooling expects (currently `1`). A mismatch indicates a WASM version mismatch.
6. **`max_escrow_total_stroops` must match the expected cap** — compare against your deployment configuration (currently `1_000_000_000_000_000`). A mismatch indicates the wrong WASM binary was deployed.

A contract is production-ready only when all four boolean fields are `true` and both constant fields match expected values.

### Example preflight check (pseudocode)

```js
const info = await contract.get_mainnet_readiness_info();

assert(info.caps_set,                  "caps not set — wrong WASM binary");
assert(info.governed_params_set,       "governance not initialized");
assert(info.emergency_controls_enabled,"emergency controls not validated");
assert(info.initialized,               "contract not initialized");
assert(info.protocol_version === 1,    "unexpected protocol version");
assert(info.max_escrow_total_stroops === 1_000_000_000_000_000n, "unexpected escrow cap");
```

---

## Read-Only Safety

`get_mainnet_readiness_info` is strictly read-only:

- **No auth required** — can be called by any account or script without signing.
- **No storage writes** — calling it any number of times leaves contract state unchanged.
- **No events emitted** — does not produce any contract events.
- **Never panics** — uses `unwrap_or_default()` on the storage read, so it returns safe all-false defaults even on a freshly deployed contract with no storage entries.

It is safe to call from preflight scripts, CI pipelines, monitoring dashboards, and any other tooling without risk of side effects.

---

## On-Chain Limits

| Mechanism | What it does |
|-----------|--------------|
| `MAINNET_MAX_TOTAL_ESCROW_PER_CONTRACT_STROOPS` | Hard cap on the sum of milestone amounts per escrow. Not changeable via governance; requires a WASM upgrade to adjust. Reflected in `max_escrow_total_stroops`. |
| `GovernedParameters` | Governed limits: protocol fee and maximum escrow cap. Configured via `set_governed_params`. Reflected in `governed_params_set`. |

---

## Monitoring Hooks (Events)

Contract events use a stable two-symbol topic prefix so indexers can filter without parsing custom data shapes:

| Phase | Topics (`symbol_short!`) | Data payload |
|-------|--------------------------|--------------|
| Creation | `tt_esc`, `create` | `(contract_id: u32, total_amount: i128)` |
| Funding | `tt_esc`, `deposit` | `(contract_id: u32, amount: i128)` |
| Release | `tt_esc`, `release` | `(contract_id: u32, milestone_id: u32, amount: i128)` |

**Note:** Validate event visibility with your RPC / integration tests; Soroban host test helpers may not mirror production event delivery exactly.

---

## Known Risks (Residual)

- **Asset movement:** This crate models escrow balances in contract state. Actual token custody and transfers must be integrated with Stellar asset contracts and audited separately.
- **Reputation:** Ratings are self-submitted by the freelancer after completion; treat reputation as informational, not as a gate for fund safety.
- **Governance:** A compromised or malicious governance admin can tighten or loosen governed parameters within validation rules. Operational multisig and timelocks are out of contract scope but required for mainnet.
- **Per-contract cap:** The mainnet total cap is fixed in code; if product needs a different ceiling, ship a new WASM revision before go-live.

---

## Tests

Unit tests live in `contracts/escrow/src/test/mainnet_readiness.rs` and cover defaults, each lifecycle flag, idempotence, and backward compatibility. Property-based tests in `contracts/escrow/src/proptest.rs` verify idempotence, no-panic, constant invariants, and round-trip correctness across many generated contract states.
