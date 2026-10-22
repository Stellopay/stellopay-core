#![cfg(test)]

//! Lifecycle-state coverage for `propose_signer_update` (issue #1353).
//!
//! Pinning behaviour of `propose_signer_update` across every lifecycle state
//! of the records it touches and interacts with:
//!
//! 1. `SignerChangeProposal` lifecycle:
//!    - `Absent`: No proposal exists in storage (`get_signer_change_proposal() == None`).
//!    - `Pending`: A proposal is in flight and within its timelock delay (`now < activation_timestamp`).
//!    - `Ready`: The timelock delay has elapsed (`now >= activation_timestamp`), ready for execution.
//!    - `Executed`: Terminal state — proposal has been executed, signers/threshold updated, proposal cleared.
//!    - `Cancelled`: Terminal state — proposal has been cancelled during delay, proposal cleared.
//!
//! 2. `OperationStatus` lifecycle:
//!    - `Pending`: Operation is awaiting required signer approvals.
//!    - `Executed`: Terminal state — operation met threshold and executed.
//!    - `Cancelled`: Terminal state — operation was cancelled by owner/creator.
//!
//! Acceptance criteria:
//! - The call is covered from each state the type defines, including its terminal states.
//! - Each outcome is asserted as a value (`Ok(Ok(()))`) or a typed error (`Err(Ok(Error::from(MultisigError::InvalidThreshold)))`),
//!   not a panic.
//! - A record in a terminal state is not mutated by a refused call.
//! - The test names which state it is driving.

use multisig::{
    MultisigContract, MultisigContractClient, MultisigError, OperationKind, OperationStatus,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Error, Vec,
};

/// High-level enum capturing the lifecycle states of a signer change proposal record.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProposalLifecycleState {
    /// No proposal exists in contract storage.
    Absent,
    /// A proposal has been proposed and is within its timelock delay.
    Pending,
    /// The timelock delay has elapsed, so the proposal is ready to execute.
    Ready,
    /// The proposal was executed (terminal state).
    Executed,
    /// The proposal was cancelled (terminal state).
    Cancelled,
}

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_multisig(
    env: &Env,
) -> (
    MultisigContractClient<'static>,
    Address,
    Vec<Address>,
    Address,
    Address,
) {
    #[allow(deprecated)]
    let id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &id);

    let owner = Address::generate(env);
    let s1 = Address::generate(env);
    let s2 = Address::generate(env);
    let mut signers = Vec::new(env);
    signers.push_back(s1.clone());
    signers.push_back(s2.clone());

    client.initialize(&owner, &signers, &2u32, &None);
    (client, owner, signers, s1, s2)
}

fn generate_signer_set(env: &Env, count: u32) -> Vec<Address> {
    let mut signers = Vec::new(env);
    for _ in 0..count {
        signers.push_back(Address::generate(env));
    }
    signers
}

fn invalid_threshold_error(
) -> Result<Result<(), soroban_sdk::ConversionError>, Result<Error, soroban_sdk::InvokeError>> {
    Err(Ok(Error::from(MultisigError::InvalidThreshold)))
}

// ============================================================================
// 1. SignerChangeProposal Lifecycle State Coverage
// ============================================================================

/// Drives `propose_signer_update` from the `Absent` proposal state:
/// when no proposal exists, an authorized valid call succeeds with a value
/// outcome (`Ok(Ok(()))`) and writes the proposal record.
#[test]
fn test_propose_signer_update_from_absent_proposal_state_succeeds() {
    let env = create_env();
    let (client, _owner, signers, _s1, _s2) = setup_multisig(&env);

    assert_eq!(client.get_signer_change_proposal(), None);

    let now = env.ledger().timestamp();
    let delay = client.get_signer_change_delay();
    let new_signers = generate_signer_set(&env, 3);

    // Call returns value Ok(Ok(())), asserted as value, not a panic.
    let result = client.try_propose_signer_update(&new_signers, &2u32);
    assert_eq!(result, Ok(Ok(())));

    let proposal = client
        .get_signer_change_proposal()
        .expect("proposal should be recorded");
    assert_eq!(proposal.new_signers, new_signers);
    assert_eq!(proposal.new_threshold, 2u32);
    assert_eq!(proposal.proposed_at, now);
    assert_eq!(proposal.activation_timestamp, now + delay);

    // Current signer set and threshold on contract remain unchanged.
    assert_eq!(client.get_signers(), signers);
    assert_eq!(client.get_threshold(), 2u32);
}

