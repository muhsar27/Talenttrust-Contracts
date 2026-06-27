use crate::{DataKey, Escrow, EscrowArgs, EscrowClient, Error, GovernedParameters, ReadinessChecklist};
use soroban_sdk::{contractimpl, symbol_short, Address, Env, Symbol};

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

    // ── Two-step admin transfer ───────────────────────────────────────────────

    /// Propose a new governance admin. Stores the proposal with a timelock.
    ///
    /// # Events
    /// `(symbol_short!("admin"), Symbol("proposed"))` → `(admin, proposed, timestamp)`
    pub(crate) fn propose_governance_admin_impl(env: &Env, proposed: Address) -> bool {
        Self::require_initialized(env);

        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| env.panic_with_error(Error::ContractNotFound));
        admin.require_auth();

        env.storage().persistent().set(
            &DataKey::PendingAdmin,
            &PendingAdminProposal {
                proposed: proposed.clone(),
                proposed_at_ledger: env.ledger().sequence(),
            },
        );

        env.events().publish(
            (symbol_short!("admin"), Symbol::new(env, "proposed")),
            (admin, proposed.clone(), env.ledger().timestamp()),
        );
        true
    }

    /// Accept a pending admin proposal, enforcing the timelock.
    ///
    /// # Events
    /// `(symbol_short!("admin"), Symbol("accepted"))` → `(old_admin, new_admin, timestamp)`
    pub(crate) fn accept_governance_admin_impl(env: &Env) -> bool {
        Self::require_initialized(env);

        let pending: PendingAdminProposal = env
            .storage()
            .persistent()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| env.panic_with_error(Error::InvalidState));

        let elapsed = env
            .ledger()
            .sequence()
            .saturating_sub(pending.proposed_at_ledger);
        if elapsed < ADMIN_ROTATION_MIN_DELAY_LEDGERS {
            env.panic_with_error(Error::TimelockNotElapsed);
        }

        let pending_admin = pending.proposed;
        pending_admin.require_auth();

        let old_admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| env.panic_with_error(Error::ContractNotFound));

        env.storage()
            .persistent()
            .set(&DataKey::Admin, &pending_admin);
        env.storage().persistent().remove(&DataKey::PendingAdmin);

        env.events().publish(
            (symbol_short!("admin"), Symbol::new(env, "accepted")),
            (old_admin, pending_admin.clone(), env.ledger().timestamp()),
        );
        true
    }

    /// Internal: return the currently pending admin address, if any.
    pub(crate) fn get_pending_governance_admin_impl(env: Env) -> Option<Address> {
        let proposal: Option<PendingAdminProposal> =
            env.storage().persistent().get(&DataKey::PendingAdmin);
        proposal.map(|p| p.proposed)
    }

    /// Internal: return the current admin address.
    pub(crate) fn get_governance_admin_impl(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Admin)
    }

    /// Set both governance parameters at once and update the readiness checklist.
    pub fn set_governed_params(
        env: Env,
        admin: Address,
        protocol_fee_bps: u32,
        max_escrow_total_stroops: i128,
    ) -> bool {
        if !env
            .storage()
            .persistent()
            .get::<_, bool>(&crate::DataKey::Initialized)
            .unwrap_or(false)
        {
            env.panic_with_error(Error::NotInitialized);
        }

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| env.panic_with_error(Error::NotInitialized));

        if admin != stored_admin {
            env.panic_with_error(Error::UnauthorizedRole);
        }
        admin.require_auth();

        if protocol_fee_bps > 10_000 {
            env.panic_with_error(Error::InvalidProtocolParameters);
        }

        let params = GovernedParameters {
            protocol_fee_bps,
            max_escrow_total_stroops,
        };
        env.storage()
            .persistent()
            .set(&DataKey::GovernedParameters, &params);

        let mut checklist: ReadinessChecklist = env
            .storage()
            .persistent()
            .get(&DataKey::ReadinessChecklist)
            .unwrap_or_default();
        checklist.governed_params_set = true;
        env.storage()
            .persistent()
            .set(&DataKey::ReadinessChecklist, &checklist);

        true
    }

    /// Retrieve the current governed parameters.
    pub fn get_governed_parameters(env: Env) -> Option<GovernedParameters> {
        env.storage().persistent().get(&DataKey::GovernedParameters)
    }
}
