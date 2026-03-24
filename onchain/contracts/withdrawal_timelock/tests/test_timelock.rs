#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

use withdrawal_timelock::{
    OperationKind, OperationStatus, TimelockError, TimelockedOperation, WithdrawalTimelock,
    WithdrawalTimelockClient, MAX_DELAY_SECONDS,
};

// ─── Shared Test Helpers ──────────────────────────────────────────────────────

/// Creates a clean environment with all auth mocked out.
fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Registers the contract and calls `initialize` with a 60-second delay.
/// Returns `(client, admin_address)`.
fn setup(env: &Env) -> (WithdrawalTimelockClient<'static>, Address) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, WithdrawalTimelock);
    let client = WithdrawalTimelockClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin, &60u64);
    (client, admin)
}

/// Advances the ledger timestamp by `seconds`.
fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

/// Builds a `Withdrawal` kind using freshly generated addresses.
fn withdrawal_kind(env: &Env) -> OperationKind {
    let token = Address::generate(env);
    let to = Address::generate(env);
    OperationKind::Withdrawal(token, to, 1_000i128)
}

/// Builds an `AdminChange` kind using freshly generated addresses.
#[allow(dead_code)]
fn admin_change_kind(env: &Env) -> OperationKind {
    let target = Address::generate(env);
    let payload: BytesN<32> = BytesN::from_array(env, &[7u8; 32]);
    OperationKind::AdminChange(target, payload)
}

/// Queues `kind`, advances time past its `eta` by 1 second, and returns op_id.
/// Convenience for execute/cancel tests that do not care about the NotReady path.
fn queue_and_advance(
    client: &WithdrawalTimelockClient<'static>,
    admin: &Address,
    kind: OperationKind,
    env: &Env,
) -> u128 {
    let op_id = client.queue(admin, &kind);
    let op: TimelockedOperation = client.get_operation(&op_id).unwrap();
    let now = env.ledger().timestamp();
    let delta = op.eta.saturating_sub(now) + 1;
    advance_time(env, delta);
    op_id
}

// ─── Group A: Initialization (5 tests) ───────────────────────────────────────

#[test]
fn initialize_sets_config() {
    let env = create_env();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, WithdrawalTimelock);
    let client = WithdrawalTimelockClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin, &3600u64);

    let (cfg_admin, delay) = client.get_config();
    assert_eq!(cfg_admin, admin);
    assert_eq!(delay, 3600u64);
}

#[test]
fn initialize_zero_delay_fails() {
    let env = create_env();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, WithdrawalTimelock);
    let client = WithdrawalTimelockClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let res = client.try_initialize(&admin, &0u64);
    assert_eq!(res, Err(Ok(TimelockError::InvalidDelay)));
}

#[test]
fn initialize_delay_exceeds_max_fails() {
    let env = create_env();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, WithdrawalTimelock);
    let client = WithdrawalTimelockClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let res = client.try_initialize(&admin, &(MAX_DELAY_SECONDS + 1));
    assert_eq!(res, Err(Ok(TimelockError::DelayTooLarge)));
}

#[test]
fn initialize_exactly_max_delay_succeeds() {
    // Boundary: MAX_DELAY_SECONDS itself must be accepted.
    let env = create_env();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, WithdrawalTimelock);
    let client = WithdrawalTimelockClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin, &MAX_DELAY_SECONDS);
    let (_, delay) = client.get_config();
    assert_eq!(delay, MAX_DELAY_SECONDS);
}

#[test]
fn initialize_twice_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let res = client.try_initialize(&admin, &60u64);
    assert_eq!(res, Err(Ok(TimelockError::AlreadyInitialized)));
}

// ─── Group B: Queue (7 tests) ────────────────────────────────────────────────

#[test]
fn queue_withdrawal_returns_op_id_one() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = client.queue(&admin, &withdrawal_kind(&env));
    assert_eq!(op_id, 1u128);

    let op: TimelockedOperation = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Queued);
    assert_eq!(op.creator, admin);
    assert!(op.eta > op.created_at);
    assert!(op.executed_at.is_none());
    assert!(op.cancelled_at.is_none());
}

#[test]
fn queue_admin_change_records_kind() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let target = Address::generate(&env);
    let payload: BytesN<32> = BytesN::from_array(&env, &[3u8; 32]);
    let kind = OperationKind::AdminChange(target.clone(), payload.clone());

    let op_id = client.queue(&admin, &kind);
    let op: TimelockedOperation = client.get_operation(&op_id).unwrap();

    match op.kind {
        OperationKind::AdminChange(t, p) => {
            assert_eq!(t, target);
            assert_eq!(p, payload);
        }
        _ => panic!("expected AdminChange kind"),
    }
}

