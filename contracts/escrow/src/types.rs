use soroban_sdk::{contracterror, contracttype, Address, String, Vec};

// ─── Indexer summary types ────────────────────────────────────────────────────

#[allow(dead_code)]
pub const CONTRACT_SUMMARY_SCHEMA_VERSION: u32 = 1;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneSummary {
    pub index: u32,
    pub amount: i128,
    pub released: bool,
    pub refunded: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractSummary {
    pub schema_version: u32,
    pub client: Address,
    pub freelancer: Address,
    pub arbiter: Option<Address>,
    pub status: ContractStatus,
    pub reputation_issued: bool,
    pub total_amount: i128,
    pub funded_amount: i128,
    pub released_amount: i128,
    pub refundable_balance: i128,
    pub released_milestone_count: u32,
    pub milestones: Vec<MilestoneSummary>,
}

/// Main escrow contract state
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contract {
    pub client: Address,
    pub freelancer: Address,
    pub arbiter: Option<Address>,
    pub status: ContractStatus,
    pub funded_amount: i128,
    pub released_amount: i128,
    pub refunded_amount: i128,
    pub release_authorization: ReleaseAuthorization,
    pub reputation_issued: bool,
    pub deposit_mode: DepositMode,
}

// ─── Storage keys ──────────────────────────────────────────────────────────────

/// Mapping from every `DataKey` variant to its storage tier, value type, and TTL
/// behavior. See `docs/escrow/state-persistence.md` for the full reference table.
///
/// # Storage tiers
/// - **Persistent** — `env.storage().persistent()`; TTL managed manually via `extend_ttl`.
/// - **Temporary** — `env.storage().temporary()`; auto-evicted by Soroban after TTL elapses.
/// - **Instance** — `env.storage().instance()`; not used by any current variant.
///
/// # TTL constants (from `crate::ttl`)
/// - `PERSISTENT_TTL_LEDGERS` = 30 d, `PERSISTENT_BUMP_THRESHOLD` = 7 d
/// - `PENDING_APPROVAL_TTL_LEDGERS` = 7 d, `PENDING_APPROVAL_BUMP_THRESHOLD` = 1 d
/// - `PENDING_MIGRATION_TTL_LEDGERS` = 21 d, `PENDING_MIGRATION_BUMP_THRESHOLD` = 3 d
///
/// # Security
/// Most persistent keys are never TTL-bumped on read (see "Security Notes" in
/// `state-persistence.md`). Only `Contract(u32)`, its composite milestone key,
/// `NextContractId`, and `MilestoneApprovals` receive explicit TTL extension.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Persistent · `bool` · Written by `initialize` · **No TTL bump on access**.
    /// Guard against double-init.
    Initialized,
    /// Persistent · `Address` · Written by `initialize`, `accept_governance_admin` · **No TTL bump on access**.
    /// The operational admin address for governance operations.
    Admin,
    /// Persistent · `bool` · Written by `pause`, `unpause`, emergency controls · **No TTL bump on access**.
    Paused,
    /// Persistent · `bool` · Written by `activate_emergency_pause`, `resolve_emergency` · **No TTL bump on access**.
    /// Indicates the contract is in emergency lockdown.
    Emergency,
    // Contract storage
    /// Persistent · `Contract` · Written by create/deposit/release/refund/cancel/migration · **Bumped on every access** (30 d).
    /// The primary escrow contract record. Milestones stored under composite key `(Contract(id), "milestones")`.
    Contract(u32),
    /// Persistent · `u32` · Written by `create_contract`, `bump_next_contract_id` · **Bumped on every write** (30 d).
    /// Monotonically incrementing counter for contract ID allocation.
    NextContractId,
    /// Persistent · `bool` · **Not written** after Milestone.released field became canonical.
    /// Legacy variant kept for backward-compatible indexing.
    MilestoneReleased(u32, u32),
    /// **Temporary** · `MilestoneApprovals` · Written by `approve_milestone` · TTL = 7 d, bump threshold = 1 d.
    /// Multi-sig approval tracking. Auto-evicted by Soroban; fail-closed on expiry.
    MilestoneApprovals(u32, u32),
    // Reputation
    /// Persistent · `bool` · Written by `issue_reputation` · **No TTL bump on access**.
    /// Prevents double-issuance per contract.
    ReputationIssued(u32),
    /// Persistent · `i128` · Written by reputation logic · **No TTL bump on access**.
    /// Counter of reputation credits awaiting issuance for an address.
    PendingReputationCredits(Address),
    /// Persistent · `Reputation` · Written by `issue_reputation` · **No TTL bump on access**.
    /// Aggregate reputation record (completed contracts, ratings).
    Reputation(Address),
    // Client migration
    /// **Temporary** · `PendingClientMigration` · Written by `propose_client_migration` · TTL = 21 d.
    /// Pending client transfer request. Cleared on accept via `remove_transient`.
    PendingClientMigration(u32),
    // Protocol / governance
    /// *(Unused)* — governance uses `DataKey::Admin`.
    GovernanceAdmin,
    /// *(Unused)* — governance uses `DataKey::PendingAdmin`.
    PendingGovernanceAdmin,
    /// *(Unused)* — declared but never stored.
    ProtocolParameters,
    /// Persistent · `u32` · Written by `set_protocol_fee_bps` · **No TTL bump on access**.
    /// Base-point fee deducted from each milestone release.
    ProtocolFeeBps,
    // Two-step admin transfer: pending admin stored here while proposal awaits acceptance
    /// Persistent · `Address` · Written by `propose_governance_admin` · **No TTL bump on access**.
    /// Cleared on accept. Enables two-step admin transfer.
    PendingAdmin,
    /// Persistent · `i128` · Written by `release_milestone` (increment) · **No TTL bump on access**.
    /// Running total of protocol fees collected.
    AccumulatedProtocolFees,
    /// *(Unused)* — `GovernedParameters` struct is used as a value type but never stored under this key.
    GovernedParameters,
    /// Persistent · `ReadinessChecklist` · Written by `initialize`, `activate_emergency_pause` · **No TTL bump on access**.
    /// Bitfield tracking initialization, params, and emergency state for mainnet readiness.
    ReadinessChecklist,
    // Finalization
    /// Persistent · `FinalizationRecord` · Written by `finalize_contract` · **No TTL bump on access**.
    /// Immutable close metadata, written once per contract.
    Finalization(u32),
    // Settlement token (SAC)
    SettlementToken,
}

