use soroban_sdk::contracttype;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractStatus {
    Created = 0,
    Funded = 1,
    Completed = 2,
    Disputed = 3,
    Cancelled = 4,
    Refunded = 5,
}
