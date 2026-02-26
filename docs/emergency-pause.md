# Emergency Pause Functionality

## Overview

The emergency pause system provides a critical safety mechanism to halt all contract operations in case of security incidents, bugs, or other emergencies. It supports both immediate owner-initiated pauses and multi-signature guardian-based pauses with optional timelocks.

## Features

### 1. Immediate Owner Pause
- Contract owner can instantly pause all operations
- No approval required
- Used for critical emergencies requiring immediate action

### 2. Multi-Signature Guardian Pause
- Multiple guardians can propose and approve pause actions
- Requires majority approval (threshold = guardians/2 + 1)
- Provides decentralized emergency response
- Prevents single point of failure

### 3. Time-Locked Activation
- Optional delay before pause takes effect
- Allows time for verification and community awareness
- Configurable timelock duration (0 = immediate)
- Prevents hasty decisions

### 4. Granular State Tracking
- Records who initiated the pause
- Timestamps for audit trail
- Tracks approval status
- Maintains timelock information

## Architecture

### Storage Structure

```rust
pub struct EmergencyPause {
    pub is_paused: bool,           // Current pause state
    pub paused_at: Option<u64>,    // Timestamp when paused
    pub paused_by: Option<Address>, // Who initiated the pause
    pub timelock_end: Option<u64>,  // When timelock expires
}
```

### Storage Keys
- `EmergencyPause` - Current pause state
- `EmergencyGuardians` - List of authorized guardians
- `PendingPause` - Proposed pause awaiting approval
- `PauseApprovals` - Addresses that approved pending pause

## Usage

### Setting Up Guardians

```rust
// Owner sets emergency guardians
let guardians = vec![guardian1, guardian2, guardian3];
contract.set_emergency_guardians(guardians);
```

**Requirements:**
- Only contract owner can set guardians
- Recommended: 3-5 guardians for optimal security/responsiveness balance
- Guardians should be trusted entities or multi-sig wallets

### Owner Emergency Pause

```rust
// Immediate pause by owner
contract.emergency_pause()?;
```

**Use Cases:**
- Critical vulnerability discovered
- Active exploit in progress
- Smart contract bug detected
- Immediate risk to user funds

### Guardian-Proposed Pause

```rust
// Guardian proposes pause with 1-hour timelock
contract.propose_emergency_pause(guardian1, 3600)?;

// Other guardians approve
contract.approve_emergency_pause(guardian2)?;
// Pause activates when threshold reached and timelock expires
```

**Approval Threshold:**
- Calculated as: `(number_of_guardians / 2) + 1`
- Examples:
  - 3 guardians → 2 approvals needed
  - 5 guardians → 3 approvals needed
  - 7 guardians → 4 approvals needed

### Unpausing

```rust
// Owner unpauses after issue resolved
contract.emergency_unpause()?;
```

**Requirements:**
- Only owner can unpause
- Should verify issue is fully resolved
- Consider announcing unpause to community

## Security Considerations

### Access Control

1. **Owner Powers:**
   - Immediate pause/unpause
   - Set/modify guardians
   - Final authority on contract state

2. **Guardian Powers:**
   - Propose emergency pause
   - Approve pending pause proposals
   - Cannot unpause (owner only)

3. **User Impact:**
   - All claims blocked during pause
   - Agreement creation blocked
   - Read operations still available

### Best Practices

1. **Guardian Selection:**
   - Choose diverse, trusted entities
   - Consider geographic distribution
   - Use multi-sig wallets as guardians
   - Regularly verify guardian availability

2. **Timelock Usage:**
   - Use 0 timelock for critical emergencies
   - Use 1-24 hour timelock for non-critical issues
   - Longer timelocks for planned maintenance

3. **Communication:**
   - Announce pause immediately via all channels
   - Explain reason for pause
   - Provide estimated resolution time
   - Update community regularly

4. **Recovery Procedures:**
   - Document incident thoroughly
   - Verify fix before unpausing
   - Consider gradual re-enabling of features
   - Post-mortem analysis

## Error Codes

| Error | Code | Description |
|-------|------|-------------|
| `EmergencyPaused` | 24 | Operation blocked due to emergency pause |
| `NotGuardian` | 25 | Caller is not an authorized guardian |
| `TimelockActive` | 26 | Timelock period has not expired |
| `InvalidTimelock` | 27 | Invalid timelock duration specified |

## Integration Examples

### Checking Pause State

```rust
// Before critical operations
if contract.is_emergency_paused() {
    return Err(PayrollError::EmergencyPaused);
}
```