/// Drives `propose_signer_update` from the `Absent` proposal state with invalid arguments:
/// the call is refused with a typed error (`MultisigError::InvalidThreshold`), not a panic,
/// and storage remains completely empty (`None`).
#[test]
fn test_propose_signer_update_from_absent_proposal_state_refused_call_yields_typed_error() {
    let env = create_env();
    let (client, _owner, signers, _s1, _s2) = setup_multisig(&env);

    assert_eq!(client.get_signer_change_proposal(), None);

    let new_signers = generate_signer_set(&env, 2);

    // Refused: threshold 0 is invalid.
    let zero_res = client.try_propose_signer_update(&new_signers, &0u32);
    assert_eq!(zero_res, invalid_threshold_error());

    // Refused: threshold exceeding signer count.
    let excess_res = client.try_propose_signer_update(&new_signers, &3u32);
    assert_eq!(excess_res, invalid_threshold_error());

    // No proposal record written.
    assert_eq!(client.get_signer_change_proposal(), None);
    assert_eq!(client.get_signers(), signers);
    assert_eq!(client.get_threshold(), 2u32);
}

/// Drives `propose_signer_update` from the `Pending` proposal state:
/// an existing proposal is in its timelock delay; a valid call succeeds with a
/// value outcome (`Ok(Ok(()))`) and overwrites the active proposal.
#[test]
fn test_propose_signer_update_from_pending_proposal_state_succeeds_and_overwrites() {
    let env = create_env();
    let (client, _owner, signers, _s1, _s2) = setup_multisig(&env);

    let delay = client.get_signer_change_delay();
    let first_signers = generate_signer_set(&env, 2);
    client.propose_signer_update(&first_signers, &1u32);

    let first_proposal = client.get_signer_change_proposal().unwrap();
    assert_eq!(first_proposal.new_signers, first_signers);

    // Advance halfway through delay: proposal is still strictly Pending.
    env.ledger().with_mut(|li| {
        li.timestamp += delay / 2;
    });
    let now = env.ledger().timestamp();
    assert!(now < first_proposal.activation_timestamp);

    // Overwrite with a new proposal from the Pending state.
    let second_signers = generate_signer_set(&env, 3);
    let result = client.try_propose_signer_update(&second_signers, &2u32);
    assert_eq!(result, Ok(Ok(())));

    let second_proposal = client.get_signer_change_proposal().unwrap();
    assert_eq!(second_proposal.new_signers, second_signers);
    assert_eq!(second_proposal.new_threshold, 2u32);
    assert_eq!(second_proposal.proposed_at, now);
    assert_eq!(second_proposal.activation_timestamp, now + delay);

    // Active contract signers remain unchanged.
    assert_eq!(client.get_signers(), signers);
}

/// Drives `propose_signer_update` from the `Pending` proposal state with invalid arguments:
/// the call is refused with a typed error (`MultisigError::InvalidThreshold`), not a panic,
/// and the pending proposal record is left completely unmutated.
#[test]
fn test_propose_signer_update_from_pending_proposal_state_refused_call_leaves_proposal_unmutated() {
    let env = create_env();
    let (client, _owner, _signers, _s1, _s2) = setup_multisig(&env);

    let initial_signers = generate_signer_set(&env, 2);
    client.propose_signer_update(&initial_signers, &1u32);

    let proposal_before = client.get_signer_change_proposal().unwrap();

    // Refused call: threshold exceeds length.
    let bad_signers = generate_signer_set(&env, 2);
    let res = client.try_propose_signer_update(&bad_signers, &5u32);
    assert_eq!(res, invalid_threshold_error());

    // Pending proposal was not mutated.
    let proposal_after = client.get_signer_change_proposal().unwrap();
    assert_eq!(proposal_after, proposal_before);
}

/// Drives `propose_signer_update` from the `Ready` proposal state:
/// the proposal's timelock delay has elapsed, but it has not been executed yet.
/// A valid call succeeds with a value outcome (`Ok(Ok(()))`) and replaces the ready
/// proposal, establishing a fresh timelock window.
#[test]
fn test_propose_signer_update_from_ready_proposal_state_succeeds_and_resets_timelock() {
    let env = create_env();
    let (client, _owner, _signers, _s1, _s2) = setup_multisig(&env);

    let delay = client.get_signer_change_delay();
    let ready_signers = generate_signer_set(&env, 2);
    client.propose_signer_update(&ready_signers, &2u32);

    // Advance past timelock: proposal is now Ready.
    env.ledger().with_mut(|li| {
        li.timestamp += delay + 10;
    });
    let now = env.ledger().timestamp();
    let ready_proposal = client.get_signer_change_proposal().unwrap();
    assert!(now >= ready_proposal.activation_timestamp);

    // Drive a new proposal from the Ready state.
    let replacement_signers = generate_signer_set(&env, 4);
    let result = client.try_propose_signer_update(&replacement_signers, &3u32);
    assert_eq!(result, Ok(Ok(())));

    let new_proposal = client.get_signer_change_proposal().unwrap();
    assert_eq!(new_proposal.new_signers, replacement_signers);
    assert_eq!(new_proposal.new_threshold, 3u32);
    assert_eq!(new_proposal.proposed_at, now);
    assert_eq!(new_proposal.activation_timestamp, now + delay);
}

