use crate::{
    ttl, Contract, ContractStatus, DataKey, Error, Escrow, EscrowArgs, EscrowClient, Milestone,
    ReleaseAuthorization,
};
use soroban_sdk::{contractimpl, symbol_short, Address, Env, Symbol, Vec};
use crate::{MAX_MILESTONES, amount_validation, types::GovernedParameters};

#[contractimpl]
impl Escrow {
    /// Creates a new escrow contract with the specified client, freelancer, and milestone amounts.
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
    /// * `InvalidParticipant` - If client and freelancer are the same address
    /// * `EmptyMilestones` - If no milestones are provided
    /// * `InvalidMilestoneAmount` - If any milestone amount is <= 0
    /// * `MissingArbiter` - If arbiter is required but not provided
    /// * `InvalidArbiter` - If arbiter is same as client or freelancer
    /// * `TooManyMilestones` - If the number of milestones exceeds MAX_MILESTONES
    /// * `TotalCapExceeded` - If the sum of milestone amounts exceeds the governed cap
    /// * `ContractIdOverflow` - If the next id would exceed `u32::MAX`
    /// * `ContractIdCollision` - If the allocated id slot is already occupied
    pub fn create_contract(
        env: Env,
        client: Address,
        freelancer: Address,
        arbiter: Option<Address>,
        milestones: Vec<i128>,
        release_authorization: ReleaseAuthorization,
    ) -> u32 {
        client.require_auth();

        if client == freelancer {
            env.panic_with_error(Error::InvalidParticipant);
        }

        match release_authorization {
            ReleaseAuthorization::ArbiterOnly | ReleaseAuthorization::ClientAndArbiter
                if arbiter.is_none() =>
            {
                env.panic_with_error(Error::MissingArbiter);
            }
            _ => {}
        }

        if let Some(ref arb) = arbiter {
            if arb == &client || arb == &freelancer {
                env.panic_with_error(Error::InvalidArbiter);
            }
        }

        if milestones.is_empty() {
            env.panic_with_error(Error::EmptyMilestones);
        }

        // Enforce maximum number of milestones
        if milestones.len() > MAX_MILESTONES {
            env.panic_with_error(Error::TooManyMilestones);
        }

        // Retrieve governed parameters for total escrow cap
        let max_total = if let Some(params) = env.storage().persistent().get::<_, GovernedParameters>(&DataKey::GovernedParameters) {
            params.max_escrow_total_stroops
        } else {
            // If governance parameters are not set, allow any total (use max i128)
            i128::MAX
        };

        // Validate milestone amounts and total against caps
        let mut native_milestones = [0_i128; MAX_MILESTONES as usize];
        let len = milestones.len() as usize;
        for i in 0..len {
            native_milestones[i] = milestones.get(i as u32).unwrap();
        }
        match amount_validation::validate_milestone_amounts(&native_milestones[..len], max_total) {
            Ok(_) => (),
            Err(err) => match err {
                Error::InvalidMilestoneAmount => env.panic_with_error(Error::InvalidMilestoneAmount),
                Error::TotalCapExceeded => env.panic_with_error(Error::TotalCapExceeded),
                _ => env.panic_with_error(Error::InvalidMilestoneAmount),
            },
        }

        let id = next_contract_id(&env);

        ttl::extend_next_contract_id_ttl(&env);

        let freelancer_addr = freelancer.clone();
        let contract = Contract {
            client: client.clone(),
            freelancer: freelancer.clone(),
            arbiter,
            status: ContractStatus::Created,
            funded_amount: 0,
            released_amount: 0,
            refunded_amount: 0,
            release_authorization,
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
                deadline: None,
            });
        }
        let milestone_key = Symbol::new(&env, "milestones");
        env.storage()
            .persistent()
            .set(&(DataKey::Contract(id), milestone_key), &milestone_vec);

        env.storage()
            .persistent()
            .set(&DataKey::NextContractId, &(id + 1));

        env.events().publish(
            (symbol_short!("created"), id),
            (client, freelancer_addr, env.ledger().timestamp()),
        );

        id
    }
}

/// Returns the next contract id after verifying the slot is unused.
fn next_contract_id(env: &Env) -> u32 {
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

/// Advances [`DataKey::NextContractId`] after a contract is persisted.
#[allow(dead_code)]
fn bump_next_contract_id(env: &Env, id: u32) {
    let next_id = id
        .checked_add(1)
        .unwrap_or_else(|| env.panic_with_error(Error::ContractIdOverflow));
    env.storage()
        .persistent()
        .set(&DataKey::NextContractId, &next_id);
}
