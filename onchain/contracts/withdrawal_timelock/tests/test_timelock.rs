#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

use withdrawal_timelock::{
    OperationKind, OperationStatus, TimelockError, TimelockedOperation, WithdrawalTimelock,
    WithdrawalTimelockClient,
};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_initialized(env: &Env) -> (WithdrawalTimelockClient<'static>, Address) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, WithdrawalTimelock);
    let client = WithdrawalTimelockClient::new(env, &contract_id);

    let admin = Address::generate(env);
    client.initialize(&admin, &60u64);

    (client, admin)
}

fn advance_time(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
    });
}

#[test]
fn initialize_and_get_config() {
    let env = create_env();
    let (client, admin) = setup_initialized(&env);

    let (cfg_admin, delay) = client.get_config();
    assert_eq!(cfg_admin, admin);
    assert_eq!(delay, 60u64);
}

#[test]
fn queue_and_execute_withdrawal_after_delay() {
    let env = create_env();
    let (client, admin) = setup_initialized(&env);

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

    // Cannot execute before eta.
    let early = client.try_execute(&admin, &op_id);
    assert_eq!(early, Err(Ok(TimelockError::NotReady)));

    // Advance beyond eta and execute.
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
    let (client, admin) = setup_initialized(&env);

    let token = Address::generate(&env);
    let to = Address::generate(&env);

    let op_id = client.queue(
        &admin,
        &OperationKind::Withdrawal(token.clone(), to.clone(), 500i128),
    );

    client.cancel(&admin, &op_id);

    let op = client.get_operation(&op_id).unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);

    let res = client.try_execute(&admin, &op_id);
    assert_eq!(res, Err(Ok(TimelockError::AlreadyExecutedOrCancelled)));
}

#[test]
fn admin_change_operation_records_intent() {
    let env = create_env();
    let (client, admin) = setup_initialized(&env);

    let target = Address::generate(&env);
    let payload: BytesN<32> = BytesN::from_array(&env, &[3u8; 32]);

    let op_id = client.queue(
        &admin,
        &OperationKind::AdminChange(target.clone(), payload.clone()),
    );

    // Fast-forward to execute.
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
    let (client, admin) = setup_initialized(&env);

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
    let (client, admin) = setup_initialized(&env);

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
