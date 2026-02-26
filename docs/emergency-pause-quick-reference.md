# Emergency Pause Quick Reference

## Contract Functions

### Setup
```rust
// Set emergency guardians (owner only)
set_emergency_guardians(guardians: Vec<Address>)

// Get current guardians
get_emergency_guardians() -> Option<Vec<Address>>
```

### Pause Operations
```rust
// Owner immediate pause
emergency_pause() -> Result<(), PayrollError>

// Guardian propose pause with timelock
propose_emergency_pause(caller: Address, timelock_seconds: u64) -> Result<(), PayrollError>

// Guardian approve pending pause
approve_emergency_pause(caller: Address) -> Result<(), PayrollError>

// Owner unpause
emergency_unpause() -> Result<(), PayrollError>
```

### Status Queries
```rust
// Check if paused
is_emergency_paused() -> bool

// Get detailed state
get_emergency_pause_state() -> Option<EmergencyPause>
```

## Error Codes

| Code | Error | Description |
|------|-------|-------------|
| 24 | EmergencyPaused | Operation blocked due to emergency pause |
| 25 | NotGuardian | Caller is not an authorized guardian |
| 26 | TimelockActive | Timelock period has not expired yet |
| 27 | InvalidTimelock | Invalid timelock duration specified |

## Common Scenarios

### Scenario 1: Critical Emergency (Owner)
```rust
// Immediate pause
contract.emergency_pause()?;

// ... fix issue ...

// Unpause
contract.emergency_unpause()?;
```

### Scenario 2: Non-Critical Issue (Multi-Sig)
```rust
// Guardian 1 proposes with 1-hour delay
contract.propose_emergency_pause(guardian1, 3600)?;

// Guardian 2 approves (reaches 2/3 threshold)
contract.approve_emergency_pause(guardian2)?;

// After timelock expires, pause activates automatically

// ... fix issue ...

// Owner unpauses
contract.emergency_unpause()?;
```

### Scenario 3: Check Before Operation
```rust
if contract.is_emergency_paused() {
    return Err(PayrollError::EmergencyPaused);
}
// Proceed with operation
```

## Guardian Threshold Calculation

| Guardians | Threshold | Example |
|-----------|-----------|---------|
| 2 | 2 | Both must approve |
| 3 | 2 | 2 out of 3 |
| 4 | 3 | 3 out of 4 |
| 5 | 3 | 3 out of 5 |
| 7 | 4 | 4 out of 7 |

Formula: `threshold = (guardians / 2) + 1`

## Affected Operations

When paused, these operations are blocked:
- ❌ `claim_payroll()`
- ❌ `claim_milestone()`
- ❌ `claim_time_based()`
- ❌ `batch_claim_payroll()`
- ❌ `batch_claim_milestones()`

These operations continue to work:
- ✅ `get_agreement()`
- ✅ `get_milestone()`
- ✅ `is_emergency_paused()`
- ✅ All read-only queries

## Testing

Run emergency pause tests:
```bash
cd onchain/contracts/stello_pay_contract
cargo test --test test_emergency_pause
```

Expected output:
```
running 12 tests
............
test result: ok. 12 passed; 0 failed
```

## Monitoring

Key metrics to monitor:
- Pause state changes
- Guardian proposal/approval events
- Failed operations due to pause
- Pause duration
- Unpause events

## Best Practices

1. **Guardian Selection**
   - Choose 3-5 trusted entities
   - Ensure geographic diversity
   - Use multi-sig wallets as guardians
   - Verify guardian availability regularly

2. **Timelock Usage**
   - 0 seconds: Critical emergencies only
   - 1 hour: Minor issues, quick response needed
   - 6-24 hours: Non-critical, planned maintenance

3. **Communication**
   - Announce pause immediately
   - Explain reason clearly
   - Provide estimated resolution time
   - Update community regularly

4. **Recovery**
   - Document incident thoroughly
   - Verify fix before unpausing
   - Monitor closely after unpause
   - Conduct post-mortem

## Emergency Contacts

Maintain a list of:
- Contract owner contact
- Guardian contacts
- Escalation procedures
- Communication channels

## Documentation

- Full documentation: `docs/emergency-pause.md`
- Implementation summary: `EMERGENCY_PAUSE_IMPLEMENTATION.md`
- Test file: `tests/test_emergency_pause.rs`
- Contract code: `src/payroll.rs` (emergency pause section)
