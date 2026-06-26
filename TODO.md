# TODO - #446 participant indexer / paginated list

## Step 1: Implement participant index & reader
- [ ] Add `DataKey::ClientContracts(Address)` and `DataKey::FreelancerContracts(Address)` in `contracts/escrow/src/types.rs`.
- [ ] Update `contracts/escrow/src/create_contract.rs` to append new contract id to both indices (client + freelancer) after persisting.
- [ ] Bump persistent TTL for the index entries on write.
- [ ] Add `list_contracts_by_participant` entrypoint in `contracts/escrow/src/lib.rs` with bounded pagination and max limit.


## Step 2: Tests
- [ ] Extend `contracts/escrow/src/test/persistence.rs` with pagination and per-participant correctness tests.
- [ ] Validate edge cases: empty index, start out of range, limit larger than index, limit==0.

## Step 3: Docs
- [x] Update `docs/escrow/state-persistence.md` documenting the two new DataKeys.



## Step 4: Sanity checks
- [ ] Run `cargo fmt --all -- --check`
- [ ] Run `cargo build`
- [ ] Run `cargo test`
- [ ] Ensure tests cover impacted modules and coverage remains high.

