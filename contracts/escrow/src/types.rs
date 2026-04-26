use soroban_sdk::contracttype;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContractStatus {
    Created,
    Funded,
    Disputed,
    Cancelled,
    Completed,
    Refunded,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub amount: i128,
    pub released: bool,
    pub refunded: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MainnetReadinessInfo {
    pub is_ready: bool,
    pub protocol_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessChecklist {
    pub has_bounds: bool,
    pub has_ttl: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Contract(u32),
    ContractCount,
    Milestones(u32),
    MilestoneReleased(u32, u32),
    MilestoneApprovalTime(u32, u32),
    RefundableBalance(u32),
    ReadinessChecklist,
    PendingApproval(u32),
    PendingMigration,
}
