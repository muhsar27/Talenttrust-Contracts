use soroban_sdk::{contracterror, contracttype, Address, String, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    // Admin / pause / emergency
    Initialized,
    Admin,
    Paused,
    Emergency,
    // Contract storage
    Contract(u32),
    NextContractId,
    MilestoneReleased(u32, u32),
    MilestoneApprovals(u32, u32),
    // Participant indexer (append-only contract id lists)
    ClientContracts(Address),
    FreelancerContracts(Address),
    // Reputation
    ReputationIssued(u32),
    PendingReputationCredits(Address),
    Reputation(Address),
    // Client migration
    PendingClientMigration(u32),
    // Protocol / governance
    GovernanceAdmin,
    PendingGovernanceAdmin,
    ProtocolParameters,
    ProtocolFeeBps,
    // Two-step admin transfer: pending admin stored here while proposal awaits acceptance
    PendingAdmin,
    AccumulatedProtocolFees,
    GovernedParameters,
    ReadinessChecklist,
    // Finalization
    Finalization(u32),
}


/// Canonical contract error type for all entrypoint-facing errors.
    // Removed duplicate canonical error enum; using unified definition from errors.rs

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

// ─── Indexer summary types ────────────────────────────────────────────────────

/// Current schema version for [`ContractSummary`].
///
/// Bumped when a breaking change is made to the summarised shape.
/// Downstream indexers MUST branch on this field to decode the
/// rest of the struct correctly.
///
/// # Current value
/// `1`
///
/// # Versioning policy
/// See `docs/escrow/indexer-schema.md`.
#[allow(dead_code)]
pub const CONTRACT_SUMMARY_SCHEMA_VERSION: u32 = 1;

/// Per-milestone summary embedded in [`ContractSummary`].
///
/// A lightweight projection of the on-chain [`Milestone`] that omits
/// internal accounting fields (`funded_amount`, `refunded_amount`,
/// `work_evidence`) that are not relevant to off-chain indexers.
///
/// # Fields
/// * `index` – 0-based position in the milestones vector.
/// * `amount` – Original amount specified at contract creation (stroops).
/// * `released` – `true` after [`release_milestone`] succeeds.
/// * `refunded` – `true` after [`refund_unreleased_milestones`] includes this
///   index.
///
/// [`release_milestone`]: crate::Escrow::release_milestone
/// [`refund_unreleased_milestones`]: crate::Escrow::refund_unreleased_milestones
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneSummary {
    /// 0-based milestone index.
    pub index: u32,
    /// Original milestone amount in stroops (set at creation).
    pub amount: i128,
    /// Whether this milestone has been released.
    pub released: bool,
    /// Whether this milestone has been refunded.
    pub refunded: bool,
}

/// Denormalised, versioned snapshot of an escrow contract for off-chain
/// indexers.
///
/// Produced during [`finalize_contract`] and stored as part of the
/// finalization record.
///
/// # Field provenance
///
/// | Classification | Fields |
/// |---|---|
/// | Copied verbatim from [`Contract`] | `client`, `freelancer`, `arbiter`, `status`, `funded_amount`, `released_amount` |
/// | Per-milestone projection from stored [`Milestone`] records | `milestones` |
/// | Derived at finalisation | `total_amount`, `refundable_balance`, `released_milestone_count` |
/// | Set to [`CONTRACT_SUMMARY_SCHEMA_VERSION`] | `schema_version` |
/// | Hardcoded (see caveat) | `reputation_issued` |
///
/// # Versioning
/// See `docs/escrow/indexer-schema.md`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractSummary {
    /// Schema version used to produce this snapshot.
    /// Indexers MUST check this before decoding.
    pub schema_version: u32,
    /// Client address (copied from [`Contract::client`]).
    pub client: Address,
    /// Freelancer address (copied from [`Contract::freelancer`]).
    pub freelancer: Address,
    /// Optional arbiter address (copied from [`Contract::arbiter`]).
    pub arbiter: Option<Address>,
    /// Contract status at finalisation time (copied from [`Contract::status`]).
    pub status: ContractStatus,
    /// **Hardcoded to `false`** – not yet wired to the on-chain
    /// `DataKey::ReputationIssued` flag.
    ///
    /// # Caveat
    /// This field does NOT reflect the actual reputation issuance state.
    /// See `docs/escrow/indexer-schema.md` for details.
    pub reputation_issued: bool,
    /// Sum of every milestone's `amount` field (derived at finalisation).
    pub total_amount: i128,
    /// Total amount deposited by the client (copied from
    /// [`Contract::funded_amount`]).
    pub funded_amount: i128,
    /// Total amount released to the freelancer (copied from
    /// [`Contract::released_amount`]).
    pub released_amount: i128,
    /// Remaining escrow balance that can be refunded (derived).
    ///
    /// Computed as:
    /// ```text
    /// refundable_balance = (funded_amount - released_amount) - refunded_amount
    /// ```
    pub refundable_balance: i128,
    /// Number of milestones with `released == true` (derived).
    pub released_milestone_count: u32,
    /// Per-milestone summary entries.
    pub milestones: Vec<MilestoneSummary>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DepositMode {
    ExactTotal = 0,
    Incremental = 1,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct Reputation {
    pub completed_contracts: i128,
    pub total_rating: i128,
    pub last_rating: i128,
}
