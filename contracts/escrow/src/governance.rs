use crate::{DataKey, EscrowError, EscrowClient, EscrowArgs, Escrow};
use soroban_sdk::{symbol_short, Address, Env, Symbol};

#[soroban_sdk::contractimpl]
impl Escrow {
    pub fn set_protocol_fee_bps(env: Env, admin: Address, new_bps: u32) -> bool {
        if !env.storage().persistent().get::<_, bool>(&DataKey::Initialized).unwrap_or(false) {
            env.panic_with_error(EscrowError::NotInitialized);
        }
        let stored_admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap_or_else(|| env.panic_with_error(EscrowError::NotInitialized));
        if admin != stored_admin {
            env.panic_with_error(EscrowError::UnauthorizedRole);
        }
        admin.require_auth();

        let old_bps: u32 = env.storage().persistent().get(&DataKey::ProtocolFeeBps).unwrap_or(0u32);
        env.storage().persistent().set(&DataKey::ProtocolFeeBps, &new_bps);

        env.events().publish(
            (Symbol::new(&env, "protocol_fee_bps"),),
            (old_bps, new_bps, admin.clone(), env.ledger().timestamp()),
        );
        true
    }

    pub fn propose_governance_admin(env: Env, admin: Address, proposed: Address) -> bool {
        if !env.storage().persistent().get::<_, bool>(&DataKey::Initialized).unwrap_or(false) {
            env.panic_with_error(EscrowError::NotInitialized);
        }
        let stored_admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap_or_else(|| env.panic_with_error(EscrowError::NotInitialized));
        if admin != stored_admin {
            env.panic_with_error(EscrowError::UnauthorizedRole);
        }
        admin.require_auth();
        env.storage().persistent().set(&DataKey::PendingAdmin, &proposed);
        env.events().publish(
            (symbol_short!("admin"), Symbol::new(&env, "proposed")),
            (admin, proposed.clone(), env.ledger().timestamp()),
        );
        true
    }

    pub fn accept_governance_admin(env: Env, proposed_admin: Address) -> bool {
        if !env.storage().persistent().get::<_, bool>(&DataKey::Initialized).unwrap_or(false) {
            env.panic_with_error(EscrowError::NotInitialized);
        }
        let pending: Address = env.storage().persistent().get(&DataKey::PendingAdmin).unwrap_or_else(|| env.panic_with_error(EscrowError::InvalidState));
        if proposed_admin != pending {
            env.panic_with_error(EscrowError::UnauthorizedRole);
        }
        proposed_admin.require_auth();

        let old_admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap_or_else(|| env.panic_with_error(EscrowError::NotInitialized));
        env.storage().persistent().set(&DataKey::Admin, &pending);
        env.storage().persistent().remove(&DataKey::PendingAdmin);

        env.events().publish(
            (symbol_short!("admin"), Symbol::new(&env, "accepted")),
            (old_admin, pending.clone(), env.ledger().timestamp()),
        );
        true
    }

    pub fn get_pending_governance_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::PendingAdmin)
    }

    pub fn get_governance_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Admin)
    }

    /// Returns the current protocol fee in basis points.
    pub fn get_protocol_fee_bps(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get::<_, u32>(&DataKey::ProtocolFeeBps)
            .unwrap_or(0)
    }

    /// Returns the total accumulated protocol fees.
    pub fn get_accumulated_protocol_fees(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get::<_, i128>(&DataKey::AccumulatedProtocolFees)
            .unwrap_or(0)
    }
}