#[test]
fn queue_non_admin_fails() {
    let env = create_env();
    let (client, _admin) = setup(&env);

    let other = Address::generate(&env);
    let res = client.try_queue(&other, &withdrawal_kind(&env));
    assert_eq!(res, Err(Ok(TimelockError::NotAdmin)));
}

#[test]
fn queue_before_initialize_fails() {
    let env = create_env();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, WithdrawalTimelock);
    let client = WithdrawalTimelockClient::new(&env, &contract_id);

    let caller = Address::generate(&env);
    let res = client.try_queue(&caller, &withdrawal_kind(&env));
    assert_eq!(res, Err(Ok(TimelockError::NotInitialized)));
}

#[test]
fn queue_increments_queued_count() {
    let env = create_env();
    let (client, admin) = setup(&env);

    assert_eq!(client.get_queued_count(), 0u32);
    client.queue(&admin, &withdrawal_kind(&env));
    assert_eq!(client.get_queued_count(), 1u32);
    client.queue(&admin, &withdrawal_kind(&env));
    assert_eq!(client.get_queued_count(), 2u32);
}

#[test]
fn queue_multiple_ids_are_sequential() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let id1 = client.queue(&admin, &withdrawal_kind(&env));
    let id2 = client.queue(&admin, &withdrawal_kind(&env));
    let id3 = client.queue(&admin, &withdrawal_kind(&env));

    assert_eq!(id2, id1 + 1);
    assert_eq!(id3, id2 + 1);
}

#[test]
fn queue_appends_to_operations_for() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let id1 = client.queue(&admin, &withdrawal_kind(&env));
    let id2 = client.queue(&admin, &withdrawal_kind(&env));

    let ids = client.get_operations_for(&admin);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get(0).unwrap(), id1);
    assert_eq!(ids.get(1).unwrap(), id2);
}

// ─── Group C: Execute (8 tests) ──────────────────────────────────────────────

#[test]
fn execute_before_eta_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = client.queue(&admin, &withdrawal_kind(&env));
    // No time advance — should fail immediately
    let res = client.try_execute(&admin, &op_id);
    assert_eq!(res, Err(Ok(TimelockError::NotReady)));
}

#[test]
fn execute_exactly_at_eta_succeeds() {
    // Boundary: timestamp == eta must be accepted (>= semantics).
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = client.queue(&admin, &withdrawal_kind(&env));
    let op: TimelockedOperation = client.get_operation(&op_id).unwrap();

    let now = env.ledger().timestamp();
    let delta = op.eta.saturating_sub(now); // advance to exactly eta
    advance_time(&env, delta);

    client.execute(&admin, &op_id); // must not panic

    let executed = client.get_operation(&op_id).unwrap();
    assert_eq!(executed.status, OperationStatus::Executed);
    assert!(executed.executed_at.is_some());
}

#[test]
fn execute_after_eta_succeeds() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = queue_and_advance(&client, &admin, withdrawal_kind(&env), &env);
    client.execute(&admin, &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Executed);
    assert!(op.executed_at.is_some());
    assert!(op.cancelled_at.is_none());
}

#[test]
fn execute_already_executed_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = queue_and_advance(&client, &admin, withdrawal_kind(&env), &env);
    client.execute(&admin, &op_id);

    let res = client.try_execute(&admin, &op_id);
    assert_eq!(res, Err(Ok(TimelockError::AlreadyExecutedOrCancelled)));
}

#[test]
fn execute_cancelled_op_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = client.queue(&admin, &withdrawal_kind(&env));
    client.cancel(&admin, &op_id);

    // Advance past eta — status is Cancelled, must still fail
    advance_time(&env, 120);
    let res = client.try_execute(&admin, &op_id);
    assert_eq!(res, Err(Ok(TimelockError::AlreadyExecutedOrCancelled)));
}

#[test]
fn execute_non_existent_op_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let res = client.try_execute(&admin, &999u128);
    assert_eq!(res, Err(Ok(TimelockError::OperationNotFound)));
}

#[test]
fn execute_non_admin_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = queue_and_advance(&client, &admin, withdrawal_kind(&env), &env);
    let other = Address::generate(&env);

    let res = client.try_execute(&other, &op_id);
    assert_eq!(res, Err(Ok(TimelockError::NotAdmin)));
}

