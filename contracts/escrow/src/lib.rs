#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec,
};

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractStatus {
    Created = 0,
    Funded = 1,
    Completed = 2,
    Disputed = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub amount: i128,
    pub released: bool,
}

/// Escrow record layout for storage version `V1`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowRecord {
    pub client: Address,
    pub freelancer: Address,
    pub milestones: Vec<Milestone>,
    pub milestone_count: u32,
    pub total_amount: i128,
    pub funded_amount: i128,
    pub released_amount: i128,
    pub released_milestones: u32,
    pub status: ContractStatus,
    pub reputation_issued: bool,
}

/// Freelancer reputation aggregate layout for storage version `V1`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reputation {
    pub total_rating: i128,
    pub ratings_count: u32,
}

/// Public description of the active storage namespaces.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageLayoutPlan {
    pub version: u32,
    pub meta_namespace: Symbol,
    pub contracts_namespace: Symbol,
    pub reputation_namespace: Symbol,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
enum StorageVersion {
    V1 = 1,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum MetaKey {
    LayoutVersion,
    NextContractId,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum V1Key {
    Contract(u32),
    Reputation(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum DataKey {
    Meta(MetaKey),
    V1(V1Key),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum EscrowError {
    InvalidParticipants = 1,
    EmptyMilestones = 2,
    InvalidMilestoneAmount = 3,
    ContractNotFound = 4,
    AmountMustBePositive = 5,
    ArithmeticOverflow = 6,
    InvalidState = 7,
    MilestoneNotFound = 8,
    MilestoneAlreadyReleased = 9,
    InsufficientEscrowBalance = 10,
    FundingExceedsRequired = 11,
    InvalidRating = 12,
    ReputationAlreadyIssued = 13,
    UnsupportedStorageVersion = 14,
    UnsupportedMigrationTarget = 15,
}

#[contract]
pub struct Escrow;

#[contractimpl]
impl Escrow {
    /// Returns the currently active storage layout version.
    ///
    /// If version metadata is missing, this initializes the contract metadata
    /// to layout `V1` and returns `1`.
    pub fn get_storage_version(env: Env) -> Result<u32, EscrowError> {
        ensure_storage_layout(&env)?;
        Ok(StorageVersion::V1 as u32)
    }

    /// Returns the storage namespace plan used by the contract.
    ///
    /// This serves as an explicit migration-safe contract between code and
    /// stored keys. Future versions can add `V2(...)` key variants without
    /// mutating `V1` data formats.
    pub fn storage_layout_plan(env: Env) -> Result<StorageLayoutPlan, EscrowError> {
        ensure_storage_layout(&env)?;
        Ok(StorageLayoutPlan {
            version: StorageVersion::V1 as u32,
            meta_namespace: symbol_short!("meta_v1"),
            contracts_namespace: symbol_short!("escrow_v1"),
            reputation_namespace: symbol_short!("rep_v1"),
        })
    }

    /// Migration entrypoint for future layouts.
    ///
    /// For now only `V1` exists. Migrating to `1` is a no-op and returns
    /// `true`. Any other target is rejected.
    pub fn migrate_storage(env: Env, target_version: u32) -> Result<bool, EscrowError> {
        ensure_storage_layout(&env)?;
        if target_version != StorageVersion::V1 as u32 {
            return Err(EscrowError::UnsupportedMigrationTarget);
        }
        Ok(true)
    }

    /// Creates a new escrow contract under storage layout `V1`.
    pub fn create_contract(
        env: Env,
        client: Address,
        freelancer: Address,
        milestone_amounts: Vec<i128>,
    ) -> Result<u32, EscrowError> {
        ensure_storage_layout(&env)?;
        client.require_auth();

        if client == freelancer {
            return Err(EscrowError::InvalidParticipants);
        }

        let milestone_count = milestone_amounts.len();
        if milestone_count == 0 {
            return Err(EscrowError::EmptyMilestones);
        }

        let mut milestones = Vec::new(&env);
        let mut total_amount = 0_i128;
        let mut i = 0_u32;
        while i < milestone_count {
            let amount = milestone_amounts
                .get(i)
                .ok_or(EscrowError::InvalidMilestoneAmount)?;
            if amount <= 0 {
                return Err(EscrowError::InvalidMilestoneAmount);
            }
            total_amount = total_amount
                .checked_add(amount)
                .ok_or(EscrowError::ArithmeticOverflow)?;
            milestones.push_back(Milestone {
                amount,
                released: false,
            });
            i += 1;
        }

        let id = next_contract_id(&env)?;
        let record = EscrowRecord {
            client,
            freelancer,
            milestones,
            milestone_count,
            total_amount,
            funded_amount: 0,
            released_amount: 0,
            released_milestones: 0,
            status: ContractStatus::Created,
            reputation_issued: false,
        };

        save_contract(&env, id, &record);
        Ok(id)
    }

    /// Deposits funds into escrow for a contract.
    pub fn deposit_funds(env: Env, contract_id: u32, amount: i128) -> Result<bool, EscrowError> {
        ensure_storage_layout(&env)?;

        if amount <= 0 {
            return Err(EscrowError::AmountMustBePositive);
        }

        let mut record = load_contract(&env, contract_id)?;
        record.client.require_auth();

        if record.status == ContractStatus::Completed {
            return Err(EscrowError::InvalidState);
        }

        let updated_funded = record
            .funded_amount
            .checked_add(amount)
            .ok_or(EscrowError::ArithmeticOverflow)?;

        if updated_funded > record.total_amount {
            return Err(EscrowError::FundingExceedsRequired);
        }

        record.funded_amount = updated_funded;
        if record.funded_amount > 0 {
            record.status = ContractStatus::Funded;
        }

        save_contract(&env, contract_id, &record);
        Ok(true)
    }

    /// Releases a milestone payment for a funded contract.
    pub fn release_milestone(
        env: Env,
        contract_id: u32,
        milestone_id: u32,
    ) -> Result<bool, EscrowError> {
        ensure_storage_layout(&env)?;

        let mut record = load_contract(&env, contract_id)?;
        record.client.require_auth();

        if record.status != ContractStatus::Funded {
            return Err(EscrowError::InvalidState);
        }

        let mut milestone = record
            .milestones
            .get(milestone_id)
            .ok_or(EscrowError::MilestoneNotFound)?;

        if milestone.released {
            return Err(EscrowError::MilestoneAlreadyReleased);
        }

        let available_balance = record
            .funded_amount
            .checked_sub(record.released_amount)
            .ok_or(EscrowError::ArithmeticOverflow)?;

        if milestone.amount > available_balance {
            return Err(EscrowError::InsufficientEscrowBalance);
        }

        milestone.released = true;
        record.milestones.set(milestone_id, milestone.clone());

        record.released_amount = record
            .released_amount
            .checked_add(milestone.amount)
            .ok_or(EscrowError::ArithmeticOverflow)?;
        record.released_milestones = record
            .released_milestones
            .checked_add(1)
            .ok_or(EscrowError::ArithmeticOverflow)?;

        if record.released_milestones == record.milestone_count {
            record.status = ContractStatus::Completed;
        }

        save_contract(&env, contract_id, &record);
        Ok(true)
    }

    /// Issues reputation for a freelancer after contract completion.
    pub fn issue_reputation(env: Env, contract_id: u32, rating: i128) -> Result<bool, EscrowError> {
        ensure_storage_layout(&env)?;

        if !(1..=5).contains(&rating) {
            return Err(EscrowError::InvalidRating);
        }

        let mut record = load_contract(&env, contract_id)?;
        record.client.require_auth();

        if record.status != ContractStatus::Completed {
            return Err(EscrowError::InvalidState);
        }

        if record.reputation_issued {
            return Err(EscrowError::ReputationAlreadyIssued);
        }

        let rep_key = DataKey::V1(V1Key::Reputation(record.freelancer.clone()));
        let mut reputation = env
            .storage()
            .persistent()
            .get::<_, Reputation>(&rep_key)
            .unwrap_or(Reputation {
                total_rating: 0,
                ratings_count: 0,
            });

        reputation.total_rating = reputation
            .total_rating
            .checked_add(rating)
            .ok_or(EscrowError::ArithmeticOverflow)?;
        reputation.ratings_count = reputation
            .ratings_count
            .checked_add(1)
            .ok_or(EscrowError::ArithmeticOverflow)?;

        env.storage().persistent().set(&rep_key, &reputation);

        record.reputation_issued = true;
        save_contract(&env, contract_id, &record);
        Ok(true)
    }

    /// Returns contract state for a given contract id.
    pub fn get_contract(env: Env, contract_id: u32) -> Result<EscrowRecord, EscrowError> {
        ensure_storage_layout(&env)?;
        load_contract(&env, contract_id)
    }

    /// Returns aggregate reputation for a freelancer.
    pub fn get_reputation(env: Env, freelancer: Address) -> Result<Reputation, EscrowError> {
        ensure_storage_layout(&env)?;
        Ok(env
            .storage()
            .persistent()
            .get::<_, Reputation>(&DataKey::V1(V1Key::Reputation(freelancer)))
            .unwrap_or(Reputation {
                total_rating: 0,
                ratings_count: 0,
            }))
    }

    /// Hello-world style function for testing and CI.
    pub fn hello(_env: Env, to: Symbol) -> Symbol {
        to
    }
}

fn ensure_storage_layout(env: &Env) -> Result<(), EscrowError> {
    let storage = env.storage().persistent();
    let version_key = DataKey::Meta(MetaKey::LayoutVersion);

    match storage.get::<_, u32>(&version_key) {
        Some(version) if version == StorageVersion::V1 as u32 => {}
        Some(_) => return Err(EscrowError::UnsupportedStorageVersion),
        None => storage.set(&version_key, &(StorageVersion::V1 as u32)),
    };

    let next_id_key = DataKey::Meta(MetaKey::NextContractId);
    if storage.get::<_, u32>(&next_id_key).is_none() {
        storage.set(&next_id_key, &1_u32);
    }
    Ok(())
}

fn next_contract_id(env: &Env) -> Result<u32, EscrowError> {
    let key = DataKey::Meta(MetaKey::NextContractId);
    let storage = env.storage().persistent();

    let id = storage.get::<_, u32>(&key).unwrap_or(1_u32);
    let next = id.checked_add(1).ok_or(EscrowError::ArithmeticOverflow)?;

    storage.set(&key, &next);
    Ok(id)
}

fn load_contract(env: &Env, contract_id: u32) -> Result<EscrowRecord, EscrowError> {
    env.storage()
        .persistent()
        .get::<_, EscrowRecord>(&DataKey::V1(V1Key::Contract(contract_id)))
        .ok_or(EscrowError::ContractNotFound)
}

fn save_contract(env: &Env, contract_id: u32, record: &EscrowRecord) {
    env.storage()
        .persistent()
        .set(&DataKey::V1(V1Key::Contract(contract_id)), record);
}

#[cfg(test)]
mod test;