/// Canonical contract error type for all entrypoint-facing errors.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    IndexOutOfBounds = 3,
    AlreadyReleased = 4,
    EmptyRefundRequest = 6,
    DuplicateMilestoneInRefund = 7,
    AlreadyRefunded = 8,
    InsufficientFunds = 9,
    ContractNotFound = 10,
    UnauthorizedRole = 11,
    MissingArbiter = 12,
    InvalidArbiter = 13,
    InvalidParticipants = 14,
    AmountMustBePositive = 15,
    InvalidState = 16,
    MilestoneAlreadyReleased = 17,
    AlreadyApproved = 18,
    MilestoneNotFound = 19,
    InsufficientApprovals = 20,
    FreelancerMismatch = 21,
    InvalidRating = 22,
    ReputationAlreadyIssued = 23,
    EvidenceTooLong = 24,
    EmptyMilestones = 25,
    InvalidMilestoneAmount = 26,
    ContractIdCollision = 27,
    ContractIdOverflow = 28,
    EmptyComment = 29,
    CommentTooLong = 30,
    ExactDepositRequired = 31,
    DepositWouldExceedTotal = 32,
}

/// Contract lifecycle states
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractStatus {
    Created = 0,
    Accepted = 1,
    Funded = 2,
    Completed = 3,
    Disputed = 4,
    Cancelled = 5,
    Refunded = 6,
    PartiallyFunded = 7,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub amount: i128,
    pub funded_amount: i128,
    pub released: bool,
    pub refunded: bool,
    pub work_evidence: Option<String>,
    pub refunded_amount: i128,
}

/// Defines who can approve milestone releases.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseAuthorization {
    ClientOnly = 0,
    ClientAndArbiter = 1,
    ArbiterOnly = 2,
    MultiSig = 3,
}

/// Tracks approval status for a milestone.
/// Stored in temporary storage with TTL for expiry grace period.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneApprovals {
    pub client_approved: bool,
    pub freelancer_approved: bool,
    pub arbiter_approved: bool,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DepositMode {
    ExactTotal = 0,
    Incremental = 1,
}

/// Readiness checklist stored under [`DataKey::ReadinessChecklist`].
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessChecklist {
    /// `true` after `initialize` has been called successfully.
    pub initialized: bool,
    /// `true` after protocol governance parameters have been set.
    pub governed_params_set: bool,
    /// `true` after an emergency control operation has been invoked.
    pub emergency_controls_enabled: bool,
}

impl Default for ReadinessChecklist {
    fn default() -> Self {
        ReadinessChecklist {
            initialized: false,
            governed_params_set: false,
            emergency_controls_enabled: false,
        }
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernedParameters {
    pub protocol_fee_bps: u32,
    pub max_escrow_total_stroops: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct Reputation {
    pub completed_contracts: i128,
    pub total_rating: i128,
    pub last_rating: i128,
}

/// Readiness checklist stored under [`DataKey::ReadinessChecklist`].
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessChecklist {
    pub initialized: bool,
    pub governed_params_set: bool,
    pub emergency_controls_enabled: bool,
}

impl Default for ReadinessChecklist {
    fn default() -> Self {
        ReadinessChecklist {
            initialized: false,
            governed_params_set: false,
            emergency_controls_enabled: false,
        }
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernedParameters {
    pub protocol_fee_bps: u32,
    pub max_escrow_total_stroops: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingAdminProposal {
    pub proposed: Address,
    pub proposed_at_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernedParameters {
    pub protocol_fee_bps: u32,
    pub max_escrow_total_stroops: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingAdminProposal {
    pub proposed: Address,
    pub proposed_at_ledger: u32,
}