#[test]
fn execute_decrements_queued_count() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = queue_and_advance(&client, &admin, withdrawal_kind(&env), &env);
    assert_eq!(client.get_queued_count(), 1u32);

    client.execute(&admin, &op_id);
    assert_eq!(client.get_queued_count(), 0u32);
}

// ─── Group D: Cancel (6 tests) ───────────────────────────────────────────────

#[test]
fn cancel_queued_op_succeeds() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = client.queue(&admin, &withdrawal_kind(&env));
    client.cancel(&admin, &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);
    assert!(op.cancelled_at.is_some());
    assert!(op.executed_at.is_none());
}

#[test]
fn cancel_executed_op_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = queue_and_advance(&client, &admin, withdrawal_kind(&env), &env);
    client.execute(&admin, &op_id);

    let res = client.try_cancel(&admin, &op_id);
    assert_eq!(res, Err(Ok(TimelockError::AlreadyExecutedOrCancelled)));
}

#[test]
fn cancel_non_existent_op_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let res = client.try_cancel(&admin, &999u128);
    assert_eq!(res, Err(Ok(TimelockError::OperationNotFound)));
}

#[test]
fn cancel_non_admin_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let op_id = client.queue(&admin, &withdrawal_kind(&env));
    let other = Address::generate(&env);

    let res = client.try_cancel(&other, &op_id);
    assert_eq!(res, Err(Ok(TimelockError::NotAdmin)));
}

#[test]
fn cancel_decrements_queued_count() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let _id1 = client.queue(&admin, &withdrawal_kind(&env));
    let id2 = client.queue(&admin, &withdrawal_kind(&env));
    assert_eq!(client.get_queued_count(), 2u32);

    client.cancel(&admin, &id2);
    assert_eq!(client.get_queued_count(), 1u32);
}

#[test]
fn cancel_then_requeue_allowed() {
    // After a cancel there must be no stale state that prevents the admin
    // from queueing a new operation of the same kind.
    let env = create_env();
    let (client, admin) = setup(&env);

    let kind = withdrawal_kind(&env);
    let op_id1 = client.queue(&admin, &kind.clone());
    client.cancel(&admin, &op_id1);
    assert_eq!(client.get_queued_count(), 0u32);

    // Re-queue: must succeed with a new monotone id
    let op_id2 = client.queue(&admin, &kind);
    assert!(op_id2 > op_id1);

    let op = client.get_operation(&op_id2).unwrap();
    assert_eq!(op.status, OperationStatus::Queued);
    assert_eq!(client.get_queued_count(), 1u32);
}

// ─── Group E: Update Delay (5 tests) ─────────────────────────────────────────

#[test]
fn update_delay_succeeds() {
    let env = create_env();
    let (client, admin) = setup(&env);

    client.update_delay(&admin, &7200u64);
    let (_, delay) = client.get_config();
    assert_eq!(delay, 7200u64);
}

#[test]
fn update_delay_zero_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let res = client.try_update_delay(&admin, &0u64);
    assert_eq!(res, Err(Ok(TimelockError::InvalidDelay)));
}

#[test]
fn update_delay_exceeds_max_fails() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let res = client.try_update_delay(&admin, &(MAX_DELAY_SECONDS + 1));
    assert_eq!(res, Err(Ok(TimelockError::DelayTooLarge)));
}

#[test]
fn update_delay_non_admin_fails() {
    let env = create_env();
    let (client, _admin) = setup(&env);

    let other = Address::generate(&env);
    let res = client.try_update_delay(&other, &120u64);
    assert_eq!(res, Err(Ok(TimelockError::NotAdmin)));
}

#[test]
fn update_delay_does_not_alter_queued_eta() {
    // Security invariant: update_delay must NOT retroactively change eta.
    let env = create_env();
    let (client, admin) = setup(&env); // delay = 60s

    let op_id = client.queue(&admin, &withdrawal_kind(&env));
    let op_before: TimelockedOperation = client.get_operation(&op_id).unwrap();
    let eta_before = op_before.eta;

    // Increase delay to 1 hour
    client.update_delay(&admin, &3600u64);

    // The already-queued op's eta must be unchanged
    let op_after: TimelockedOperation = client.get_operation(&op_id).unwrap();
    assert_eq!(op_after.eta, eta_before);

    // New ops queued after the update must use the new delay
    let op_id2 = client.queue(&admin, &withdrawal_kind(&env));
    let op2: TimelockedOperation = client.get_operation(&op_id2).unwrap();
    // op2.eta = op2.created_at + 3600 >= op_before.created_at + 3600 > eta_before
    assert!(op2.eta > eta_before);
}

// ─── Group F: Read Helpers (4 tests) ─────────────────────────────────────────

