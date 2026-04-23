# Governance Module

The Governance contract provides on-chain decision-making primitives for the Stellopay ecosystem. it enables stakeholders to propose, vote on, and execute changes such as parameter adjustments, contract upgrades, and arbiter changes.

## Proposal Lifecycle

1.  **Propose**: Any address with non-zero voting power can create a proposal. The voting power of all participants is snapshotted at the moment of proposal creation to prevent vote inflation via power transfers during the process.
2.  **Vote**: Holders of voting power cast votes (`For`, `Against`, or `Abstain`). Each address can vote only once per proposal.
3.  **Queue**: Once the voting period ends, any user can trigger the `queue` function. The contract calculates if the quorum was met and if the proposal was approved (`For > Against`). If successful, the proposal enters a **Timelock** state.
4.  **Execute**: After the timelock expires, the proposal can be executed. Execution must occur within the **Execution Window**. If this window passes, the proposal is marked as `Expired` and cannot be executed.
5.  **Cancel**: The owner can cancel a proposal at any time before it is executed, providing an emergency guardrail.

## Configuration

| Parameter | Description |
| :--- | :--- |
| `Quorum Bps` | Minimum percentage of total voting power required for a proposal to be valid (in basis points, e.g., 5000 = 50%). |
| `Voting Period` | Duration (in seconds) that a proposal is open for voting. |
| `Timelock` | Forced delay (in seconds) between proposal success and execution. |
| `Execution Window` | Duration (in seconds) after the timelock during which a proposal must be executed. |

## Timelock Integration Pattern

The Governance contract integrates with the `withdrawal_timelock` contract to provide a two-stage safety net for sensitive operations like admin changes:

### Integration Flow

1. **Governance Proposal**: Create a proposal for an admin change or other sensitive operation
2. **Vote & Queue**: Standard governance voting and queuing process
3. **Timelock Queue**: After proposal success, queue the operation in the withdrawal_timelock contract
4. **Second Timelock**: The withdrawal_timelock enforces an additional delay
5. **Execution**: After both timelocks expire, execute the operation

### Payload Hash Derivation

For admin change operations, the payload hash is computed deterministically:

```rust
fn create_admin_change_payload_hash(
    env: &Env,
    target_contract: &Address,
    new_admin: &Address,
    nonce: u64,
) -> BytesN<32> {
    let mut payload = Vec::new(env);
    
    // Domain separation prefix
    payload.push_back(Symbol::new(env, "ADMIN_CHANGE").to_val());
    
    // Target contract address
    payload.push_back(target_contract.to_val());
    
    // New admin address  
    payload.push_back(new_admin.to_val());
    
    // Nonce for uniqueness
    payload.push_back(nonce.to_val());
    
    // Compute SHA-256 hash
    env.crypto().sha256(&payload)
}
```

### Security Benefits

- **Double Timelock**: Governance timelock + withdrawal_timelock delay
- **Domain Separation**: Payload hashes include operation type to prevent collisions
- **Deterministic Verification**: Off-chain tooling can verify payload hashes
- **Access Control**: Only authorized governance execution can queue timelock ops

## Security Assumptions

-   **Double Execution**: Prevented by state transitions; once a proposal is `Executed`, it cannot be triggered again.
-   **Vote Inflation**: Prevented by using the snapshot of voting power taken at proposal creation.
-   **Late Votes**: Disallowed by strict timestamp checks against `end_time`.
-   **Execution Guards**: Timelocks ensure stakeholders have time to react to approved changes, and execution windows prevent stale proposals from being executed unexpectedly.
-   **Timelock Integration**: Admin changes require both governance approval and withdrawal_timelock queuing, providing defense-in-depth.
-   **Payload Hash Security**: Domain separation prevents hash collision attacks across different operation types.
