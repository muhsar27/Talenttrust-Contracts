use crate::ttl::{read_if_live, remove_transient, store_with_ttl, PENDING_MIGRATION_TTL_LEDGERS};
use crate::{Contract, ContractStatus, DataKey, Escrow, Error};
use soroban_sdk::{contracttype, Address, Env, Symbol};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingClientMigration {
    pub current_client: Address,
    pub proposed_client: Address,
    pub requested_at_ledger: u32,
    pub expires_at_ledger: u32,
}

#[contractimpl]
impl Escrow {
    fn pending_migration_key(contract_id: u32) -> DataKey {
        DataKey::PendingClientMigration(contract_id)
    }

    fn load_contract(env: &Env, contract_id: u32) -> EscrowContractData {
        env.storage()
            .persistent()
            .get::<_, Contract>(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(Error::ContractNotFound))
    }

    fn require_migration_allowed(env: &Env, status: ContractStatus) {
        if matches!(
            status,
            ContractStatus::Completed
                | ContractStatus::Refunded
                | ContractStatus::Disputed
        ) {
            env.panic_with_error(Error::InvalidStatusTransition);
        }
    }

    fn pending_migration_exists(env: &Env, contract_id: u32) -> bool {
        read_if_live::<_, PendingClientMigration>(env, &DataKey::PendingClientMigration(contract_id))
            .is_some()
    }

    pub fn propose_client_migration(
        env: Env,
        contract_id: u32,
        current_client: Address,
        new_client: Address,
    ) -> bool {
        Self::require_not_paused(&env);
        current_client.require_auth();

    let contract = Escrow::load_contract(&env, contract_id);
    Escrow::require_not_finalized(&env, contract_id);
    if current_client != contract.client {
        env.panic_with_error(Error::UnauthorizedRole);
    }
    if new_client == contract.client || new_client == contract.freelancer {
        env.panic_with_error(Error::InvalidParticipant);
    }
    Escrow::require_migration_allowed(&env, contract.status);
    if Escrow::pending_migration_exists(&env, contract_id) {
        env.panic_with_error(Error::InvalidState);
    }

    pub fn accept_client_migration(env: Env, contract_id: u32, new_client: Address) -> bool {
        Self::require_not_paused(&env);
        new_client.require_auth();

        let mut contract = Self::load_contract(&env, contract_id);
        Self::require_migration_allowed(&env, contract.status);

        let key = Self::pending_migration_key(contract_id);
        let pending: PendingClientMigration = read_if_live(&env, &key)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::InvalidState));

        if pending.proposed_client != new_client {
            env.panic_with_error(EscrowError::UnauthorizedRole);
        }
        if pending.current_client != contract.client {
            env.panic_with_error(EscrowError::InvalidState);
        }

    let key = Escrow::pending_migration_key(contract_id);
    let pending: PendingClientMigration =
        read_if_live(&env, &key).unwrap_or_else(|| env.panic_with_error(Error::InvalidState));

    if pending.proposed_client != new_client {
        env.panic_with_error(Error::UnauthorizedRole);
    }
    if pending.current_client != contract.client {
        env.panic_with_error(Error::InvalidState);
    }

    pub fn has_pending_client_migration(env: Env, contract_id: u32) -> bool {
        Self::pending_migration_exists(&env, contract_id)
    }

    env.events().publish(
        (Symbol::new(&env, "client_migration_accepted"), contract_id),
        (pending.current_client, new_client, env.ledger().timestamp()),
    );
    true
}

/// Return true if a live pending client migration exists.
pub fn has_pending_client_migration_impl(env: &Env, contract_id: u32) -> bool {
    Escrow::pending_migration_exists(&env, contract_id)
}

/// Return the live pending client migration record.
pub fn get_pending_client_migration_impl(env: &Env, contract_id: u32) -> PendingClientMigration {
    read_if_live(&env, &Escrow::pending_migration_key(contract_id))
        .unwrap_or_else(|| env.panic_with_error(Error::InvalidState))
}