#[test]
fn get_operations_for_returns_empty_before_queue() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let ids = client.get_operations_for(&admin);
    assert_eq!(ids.len(), 0);
}

#[test]
fn get_operation_returns_none_for_missing_id() {
    let env = create_env();
    let (client, _admin) = setup(&env);

    let result = client.get_operation(&42u128);
    assert!(result.is_none());
}

#[test]
fn get_queued_count_zero_when_empty() {
    let env = create_env();
    let (client, _admin) = setup(&env);

    assert_eq!(client.get_queued_count(), 0u32);
}

#[test]
fn get_config_before_initialize_fails() {
    let env = create_env();
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, WithdrawalTimelock);
    let client = WithdrawalTimelockClient::new(&env, &contract_id);

    let res = client.try_get_config();
    assert_eq!(res, Err(Ok(TimelockError::NotInitialized)));
}

// ─── Regression: Existing Tests (preserved) ──────────────────────────────────

#[test]
fn queue_and_execute_withdrawal_after_delay() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let token = Address::generate(&env);
    let to = Address::generate(&env);

    let op_id = client.queue(
        &admin,
        &OperationKind::Withdrawal(token.clone(), to.clone(), 1_000i128),
    );

    let op: TimelockedOperation = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Queued);
    assert_eq!(op.creator, admin);
    assert!(op.eta > op.created_at);

    // Cannot execute before eta
    let early = client.try_execute(&admin, &op_id);
    assert_eq!(early, Err(Ok(TimelockError::NotReady)));

    // Advance beyond eta and execute
    let now = env.ledger().timestamp();
    let delta = op.eta.saturating_sub(now) + 1;
    advance_time(&env, delta);

    client.execute(&admin, &op_id);

    let executed = client.get_operation(&op_id).unwrap();
    assert_eq!(executed.status, OperationStatus::Executed);
    assert!(executed.executed_at.is_some());
}

#[test]
fn cancel_prevents_later_execution() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let token = Address::generate(&env);
    let to = Address::generate(&env);

    let op_id = client.queue(
        &admin,
        &OperationKind::Withdrawal(token.clone(), to.clone(), 500i128),
    );

    client.cancel(&admin, &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);
    // cancelled_at is now set — verify the audit field
    assert!(op.cancelled_at.is_some());

    let res = client.try_execute(&admin, &op_id);
    assert_eq!(res, Err(Ok(TimelockError::AlreadyExecutedOrCancelled)));
}

#[test]
fn admin_change_operation_records_intent() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let target = Address::generate(&env);
    let payload: BytesN<32> = BytesN::from_array(&env, &[3u8; 32]);

    let op_id = client.queue(
        &admin,
        &OperationKind::AdminChange(target.clone(), payload.clone()),
    );

    let op = client.get_operation(&op_id).unwrap();
    let now = env.ledger().timestamp();
    let delta = op.eta.saturating_sub(now) + 1;
    advance_time(&env, delta);

    client.execute(&admin, &op_id);

    let executed = client.get_operation(&op_id).unwrap();
    match executed.kind {
        OperationKind::AdminChange(t, p) => {
            assert_eq!(t, target);
            assert_eq!(p, payload);
        }
        _ => panic!("expected admin change kind"),
    }
    assert_eq!(executed.status, OperationStatus::Executed);
}

#[test]
fn non_admin_cannot_queue_or_execute() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let other = Address::generate(&env);
    let token = Address::generate(&env);
    let to = Address::generate(&env);

    let res = client.try_queue(
        &other,
        &OperationKind::Withdrawal(token.clone(), to.clone(), 1_000i128),
    );
    assert_eq!(res, Err(Ok(TimelockError::NotAdmin)));

    let op_id = client.queue(
        &admin,
        &OperationKind::Withdrawal(token.clone(), to.clone(), 1_000i128),
    );

    let res = client.try_execute(&other, &op_id);
    assert_eq!(res, Err(Ok(TimelockError::NotAdmin)));
}

#[test]
fn operations_for_admin_lists_ids() {
    let env = create_env();
    let (client, admin) = setup(&env);

    let token = Address::generate(&env);
    let to = Address::generate(&env);

    let id1 = client.queue(
        &admin,
        &OperationKind::Withdrawal(token.clone(), to.clone(), 1_000i128),
    );
    let id2 = client.queue(
        &admin,
        &OperationKind::Withdrawal(token.clone(), to.clone(), 2_000i128),
    );

    let ids = client.get_operations_for(&admin);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get(0).unwrap(), id1);
    assert_eq!(ids.get(1).unwrap(), id2);
}