### Monitoring Pause Events

```rust
// Get detailed pause state
let state = contract.get_emergency_pause_state()?;
if state.is_paused {
    log!("Contract paused at: {}", state.paused_at.unwrap());
    log!("Paused by: {}", state.paused_by.unwrap());
}
```

### Guardian Management

```rust
// Check current guardians
let guardians = contract.get_emergency_guardians()?;
log!("Active guardians: {}", guardians.len());
```

## Testing

Comprehensive test coverage includes:

1. **Basic Functionality:**
   - Owner pause/unpause
   - Guardian setup
   - State queries

2. **Multi-Sig Workflow:**
   - Proposal creation
   - Approval threshold
   - Duplicate approval handling

3. **Timelock Mechanism:**
   - Timelock enforcement
   - Expiration handling

4. **Operational Impact:**
   - Claims blocked when paused
   - Milestone claims blocked
   - Functionality restored after unpause

5. **Edge Cases:**
   - Invalid guardian attempts
   - Timelock edge cases
   - State consistency

Run tests:
```bash
cd onchain/contracts/stello_pay_contract
cargo test test_emergency_pause
```

## Emergency Response Workflow

### Phase 1: Detection
1. Security incident identified
2. Assess severity and impact
3. Determine if pause is necessary

### Phase 2: Activation
**Critical (Immediate):**
```
Owner → emergency_pause() → All operations halted
```

**Non-Critical (Deliberate):**
```
Guardian1 → propose_emergency_pause(timelock)
Guardian2+ → approve_emergency_pause()
System → Pause activates after timelock
```

### Phase 3: Investigation
1. Analyze root cause
2. Develop fix
3. Test thoroughly
4. Prepare deployment

### Phase 4: Resolution
1. Deploy fix (if needed)
2. Verify system integrity
3. Owner calls `emergency_unpause()`
4. Monitor for issues

### Phase 5: Post-Mortem
1. Document incident
2. Update procedures
3. Improve monitoring
4. Communicate learnings

## Monitoring and Alerts

### Recommended Monitoring

1. **Pause State:**
   - Alert on any pause activation
   - Track pause duration
   - Monitor unpause events

2. **Guardian Activity:**
   - Log all proposals
   - Track approval patterns
   - Alert on unusual activity

3. **Failed Operations:**
   - Count EmergencyPaused errors
   - Track affected users
   - Monitor retry attempts

### Integration with External Systems

```rust
// Example monitoring integration
if let Some(state) = contract.get_emergency_pause_state() {
    if state.is_paused {
        alert_system.send_critical(
            "Contract Emergency Pause Active",
            format!("Paused at: {}, By: {}", 
                state.paused_at.unwrap(),
                state.paused_by.unwrap()
            )
        );
    }
}
```

## Upgrade Considerations

When upgrading the contract:

1. **Preserve Pause State:**
   - Migrate emergency pause data
   - Maintain guardian list
   - Preserve pending proposals

2. **Backward Compatibility:**
   - Ensure pause checks remain consistent
   - Maintain error code values
   - Keep storage key structure

3. **Testing:**
   - Verify pause works post-upgrade
   - Test guardian functionality
   - Validate state migration

## Compliance and Auditing

### Audit Trail

All emergency pause actions are recorded:
- Timestamp of pause/unpause
- Address that initiated action
- Approval history for multi-sig pauses
- Timelock parameters

### Regulatory Considerations

- Emergency pause provides required "circuit breaker" functionality
- Demonstrates risk management controls
- Supports compliance with financial regulations
- Enables rapid response to regulatory requests

## FAQ

**Q: Can guardians unpause the contract?**  
A: No, only the owner can unpause. This prevents premature resumption.

**Q: What happens to pending transactions during pause?**  
A: All state-changing operations fail with `EmergencyPaused` error. Read operations continue.

**Q: Can the owner override a timelock?**  
A: Yes, the owner can use `emergency_pause()` for immediate pause regardless of pending proposals.

**Q: How long should a pause last?**  
A: Only as long as necessary to resolve the issue. Communicate expected duration to users.

**Q: Can guardians be changed during a pause?**  
A: Yes, the owner can modify guardians at any time.

**Q: What if all guardians are unavailable?**  
A: The owner retains ability to pause immediately. Consider this when selecting guardians.

## Related Documentation

- [Multisig Documentation](./multisig.md)
- [Security Best Practices](./best-practices/README.md)
- [Error Handling](./error-handling.md)
- [Upgrade Procedures](./migrations.md)
