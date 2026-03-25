# PR: feat: implement contract status transition guardrails with tests and docs

## Summary
- Added status transition guardrails for escrow contract states in `contracts/escrow/src/lib.rs`.
- Implemented transition matrix in `ContractStatus::can_transition_to`.
- Enforced transition rules in `EscrowContract::transition_status`.
- Updated `deposit_funds` and `release_milestone` to use safe transitions.
- Added `dispute_contract` operation to cover `Funded -> Disputed` transition.
- Added tests in `contracts/escrow/src/test.rs`:
  - `test_dispute_contract_transitions`
  - `test_invalid_status_transition_from_completed_to_funded`
- Added docs: `README.md` and `docs/escrow/status-transition-guardrails.md`.

## Testing
- Intended command: `cargo test --workspace`.
- Container lacks root permission to install Rust toolchain (`apt-get` permission denied), so tests could not be executed in this environment. Please run locally or in CI.

## Security notes
- Invalid transitions now panic explicitly; no undefined state changes.
- Contract status changes are centralized in transition helper.
- Released milestone progression and dispute handling are covered.

## Attachments
- ![Proof image](ATTACHMENT_PLACEHOLDER)

### How to attach
1. Run tests locally: `cargo test --workspace`.
2. Capture screenshot or test output log.
3. Add to PR comments using GitHub upload, then replace `ATTACHMENT_PLACEHOLDER` with the actual image URL.

---

Please verify the update path and run end-to-end tests with a Rust toolchain available.  
