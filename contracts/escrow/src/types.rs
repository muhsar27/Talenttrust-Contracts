use soroban_sdk::{contracterror, contracttype, Address, String, Vec};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
/// Unified error enum for the escrow contract.
pub enum Error {
    // ── Participant / identity ─────────────────────────────────────────────
    /// `client` and `freelancer` must be distinct addresses.
    InvalidParticipant = 1,
    /// `arbiter` address overlaps with `client` or `freelancer`.
    InvalidArbiter = 2,
    /// An arbiter-requiring `ReleaseAuthorization` mode was selected but no arbiter was provided.
    MissingArbiter = 3,
    /// A contract participant address failed a role check.
    UnauthorizedRole = 4,

    // ── Milestone amount validation ────────────────────────────────────────
    /// Milestone list is empty.
    EmptyMilestones = 5,
    /// Too many milestones (exceeds [`MAX_MILESTONES`]).
    TooManyMilestones = 6,
    /// A milestone amount is zero or negative.
    InvalidMilestoneAmount = 7,
    /// The sum of all milestone amounts exceeds [`MAX_TOTAL_ESCROW_STROOPS`].
    TotalCapExceeded = 8,
    /// Checked arithmetic detected a potential i128 overflow.
    PotentialOverflow = 9,

    // ── Deposit validation ────────────────────────────────────────────────
    /// The deposit amount is zero or negative.
    InvalidDepositAmount = 10,
    /// Depositing this amount would push `total_deposited` above the contract total.
    DepositWouldExceedTotal = 11,

    // ── State machine ─────────────────────────────────────────────────────
    /// The referenced contract ID does not exist.
    ContractNotFound = 12,
    /// The contract is not in the required state for this operation.
    InvalidState = 13,

    // ── Milestone lifecycle ───────────────────────────────────────────────
    /// The milestone index is out of bounds.
    InvalidMilestone = 14,
    /// The milestone was already released.
    AlreadyReleased = 15,
    /// The milestone was already refunded.
    AlreadyRefunded = 16,
    /// The contract does not have enough funded balance.
    InsufficientFunds = 17,

    // ── Refund ───────────────────────────────────────────────────────────
    /// Refund request contains no milestone indices.
    EmptyRefundRequest = 18,
    /// The same milestone index appears more than once in a single refund request.
    DuplicateMilestoneInRefund = 19,

    // ── Approvals ─────────────────────────────────────────────────────────
    /// The required approval(s) are missing or were never submitted.
    InsufficientApprovals = 20,
    /// The approval record in temporary storage has expired (TTL elapsed).
    ApprovalExpired = 21,
    /// The caller already submitted an approval for this milestone.
    AlreadyApproved = 22,
    /// The milestone was already released (approval-time check).
    MilestoneAlreadyReleased = 23,

    // ── Misc ──────────────────────────────────────────────────────────────
    /// The amount supplied must be a positive value (> 0 stroops).
    AmountMustBePositive = 24,
    /// Accounting invariant violated (internal consistency check).
    AccountingInvariantViolated = 25,

    // ── Reputation ───────────────────────────────────────────────────────
    /// Rating value is outside the allowed range.
    InvalidRating = 26,
    /// Reputation token was already issued for this contract.
    ReputationAlreadyIssued = 27,
    /// The supplied freelancer address does not match the stored one.
    FreelancerMismatch = 28,

    // ── Additional error codes ───────────────────────────────────────────
    ContractIdCollision = 29,
    ContractIdOverflow = 30,
    IndexOutOfBounds = 31,
    AlreadyInitialized = 32,
    InsufficientAccumulatedFees = 33,
    AlreadyFinalized = 34,
    InvalidDisputeSplit = 35,
    NotCompleted = 36,
    SelfRating = 37,
    ContractPaused = 38,
    EmergencyActive = 39,
    InvalidStatusTransition = 40,
    NotInitialized = 41,
    TotalExceedsMaxEscrow = 42,
    FundingExceedsRequired = 43,
    InvalidParticipants = 44,
    InsufficientEscrowBalance = 45,
    MilestoneNotFound = 46,
    ExactDepositRequired = 47,
    InvalidProtocolParameters = 48,
}



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
    pub deadline: Option<u64>,
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
}

/// Defines who can approve milestone releases.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseAuthorization {
    /// Only client can approve.
    ClientOnly = 0,
    /// Either client or arbiter can approve.
    ClientAndArbiter = 1,
    /// Only arbiter can approve.
    ArbiterOnly = 2,
    /// Both client and freelancer must approve; only either of them may release
    /// after both approvals are present.
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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct Reputation {
    pub completed_contracts: i128,
    pub total_rating: i128,
    pub last_rating: i128,
}
