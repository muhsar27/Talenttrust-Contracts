use crate::{safe_add_amounts, ContractStatus, Contract as EscrowContractData, EscrowError, DisputeResolution};

use crate::{safe_add_amounts, Contract, ContractStatus, DataKey, Escrow, EscrowArgs, EscrowClient, Error};
use soroban_sdk::{contractimpl, symbol_short, Address, Env, Symbol};

// Removed obsolete duplicated `impl Escrow`

/// Resolution selected by the assigned arbiter for a disputed escrow.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeResolution {
    /// Refund all remaining escrowed funds to the client.
    FullRefund,
    /// Refund 70% of the remaining balance to the client and release 30% to the freelancer.
    PartialRefund,
    /// Release all remaining escrowed funds to the freelancer.
    FullPayout,
    /// Apply a custom split of the remaining balance.
    Split(i128, i128),
}

// Removed another obsolete copied chunk

impl DisputeResolution {
    pub fn code(&self) -> u32 {
        match self {
            Self::FullRefund => 0,
            Self::PartialRefund => 1,
            Self::FullPayout => 2,
            Self::Split(_, _) => 3,
        }
    }
}

#[allow(dead_code)]
pub fn resolution_payouts(
    contract: &EscrowContractData,
    resolution: &DisputeResolution,
) -> Result<(i128, i128), EscrowError> {
    let available = contract
        .total_deposited
        .checked_sub(contract.released_amount)
        .and_then(|value| value.checked_sub(contract.refunded_amount))
        .ok_or(EscrowError::AccountingInvariantViolated)?;
    if available < 0 {
        return Err(EscrowError::AccountingInvariantViolated);
    }

    match resolution {
        DisputeResolution::FullRefund => Ok((available, 0)),
        DisputeResolution::PartialRefund => {
            let freelancer_payout = available
                .checked_mul(30)
                .and_then(|value| value.checked_div(100))
                .ok_or(EscrowError::PotentialOverflow)?;
            Ok((available - freelancer_payout, freelancer_payout))
        }
        DisputeResolution::FullPayout => Ok((0, available)),
        DisputeResolution::Split(amounts) => {
            let client_amount = amounts.client_amount;
            let freelancer_amount = amounts.freelancer_amount;
            if client_amount < 0 || freelancer_amount < 0 {
                return Err(EscrowError::InvalidDisputeSplit);
            }
            let total = safe_add_amounts(client_amount, freelancer_amount)
                .ok_or(EscrowError::PotentialOverflow)?;
            if total != available {
                return Err(EscrowError::InvalidDisputeSplit);
            }
            Ok((client_amount, freelancer_amount))
        }
    }
}

pub fn final_status_after_resolution(contract: &EscrowContractData) -> ContractStatus {
    if contract.refunded_amount == contract.total_deposited {
        ContractStatus::Refunded
    } else {
        ContractStatus::Completed
    }
}