/// Drives `propose_signer_update` from the `Ready` proposal state with invalid arguments:
/// the call is refused with a typed error (`MultisigError::InvalidThreshold`), not a panic,
/// and the ready proposal record is left completely unmutated.
#[test]
fn test_propose_signer_update_from_ready_proposal_state_refused_call_leaves_proposal_unmutated() {
    let env = create_env();
    let (client, _owner, _signers, _s1, _s2) = setup_multisig(&env);

    let delay = client.get_signer_change_delay();
    let ready_signers = generate_signer_set(&env, 2);
    client.propose_signer_update(&ready_signers, &2u32);

    env.ledger().with_mut(|li| {
        li.timestamp += delay + 5;
    });
    let proposal_before = client.get_signer_change_proposal().unwrap();

    // Refused call: threshold 0.
    let bad_signers = generate_signer_set(&env, 2);
    let res = client.try_propose_signer_update(&bad_signers, &0u32);
    assert_eq!(res, invalid_threshold_error());

    // Ready proposal remains completely unmutated.
    let proposal_after = client.get_signer_change_proposal().unwrap();
    assert_eq!(proposal_after, proposal_before);
}

/// Drives `propose_signer_update` from the `Executed` terminal proposal state:
/// after execution, the previous proposal has reached its terminal state.
/// A refused call with invalid threshold must return a typed error, not panic,
/// and must not mutate the executed contract state (signers, threshold, proposal slot).
#[test]
fn test_propose_signer_update_from_executed_terminal_proposal_state_refused_call_does_not_mutate_state(
) {
    let env = create_env();
    let (client, _owner, _signers, _s1, _s2) = setup_multisig(&env);

    let delay = client.get_signer_change_delay();
    let executed_signers = generate_signer_set(&env, 3);
    client.propose_signer_update(&executed_signers, &2u32);

    // Advance past timelock and execute.
    env.ledger().with_mut(|li| {
        li.timestamp += delay;
    });
    client.execute_signer_update();

    // Verify terminal state reached:
    assert_eq!(client.get_signers(), executed_signers);
    assert_eq!(client.get_threshold(), 2u32);
    assert_eq!(client.get_signer_change_proposal(), None);

    // Refused call from Executed terminal state.
    let bad_signers = generate_signer_set(&env, 2);
    let res = client.try_propose_signer_update(&bad_signers, &0u32);
    assert_eq!(res, invalid_threshold_error());

    // State in Executed terminal condition is NOT mutated by the refused call:
    assert_eq!(client.get_signers(), executed_signers);
    assert_eq!(client.get_threshold(), 2u32);
    assert_eq!(client.get_signer_change_proposal(), None);
}

/// Drives `propose_signer_update` from the `Executed` terminal proposal state:
/// after execution has finalized the previous proposal, a new valid proposal
/// can be initiated and returns `Ok(Ok(()))`.
#[test]
fn test_propose_signer_update_from_executed_terminal_proposal_state_succeeds() {
    let env = create_env();
    let (client, _owner, _signers, _s1, _s2) = setup_multisig(&env);

    let delay = client.get_signer_change_delay();
    let executed_signers = generate_signer_set(&env, 3);
    client.propose_signer_update(&executed_signers, &2u32);

    env.ledger().with_mut(|li| {
        li.timestamp += delay;
    });
    client.execute_signer_update();

    // From Executed terminal state, propose a new update.
    let next_signers = generate_signer_set(&env, 2);
    let result = client.try_propose_signer_update(&next_signers, &1u32);
    assert_eq!(result, Ok(Ok(())));

    let proposal = client.get_signer_change_proposal().unwrap();
    assert_eq!(proposal.new_signers, next_signers);
    assert_eq!(proposal.new_threshold, 1u32);

    // Active signers still reflect the finalized execution.
    assert_eq!(client.get_signers(), executed_signers);
}

