use soroban_sdk::{contractimpl, Address, Env};

use crate::{DataKey, EscrowError, GovernedParameters};

#[contractimpl]
impl super::Escrow {
    /// Set governance-controlled deployment parameters.
    ///
    /// This admin-only entrypoint persists the governed parameters and flips
    /// `ReadinessChecklist.governed_params_set` to `true`.
    pub fn set_governed_params(
        env: Env,
        admin: Address,
        protocol_fee_bps: u32,
        max_escrow_total_stroops: i128,
    ) -> bool {
        Self::require_initialized(&env);
        Self::require_admin(&env, &admin);

        if protocol_fee_bps > 10_000 {
            env.panic_with_error(EscrowError::InvalidProtocolParameters);
        }
        if max_escrow_total_stroops <= 0 {
            env.panic_with_error(EscrowError::InvalidProtocolParameters);
        }
        if max_escrow_total_stroops > super::MAX_TOTAL_ESCROW_STROOPS {
            env.panic_with_error(EscrowError::InvalidProtocolParameters);
        }

        let params = GovernedParameters {
            protocol_fee_bps,
            max_escrow_total_stroops,
        };

        env.storage()
            .persistent()
            .set(&DataKey::GovernedParameters, &params);

        let mut checklist = Self::load_checklist(&env);
        checklist.governed_params_set = true;
        env.storage()
            .persistent()
            .set(&DataKey::ReadinessChecklist, &checklist);

        true
    }

    /// Returns the currently configured governed parameters, if any.
    pub fn get_governed_parameters(env: Env) -> Option<GovernedParameters> {
        env.storage().persistent().get(&DataKey::GovernedParameters)
    }
}
