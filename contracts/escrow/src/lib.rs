#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Env, Symbol, Vec,
};

#[contract]
pub struct Escrow;

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowError {
    InvalidParticipant = 1,
    EmptyMilestones = 2,
    InvalidMilestoneAmount = 3,
    ContractNotFound = 4,
    InvalidDepositAmount = 5,
    DepositExceedsTotal = 6,
    InvalidMilestone = 7,
    MilestoneAlreadyReleased = 8,
    MilestoneAlreadyRefunded = 9,
    InsufficientEscrowBalance = 10,
    InvalidStatus = 11,
    EmptyRefundRequest = 12,
    DuplicateMilestone = 13,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractStatus {
    Created = 0,
    Funded = 1,
    Completed = 2,
    Refunded = 3,
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
pub struct EscrowContractData {
    pub client: Address,
    pub freelancer: Address,
    pub status: ContractStatus,
    pub total_amount: i128,
    pub funded_amount: i128,
    pub released_amount: i128,
    pub refunded_amount: i128,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Contract(u32),
    Milestones(u32),
    ContractCount,
}

#[contractimpl]
impl Escrow {
    pub fn hello(_env: Env, to: Symbol) -> Symbol {
        to
    }

    pub fn create_contract(
        env: Env,
        client: Address,
        freelancer: Address,
        milestone_amounts: Vec<i128>,
    ) -> u32 {
        client.require_auth();

        if client == freelancer {
            env.panic_with_error(EscrowError::InvalidParticipant);
        }
        if milestone_amounts.is_empty() {
            env.panic_with_error(EscrowError::EmptyMilestones);
        }

        let mut total_amount: i128 = 0;
        let mut milestones: Vec<Milestone> = Vec::new(&env);
        for amount in milestone_amounts.iter() {
            if amount <= 0 {
                env.panic_with_error(EscrowError::InvalidMilestoneAmount);
            }
            total_amount += amount;
            milestones.push_back(Milestone {
                amount,
                released: false,
                refunded: false,
            });
        }

        let id: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ContractCount)
            .unwrap_or(0u32);

        let data = EscrowContractData {
            client,
            freelancer,
            status: ContractStatus::Created,
            total_amount,
            funded_amount: 0,
            released_amount: 0,
            refunded_amount: 0,
        };

        env.storage().persistent().set(&DataKey::Contract(id), &data);
        env.storage()
            .persistent()
            .set(&DataKey::Milestones(id), &milestones);
        env.storage().persistent().set(&DataKey::ContractCount, &(id + 1));

        id
    }

    pub fn deposit_funds(env: Env, contract_id: u32, amount: i128) -> bool {
        if amount <= 0 {
            env.panic_with_error(EscrowError::InvalidDepositAmount);
        }

        let mut data = Self::load_contract(&env, contract_id);
        data.client.require_auth();
        Self::assert_open(&env, data.status);

        let new_funded = data.funded_amount + amount;
        if new_funded > data.total_amount {
            env.panic_with_error(EscrowError::DepositExceedsTotal);
        }

        data.funded_amount = new_funded;
        if data.status == ContractStatus::Created {
            data.status = ContractStatus::Funded;
        }
        Self::save_contract(&env, contract_id, &data);
        true
    }

    pub fn release_milestone(env: Env, contract_id: u32, milestone_index: u32) -> bool {
        let mut data = Self::load_contract(&env, contract_id);
        data.client.require_auth();
        Self::assert_open(&env, data.status);

        let mut milestones = Self::load_milestones(&env, contract_id);
        if milestone_index >= milestones.len() {
            env.panic_with_error(EscrowError::InvalidMilestone);
        }
        let mut m = milestones.get(milestone_index).unwrap();

        if m.released {
            env.panic_with_error(EscrowError::MilestoneAlreadyReleased);
        }
        if m.refunded {
            env.panic_with_error(EscrowError::MilestoneAlreadyRefunded);
        }
        if Self::available_balance(&data) < m.amount {
            env.panic_with_error(EscrowError::InsufficientEscrowBalance);
        }

        m.released = true;
        data.released_amount += m.amount;
        milestones.set(milestone_index, m);

        data.status = Self::derive_status(&data, &milestones);
        Self::save_contract(&env, contract_id, &data);
        Self::save_milestones(&env, contract_id, &milestones);
        true
    }

    pub fn refund_unreleased_milestones(
        env: Env,
        contract_id: u32,
        milestone_ids: Vec<u32>,
    ) -> i128 {
        if milestone_ids.is_empty() {
            env.panic_with_error(EscrowError::EmptyRefundRequest);
        }

        let mut data = Self::load_contract(&env, contract_id);
        data.client.require_auth();
        Self::assert_open(&env, data.status);

        let mut milestones = Self::load_milestones(&env, contract_id);
        let mut total_refund: i128 = 0;
        let mut seen: Vec<u32> = Vec::new(&env);

        for id in milestone_ids.iter() {
            if seen.contains(&id) {
                env.panic_with_error(EscrowError::DuplicateMilestone);
            }
            seen.push_back(id);

            if id >= milestones.len() {
                env.panic_with_error(EscrowError::InvalidMilestone);
            }
            let m = milestones.get(id).unwrap();
            if m.released {
                env.panic_with_error(EscrowError::MilestoneAlreadyReleased);
            }
            if m.refunded {
                env.panic_with_error(EscrowError::MilestoneAlreadyRefunded);
            }
            total_refund += m.amount;
        }

        if Self::available_balance(&data) < total_refund {
            env.panic_with_error(EscrowError::InsufficientEscrowBalance);
        }

        for id in seen.iter() {
            let mut m = milestones.get(id).unwrap();
            m.refunded = true;
            milestones.set(id, m);
        }
        data.refunded_amount += total_refund;

        data.status = Self::derive_status(&data, &milestones);
        Self::save_contract(&env, contract_id, &data);
        Self::save_milestones(&env, contract_id, &milestones);

        total_refund
    }

    pub fn get_contract(env: Env, contract_id: u32) -> EscrowContractData {
        Self::load_contract(&env, contract_id)
    }

    pub fn get_milestones(env: Env, contract_id: u32) -> Vec<Milestone> {
        Self::load_milestones(&env, contract_id)
    }

    fn load_contract(env: &Env, contract_id: u32) -> EscrowContractData {
        env.storage()
            .persistent()
            .get(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound))
    }

    fn load_milestones(env: &Env, contract_id: u32) -> Vec<Milestone> {
        env.storage()
            .persistent()
            .get(&DataKey::Milestones(contract_id))
            .unwrap_or_else(|| env.panic_with_error(EscrowError::ContractNotFound))
    }

    fn save_contract(env: &Env, contract_id: u32, data: &EscrowContractData) {
        env.storage()
            .persistent()
            .set(&DataKey::Contract(contract_id), data);
    }

    fn save_milestones(env: &Env, contract_id: u32, milestones: &Vec<Milestone>) {
        env.storage()
            .persistent()
            .set(&DataKey::Milestones(contract_id), milestones);
    }

    fn assert_open(env: &Env, status: ContractStatus) {
        match status {
            ContractStatus::Created | ContractStatus::Funded => {}
            _ => env.panic_with_error(EscrowError::InvalidStatus),
        }
    }

    fn available_balance(data: &EscrowContractData) -> i128 {
        data.funded_amount - data.released_amount - data.refunded_amount
    }

    fn derive_status(
        data: &EscrowContractData,
        milestones: &Vec<Milestone>,
    ) -> ContractStatus {
        let mut any_refunded = false;
        let mut all_settled = true;
        for m in milestones.iter() {
            if m.refunded {
                any_refunded = true;
            }
            if !m.released && !m.refunded {
                all_settled = false;
            }
        }
        if all_settled {
            if any_refunded {
                ContractStatus::Refunded
            } else {
                ContractStatus::Completed
            }
        } else if data.funded_amount > 0 {
            ContractStatus::Funded
        } else {
            ContractStatus::Created
        }
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod proptest;