/// Drives `propose_signer_update` from the `Cancelled` terminal proposal state:
/// after cancellation has finalized the proposal, a refused call with invalid threshold
/// must return a typed error, not panic, and must not mutate contract state.
#[test]
fn test_propose_signer_update_from_cancelled_terminal_proposal_state_refused_call_does_not_mutate_state(
) {
    let env = create_env();
    let (client, owner, original_signers, _s1, _s2) = setup_multisig(&env);

    let cancelled_signers = generate_signer_set(&env, 3);
    client.propose_signer_update(&cancelled_signers, &2u32);

    // Cancel proposal during delay.
    client.cancel_signer_update(&owner);

    // Verify terminal state reached:
    assert_eq!(client.get_signer_change_proposal(), None);
    assert_eq!(client.get_signers(), original_signers);
    assert_eq!(client.get_threshold(), 2u32);

    // Refused call from Cancelled terminal state.
    let bad_signers = generate_signer_set(&env, 2);
    let res = client.try_propose_signer_update(&bad_signers, &0u32);
    assert_eq!(res, invalid_threshold_error());

    // State in Cancelled terminal condition is NOT mutated by the refused call:
    assert_eq!(client.get_signer_change_proposal(), None);
    assert_eq!(client.get_signers(), original_signers);
    assert_eq!(client.get_threshold(), 2u32);
}

/// Drives `propose_signer_update` from the `Cancelled` terminal proposal state:
/// after cancellation, an authorized valid proposal succeeds with value outcome `Ok(Ok(()))`.
#[test]
fn test_propose_signer_update_from_cancelled_terminal_proposal_state_succeeds() {
    let env = create_env();
    let (client, owner, original_signers, _s1, _s2) = setup_multisig(&env);

    let cancelled_signers = generate_signer_set(&env, 3);
    client.propose_signer_update(&cancelled_signers, &2u32);
    client.cancel_signer_update(&owner);

    let next_signers = generate_signer_set(&env, 2);
    let result = client.try_propose_signer_update(&next_signers, &1u32);
    assert_eq!(result, Ok(Ok(())));

    let proposal = client.get_signer_change_proposal().unwrap();
    assert_eq!(proposal.new_signers, next_signers);
    assert_eq!(proposal.new_threshold, 1u32);
    assert_eq!(client.get_signers(), original_signers);
}

// ============================================================================
// 2. OperationStatus Lifecycle State Coverage
// ============================================================================

/// Helper to create an operation payload that executes purely internally
/// without requiring external token contract dependencies.
fn sample_operation_kind() -> OperationKind {
    OperationKind::SetThresholdOverride(multisig::OperationType::LargePayment, Some(2))
}

/// Drives `propose_signer_update` when an operation record is in `OperationStatus::Pending`:
/// the proposal is created with value outcome `Ok(Ok(()))` and the pending operation
/// record is not mutated.
#[test]
fn test_propose_signer_update_from_pending_operation_state_succeeds() {
    let env = create_env();
    let (client, _owner, _signers, s1, _s2) = setup_multisig(&env);

    let kind = sample_operation_kind();
    let op_id = client.propose_operation(&s1, &kind);

    let op_before = client.get_operation(&op_id).unwrap();
    assert_eq!(op_before.status, OperationStatus::Pending);

    // Call propose_signer_update while operation is pending.
    let new_signers = generate_signer_set(&env, 3);
    let res = client.try_propose_signer_update(&new_signers, &2u32);
    assert_eq!(res, Ok(Ok(())));

    // Pending operation is untouched.
    let op_after = client.get_operation(&op_id).unwrap();
    assert_eq!(op_after, op_before);
    assert_eq!(op_after.status, OperationStatus::Pending);
}

/// Drives `propose_signer_update` when an operation record is in `OperationStatus::Executed`
/// (a terminal state of OperationStatus):
/// a valid call succeeds with value outcome `Ok(Ok(()))`, and the executed operation
/// record is untouched.
#[test]
fn test_propose_signer_update_from_executed_terminal_operation_state_succeeds() {
    let env = create_env();
    let (client, _owner, _signers, s1, s2) = setup_multisig(&env);

    let kind = sample_operation_kind();
    let op_id = client.propose_operation(&s1, &kind);
    // Threshold is 2: s1 proposed (auto-approved), s2 approves -> executes.
    client.approve_operation(&s2, &op_id);

    let op_before = client.get_operation(&op_id).unwrap();
    assert_eq!(op_before.status, OperationStatus::Executed);
    assert!(op_before.executed_at.is_some());

    let new_signers = generate_signer_set(&env, 3);
    let res = client.try_propose_signer_update(&new_signers, &2u32);
    assert_eq!(res, Ok(Ok(())));

    let op_after = client.get_operation(&op_id).unwrap();
    assert_eq!(op_after, op_before);
    assert_eq!(op_after.status, OperationStatus::Executed);
}

