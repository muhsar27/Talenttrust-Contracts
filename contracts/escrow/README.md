# Escrow Contract

This is a Rust smart contract for the **Soroban** platform that implements a simple escrow system for freelancers and clients. Payments are handled via **milestones**, ensuring that funds are only released when work is completed and verified. 

---

## Features

- Create an escrow contract between a **client** and a **freelancer**.  
- Define multiple **milestones**, each with a set payment amount.  
- Deposit funds into escrow (only the client can do this).  
- Release milestone payments to the freelancer once verified.  
- Finalize contracts to enable leftover fund withdrawals.  
- **Withdraw leftover funds** after contract finalization (client only).  
- Issue a reputation score or credential to the freelancer after the contract is completed.  
- Fully tested with unit tests to ensure contract correctness.

---

## Leftover Fund Withdrawal

After a contract is finalized, the client can withdraw any remaining funds that were not released as milestone payments. This feature includes strict security invariants:

- **Only allowed after contract finalization**
- **Only the client can withdraw leftover funds**
- **Cannot withdraw more than available balance**
- **Prevents double withdrawals**
- **Emits events for transparency**

### Usage Example

```rust
// Create contract and deposit funds
let contract_id = client.create_contract(&client_addr, &freelancer_addr, &milestones);
client.deposit_funds(&contract_id, &1000, &client_addr);

// Release some milestones
client.release_milestone(&contract_id, &0, &client_addr);

// Finalize the contract
client.finalize_contract(&contract_id, &client_addr);

// Withdraw remaining funds
let leftover = client.withdraw_leftover(&contract_id, &client_addr);
```

---

## Security Notes

- Only the **client** is allowed to deposit funds.  
- Only the **freelancer** can receive payments for milestones.  
- Milestone amounts are validated to be **greater than zero**.  
- Non-existent contracts are safely handled to prevent panics.  
- Token transfers are skipped during testing to avoid unnecessary errors.  
- Always verify the addresses used when calling contract methods.  
- **Leftover withdrawals are protected by strict invariants** to prevent unauthorized access.  


