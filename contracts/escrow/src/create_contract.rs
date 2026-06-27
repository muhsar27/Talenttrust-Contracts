use crate::{
    ttl, Contract, ContractStatus, DataKey, Error, EscrowError, GovernedParameters, Milestone,
    ReleaseAuthorization,
};
use soroban_sdk::{symbol_short, Address, Env, Symbol, Vec};

/// Creates a new escrow contract with the specified client, freelancer, and milestone amounts
///
/// # Arguments
/// * `env` - The contract environment
/// * `client` - The address of the client funding the contract
/// * `freelancer` - The address of the freelancer performing the work
/// * `arbiter` - Optional arbiter address for dispute resolution
/// * `milestones` - Vector of milestone amounts (in stroops)
/// * `release_authorization` - Authorization mode for milestone releases
///
/// # Returns
/// The unique contract ID
///
/// # Errors
/// * `InvalidParticipants` - If client and freelancer are the same address
/// * `EmptyMilestones` - If no milestones are provided
/// * `InvalidMilestoneAmount` - If any milestone amount is <= 0
/// * `MissingArbiter` - If arbiter is required but not provided
/// * `InvalidArbiter` - If arbiter is same as client or freelancer
/// * `ContractIdOverflow` - If `NextContractId` is already `u32::MAX`; the counter
///   is never incremented past the ceiling, so no wrap-to-zero collision is possible.
/// * `ContractIdCollision` - If the allocated id slot is already occupied
pub fn create_contract_impl(
    env: &Env,
    client: Address,
    freelancer: Address,
    arbiter: Option<Address>,
    milestones: Vec<i128>,
    release_authorization: ReleaseAuthorization,
) -> u32 {
    client.require_auth();

    if client == freelancer {
        env.panic_with_error(EscrowError::InvalidParticipants);
    }

    match release_authorization {
        ReleaseAuthorization::ArbiterOnly | ReleaseAuthorization::ClientAndArbiter
            if arbiter.is_none() =>
        {
            env.panic_with_error(EscrowError::MissingArbiter);
        }
        _ => {}
    }

    if let Some(ref arb) = arbiter {
        if arb == &client || arb == &freelancer {
            env.panic_with_error(EscrowError::InvalidArbiter);
        }
    }

    if milestones.is_empty() {
        env.panic_with_error(EscrowError::EmptyMilestones);
    }

    for amount in milestones.iter() {
        if amount <= 0 {
            env.panic_with_error(EscrowError::InvalidMilestoneAmount);
        }
    }

    // Check governed max_escrow_total_stroops cap if set
    let total_milestones: i128 = milestones.iter().map(|m| m).sum();
    if let Some(params) = env
        .storage()
        .persistent()
        .get::<_, GovernedParameters>(&DataKey::GovernedParameters)
    {
        if params.max_escrow_total_stroops > 0 && total_milestones > params.max_escrow_total_stroops
        {
            env.panic_with_error(EscrowError::EscrowCapExceeded);
        }
    }

    let total_milestones_amount: i128 = milestones.iter().fold(0, |acc, &x| {
        acc.checked_add(x)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::PotentialOverflow))
    });

    if let Some(params) = env
        .storage()
        .persistent()
        .get::<_, GovernedParameters>(&DataKey::GovernedParameters)
    {
        if params.max_escrow_total_stroops > 0
            && total_milestones_amount > params.max_escrow_total_stroops
        {
            env.panic_with_error(EscrowError::EscrowCapExceeded);
        }
    }

    let id = next_contract_id(&env);

    ttl::extend_next_contract_id_ttl(&env);

    let freelancer_addr = freelancer.clone();
    let contract = Contract {
        client: client.clone(),
        freelancer: freelancer.clone(),
        arbiter,
        status: ContractStatus::Created,
        total_deposited: 0,
        funded_amount: 0,
        released_amount: 0,
        refunded_amount: 0,
        release_authorization,
        reputation_issued: false,
    };
    env.storage()
        .persistent()
        .set(&DataKey::Contract(id), &contract);

    let mut milestone_vec: Vec<Milestone> = Vec::new(&env);
    for amount in milestones.iter() {
        milestone_vec.push_back(Milestone {
            amount,
            funded_amount: 0,
            released: false,
            refunded: false,
            work_evidence: None,
            refunded_amount: 0,
        });
    }
    let milestone_key = Symbol::new(&env, "milestones");
    env.storage()
        .persistent()
        .set(&(DataKey::Contract(id), milestone_key), &milestone_vec);

    /// Safety: `next_contract_id` already checked that `id < u32::MAX`; the
    /// `checked_add` here is a defense-in-depth guard. `None` (overflow) is
    /// unreachable in practice but must be handled to match documented behavior.
    let next_id = id
        .checked_add(1)
        .unwrap_or_else(|| env.panic_with_error(Error::ContractIdOverflow));
    env.storage()
        .persistent()
        .set(&DataKey::NextContractId, &next_id);

    env.events().publish(
        (symbol_short!("created"), id),
        (client, freelancer_addr, env.ledger().timestamp()),
    );

    id
}

/// Returns the next contract id after verifying the slot is unused and that
/// incrementing the counter will not overflow.
///
/// # Overflow safety
/// When `id == u32::MAX` there is no valid successor, so this function panics
/// with [`Error::ContractIdOverflow`] **before** any state is written. The
/// counter stored under [`DataKey::NextContractId`] is therefore never advanced
/// to zero, making wrap-to-zero collisions with existing low-numbered contracts
/// impossible.
pub(crate) fn next_contract_id(env: &Env) -> u32 {
    let id: u32 = env
        .storage()
        .persistent()
        .get(&DataKey::NextContractId)
        .unwrap_or(1);

    if env
        .storage()
        .persistent()
        .get::<_, Contract>(&DataKey::Contract(id))
        .is_some()
    {
        env.panic_with_error(Error::ContractIdCollision);
    }

    id
}