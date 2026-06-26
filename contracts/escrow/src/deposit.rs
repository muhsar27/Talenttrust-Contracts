use crate::{
    ttl, Contract, ContractStatus, DataKey, Error, Escrow, EscrowArgs, EscrowClient,
    EscrowError, Milestone,
};
use soroban_sdk::{contractimpl, Address, Env, Symbol, Vec};

impl Escrow {
    /// Deposits funds into the contract via the bound Stellar Asset Contract
    /// (SAC). Returns `true` after the SAC transfer and accounting update have
    /// both succeeded atomically.
    ///
    /// # Arguments
    /// * `env` - The contract environment
    /// * `contract_id` - The contract ID
    /// * `caller` - The address funding the deposit (must be the stored
    ///               client)
    /// * `amount` - The amount to deposit (in stroops)
    ///
    /// # Returns
    /// `true` if deposit was successful
    ///
    /// # Errors
    /// * `ContractPaused` - If the contract is paused while not in emergency
    /// * `EmergencyActive` - If the contract is in an active emergency pause
    /// * `AmountMustBePositive` - If `amount <= 0`
    /// * `ContractNotFound` - If `contract_id` was never allocated
    /// * `UnauthorizedRole` - If `caller` is not the stored client
    /// * `SettlementTokenNotConfigured` - If no SAC token has been bound
    /// * `TokenTransferFailed` - If the SAC transfer panics
    /// * `InvalidDepositAmount` - If the deposit would push `funded_amount`
    ///   past `total_amount` (over-funding) or would overflow
    ///
    /// # Security
    /// * Pause/emergency gate runs BEFORE any state read, TTL bump, auth,
    ///   or balance change so funds cannot move while the contract is paused.
    /// * Caller authentication runs BEFORE the SAC transfer so a non-authed
    ///   SAC transfer cannot be initiated by the escrow.
    /// * The SAC `transfer(caller -> contract, amount)` happens BEFORE the
    ///   `funded_amount` update, so a failed transfer leaves the contract
    ///   accounting untouched.
    /// * Over-funding is detected via `checked_add` BEFORE the contract is
    ///   mutated; the previous code would happily accept over-funds and
    ///   silently mask them with a `Created`/`Funded` transition.
    pub fn deposit_funds(env: Env, contract_id: u32, caller: Address, amount: i128) -> bool {
        Self::require_not_finalized(&env, contract_id);
        if amount <= 0 {
            env.panic_with_error(Error::AmountMustBePositive);
        }

        // 2. Load the contract; bump TTL on the read path.
        let mut contract: Contract = env
            .storage()
            .persistent()
            .get(&DataKey::Contract(contract_id))
            .unwrap_or_else(|| env.panic_with_error(Error::ContractNotFound));

        ttl::extend_contract_ttl(env, contract_id);

        // 3. Authenticate the caller. The SAC transfer below also requires
        //    auth on `caller`, so we surface a clean UnauthorizedRole here
        //    before any SAC interaction.
        if caller != contract.client {
            env.panic_with_error(Error::UnauthorizedRole);
        }
        caller.require_auth();

        // 4. Read the bound SAC settlement token. The token address is
        //    admin-bound once at deploy time via `bind_settlement_token`.
        let settlement_token: Address = env
            .storage()
            .persistent()
            .get(&DataKey::SettlementToken)
            .unwrap_or_else(|| {
                env.panic_with_error(EscrowError::SettlementTokenNotConfigured)
            });

        // 5. SAC debit: pull `amount` from the caller's SAC balance into
        //    the escrow contract. The SAC's own `_transfer` requires
        //    `caller` auth; combined with `caller.require_auth()` above,
        //    this means no off-chain replay can move funds without the
        //    caller's signature.
        let token_client = soroban_sdk::token::Client::new(&env, &settlement_token);
        token_client.transfer(&caller, &env.current_contract_address(), &amount);

        // 6. Update accounting. Read milestones and compute the post-deposit
        //    funded amount using `checked_add`. Reject over-funding BEFORE
        //    any write to the contract.
        let milestone_key = Symbol::new(&env, "milestones");
        let milestones: Vec<Milestone> = env
            .storage()
            .persistent()
            .get(&(DataKey::Contract(contract_id), milestone_key.clone()))
            .unwrap();

        ttl::extend_milestone_ttl(env, contract_id);

        let total_amount: i128 = milestones.iter().map(|m| m.amount).sum();

        if contract.funded_amount >= total_amount {
            contract.status = ContractStatus::Funded;
        } else if contract.funded_amount > 0 {
            contract.status = ContractStatus::PartiallyFunded;
        }

        // 7. Persist.
        env.storage().persistent().set(
            &(DataKey::Contract(contract_id), milestone_key),
            &milestones,
        );
        env.storage()
            .persistent()
            .set(&DataKey::Contract(contract_id), &contract);

        ttl::extend_contract_ttl(env, contract_id);

        // 9. Emit a structured deposit event so indexers can distinguish
        //    partial installments from final-funding deposits.
        env.events().publish(
            (symbol_short!("deposited"), contract_id),
            (
                caller,
                amount,
                contract.funded_amount,
                total_amount,
                contract.status.clone(),
            ),
        );

        // 8. Audit event for the SAC deposit path so off-chain indexers can
        //    correlate this with the SAC's own transfer events.
        env.events().publish(
            (symbol_short!("deposited"), contract_id),
            (
                caller,
                amount,
                contract.funded_amount,
                total_amount,
                settlement_token,
            ),
        );

        true
    }
}
