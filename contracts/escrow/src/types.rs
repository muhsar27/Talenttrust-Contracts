use soroban_sdk::{contracterror, contracttype, Bytes, String};

#[contracttype]
pub enum DataKey {
    Client,
    Freelancer,
    Milestones,
    Initialized,
    MilestoneFunded(u32),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    IndexOutOfBounds = 3,
    AlreadyReleased = 4,
    InvalidStatusTransition = 5,
    InsufficientMilestoneFunding = 6,
}

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
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Milestone {
    pub amount: i128,
    pub released: bool,
    pub refunded: bool,
    pub work_evidence: Option<String>,
    pub funded_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneFunding {
    pub contract_id: u32,
    pub milestone_idx: u32,
    pub funded_amount: i128,
}

