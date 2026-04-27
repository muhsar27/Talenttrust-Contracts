# Reputation Credential Issuance

The Escrow contract issues reputation credentials (ratings) to freelancers upon the successful completion of a milestone or project. This module contains validations to ensure the integrity of the reputation system.

## Validation Rules

1. **Rating Bounds:** Must be between 1 and 5 (inclusive). Ratings outside these bounds will be rejected with an `InvalidRating` error.
2. **Comment Validation:** 
    - Max length: 1000 characters.
    - Cannot be empty (whitespace-only strings should be avoided, though the contract currently checks for length > 0).
    - Violation results in `CommentTooLong` or `EmptyComment` errors.
3. **Self-Rating Prevention:** Clients cannot rate themselves if they are also the freelancer in the same contract. Prevented by `SelfRating` check and also at contract creation via `InvalidParticipant`.
4. **Issuance Timing:** Credentials can only be issued if the project is completely finished (i.e. status is `Completed` or `Refunded`). If the project is in `Created`, `Funded`, or `Disputed` state, issuing ratings will fail with a `NotCompleted` error.
5. **Duplicate Prevention:** A freelancer can only receive exactly one rating credential per contract (project). Subsequent attempts to issue a rating will fail with a `DuplicateRating` error.

## Audit Trail

Every successful reputation update emits a `rated` event containing:
- `reviewer`: The address of the client who gave the rating.
- `target`: The address of the freelancer being rated.
- `rating`: The numeric score (1-5).
- `comment`: The optional text comment.
- `context_id`: The contract ID associated with the rating.

## Persistence

Ratings are persisted as `ReputationEntry` structs in the contract's persistent storage, mapping `DataKey::Reputation(contract_id, freelancer_address)` to the entry. This ensures an immutable audit trail of individual ratings alongside aggregate scores in `ReputationRecord`.

## Security Assumptions

- **Access Control:** `issue_reputation` requires the client's authentication.
- **Contract Completion:** Enforced by status check (`Completed` or `Refunded`).
- **Duplicate tracking state:** Prevented by storage key check.
- **Anti-Abuse:** Self-rating and out-of-bounds ratings are natively blocked.

## Threat Scenarios

- **Duplicate rating attack:** Attackers or clients attempting to unfairly inflate or deflate a freelancer's score by rating repeatedly on the same job. Prevented by checking the reputation map before issuance.
- **Early rating attack:** Clients attempting to lock in a rating or rate negatively prematurely before finishing escrow obligations. Prevented by enforcing the `Completed` state as an issuance prerequisite.
- **Out-of-bounds rating attack:** Attackers attempting to provide extremely high ratings to manipulate global average calculations. Prevented by enforcing the `1 <= rating <= 5` boundary natively in the Escrow contract.
