/// TTL (Time To Live) constants for temporary storage
/// 
/// These constants define the lifetime of approval records in temporary storage.
/// Expired approvals are automatically evicted and treated as absent.

/// Number of ledgers an approval remains valid before expiring
/// At ~5 seconds per ledger, this is approximately 7 days
pub const PENDING_APPROVAL_TTL_LEDGERS: u32 = 120_960;

/// Threshold at which to bump the TTL for an approval
/// Set to 50% of TTL to ensure approvals don't expire unexpectedly
pub const PENDING_APPROVAL_BUMP_THRESHOLD: u32 = 60_480;

/// Minimum TTL for approval records (1 day worth of ledgers)
pub const MIN_APPROVAL_TTL: u32 = 17_280;