/// Drives `propose_signer_update` when an operation record is in `OperationStatus::Executed`
/// (terminal state) with invalid arguments:
/// the call is refused with a typed error (`MultisigError::InvalidThreshold`), not a panic,
/// and the record in the terminal state is proven NOT to be mutated.
#[test]
fn test_propose_signer_update_from_executed_terminal_operation_state_refused_call_leaves_record_unmutated(
) {
    let env = create_env();
    let (client, _owner, _signers, s1, s2) = setup_multisig(&env);

    let kind = sample_operation_kind();
    let op_id = client.propose_operation(&s1, &kind);
    client.approve_operation(&s2, &op_id);

    let op_before = client.get_operation(&op_id).unwrap();
    assert_eq!(op_before.status, OperationStatus::Executed);

    // Refused call: threshold 0.
    let bad_signers = generate_signer_set(&env, 2);
    let res = client.try_propose_signer_update(&bad_signers, &0u32);
    assert_eq!(res, invalid_threshold_error());

    // Terminal record must not be mutated by a refused call.
    let op_after = client.get_operation(&op_id).unwrap();
    assert_eq!(op_after.id, op_before.id);
    assert_eq!(op_after.kind, op_before.kind);
    assert_eq!(op_after.creator, op_before.creator);
    assert_eq!(op_after.status, OperationStatus::Executed);
    assert_eq!(op_after.created_at, op_before.created_at);
    assert_eq!(op_after.executed_at, op_before.executed_at);
    assert_eq!(op_after, op_before);
}

/// Drives `propose_signer_update` when an operation record is in `OperationStatus::Cancelled`
/// (a terminal state of OperationStatus):
/// a valid call succeeds with value outcome `Ok(Ok(()))`, and the cancelled operation
/// record is untouched.
#[test]
fn test_propose_signer_update_from_cancelled_terminal_operation_state_succeeds() {
    let env = create_env();
    let (client, owner, _signers, s1, _s2) = setup_multisig(&env);

    let kind = sample_operation_kind();
    let op_id = client.propose_operation(&s1, &kind);
    client.cancel_operation(&owner, &op_id);

    let op_before = client.get_operation(&op_id).unwrap();
    assert_eq!(op_before.status, OperationStatus::Cancelled);

    let new_signers = generate_signer_set(&env, 3);
    let res = client.try_propose_signer_update(&new_signers, &2u32);
    assert_eq!(res, Ok(Ok(())));

    let op_after = client.get_operation(&op_id).unwrap();
    assert_eq!(op_after, op_before);
    assert_eq!(op_after.status, OperationStatus::Cancelled);
}

/// Drives `propose_signer_update` when an operation record is in `OperationStatus::Cancelled`
/// (terminal state) with invalid arguments:
/// the call is refused with a typed error (`MultisigError::InvalidThreshold`), not a panic,
/// and the record in the terminal state is proven NOT to be mutated.
#[test]
fn test_propose_signer_update_from_cancelled_terminal_operation_state_refused_call_leaves_record_unmutated(
) {
    let env = create_env();
    let (client, owner, _signers, s1, _s2) = setup_multisig(&env);

    let kind = sample_operation_kind();
    let op_id = client.propose_operation(&s1, &kind);
    client.cancel_operation(&owner, &op_id);

    let op_before = client.get_operation(&op_id).unwrap();
    assert_eq!(op_before.status, OperationStatus::Cancelled);

    // Refused call: threshold exceeds length.
    let bad_signers = generate_signer_set(&env, 2);
    let res = client.try_propose_signer_update(&bad_signers, &4u32);
    assert_eq!(res, invalid_threshold_error());

    // Terminal record must not be mutated by a refused call.
    let op_after = client.get_operation(&op_id).unwrap();
    assert_eq!(op_after.id, op_before.id);
    assert_eq!(op_after.kind, op_before.kind);
    assert_eq!(op_after.creator, op_before.creator);
    assert_eq!(op_after.status, OperationStatus::Cancelled);
    assert_eq!(op_after.created_at, op_before.created_at);
    assert_eq!(op_after.executed_at, op_before.executed_at);
    assert_eq!(op_after, op_before);
}
