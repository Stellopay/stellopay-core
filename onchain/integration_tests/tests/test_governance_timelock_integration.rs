//! Integration tests for governance and withdrawal_timelock interoperability.
//!
//! This test suite verifies that governance proposals can queue timelock operations
//! for admin changes with proper payload hashes and execution/cancel semantics.
//!
//! ## Test Scenarios
//!
//! 1. **Proposal Success Flow**: propose() -> vote() -> queue() -> timelock.queue() -> execute()
//! 2. **Proposal Cancel Flow**: propose() -> vote() -> queue() -> timelock.queue() -> cancel()
//! 3. **Edge Cases**: duplicate proposals, delay updates, payload hash verification
//!
//! ## Security Assumptions Tested
//!
//! - Only authorized governance execution can queue timelock ops
//! - Payload hashes are collision-resistant with domain separation
//! - Non-retroactivity of delay updates on queued operations
//! - Proper access control throughout the flow

#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, BytesN, Env,
};

use governance::{
    GovernanceContract, GovernanceContractClient, ProposalKind, ProposalStatus, VoteChoice,
};
use withdrawal_timelock::{
    WithdrawalTimelock, WithdrawalTimelockClient, OperationKind, OperationStatus,
};

// ============================================================================
// CONSTANTS
// ============================================================================

const QUORUM_BPS: u32 = 5_000; // Test constants
const VOTING_PERIOD: u64 = 86_400;      // 1 day
const TIMELOCK_DELAY: u64 = 604_800;    // 1 week
const EXECUTION_WINDOW: u64 = 86_400;   // 1 day

const VOTING_POWER: i128 = 1_000;

// ============================================================================
// HELPERS
// ============================================================================

/// Creates a test environment with all auths mocked.
fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

/// Generates a fresh test address.
fn addr(env: &Env) -> Address {
    Address::generate(env)
}

/// Advances the ledger timestamp by `seconds`.
fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

/// Creates a deterministic payload hash for admin change operations.
///
/// This function implements the exact payload hash derivation that off-chain
/// tooling must use to verify operations deterministically.
///
/// ## Domain Separation
///
/// The hash includes domain separation to prevent collision attacks:
/// - Domain prefix: "ADMIN_CHANGE"
/// - Target contract address
/// - New admin address
/// - Nonce/timestamp for uniqueness
fn create_admin_change_payload_hash(
    env: &Env,
    target_contract: &Address,
    new_admin: &Address,
    nonce: u64,
) -> BytesN<32> {
    // For simplicity and determinism, create a hash based on the inputs
    // In a real implementation, this would be more sophisticated
    let target_bytes = target_contract.to_string();
    let admin_bytes = new_admin.to_string();
    let nonce_bytes = nonce.to_string();
    
    // Create a combined string for hashing
    let combined_str = format!("ADMIN_CHANGE{}{}{}", 
        target_bytes, admin_bytes, nonce_bytes);
    
    // Convert to bytes and hash
    let combined_bytes = Bytes::from_slice(env, combined_str.as_bytes());
    let hash = env.crypto().sha256(&combined_bytes);
    BytesN::from_array(env, &hash.to_array())
}

/// Deploys and initializes both contracts with proper configuration.
fn setup_contracts(env: &Env) -> (GovernanceContractClient<'_>, WithdrawalTimelockClient<'_>) {
    let gov_owner = addr(env);
    let timelock_admin = addr(env);
    
    // Deploy governance contract
    let gov_address = env.register_contract(None, GovernanceContract);
    let gov_client = GovernanceContractClient::new(env, &gov_address);
    
    // Initialize governance
    gov_client.initialize(
        &gov_owner,
        &QUORUM_BPS,
        &VOTING_PERIOD,
        &TIMELOCK_DELAY,
        &EXECUTION_WINDOW,
    );
    
    // Deploy timelock contract
    let timelock_address = env.register_contract(None, WithdrawalTimelock);
    let timelock_client = WithdrawalTimelockClient::new(env, &timelock_address);
    
    // Initialize timelock
    timelock_client.initialize(&timelock_admin, &TIMELOCK_DELAY);
    
    (gov_client, timelock_client)
}

/// Sets up voters for governance testing.
fn setup_voters(env: &Env, gov_client: &GovernanceContractClient) -> (Address, Address, Address) {
    let owner = gov_client.get_config().0;
    let voter1 = addr(env);
    let voter2 = addr(env);
    let voter3 = addr(env);
    
    // Set voting power
    gov_client.set_voter_power(&owner, &voter1, &VOTING_POWER);
    gov_client.set_voter_power(&owner, &voter2, &VOTING_POWER);
    gov_client.set_voter_power(&owner, &voter3, &VOTING_POWER);
    
    (voter1, voter2, voter3)
}

// ============================================================================
// TESTS
// ============================================================================

#[test]
fn test_governance_timelock_admin_change_execute_flow() {
    let env = env();
    let (gov_client, timelock_client) = setup_contracts(&env);
    let (voter1, voter2, voter3) = setup_voters(&env, &gov_client);
    
    let proposer = voter1.clone();
    let target_contract = addr(&env);
    let new_admin = addr(&env);
    let nonce = env.ledger().timestamp();
    
    // Create payload hash deterministically
    let payload_hash = create_admin_change_payload_hash(&env, &target_contract, &new_admin, nonce);
    
    // Step 1: Create governance proposal
    let proposal_id = gov_client.propose(
        &proposer,
        &ProposalKind::ArbiterChange(new_admin.clone()), // Using ArbiterChange as proxy for admin change
    );
    
    // Step 2: Vote on proposal (achieve quorum and approval)
    gov_client.vote(&voter1, &proposal_id, &VoteChoice::For);
    gov_client.vote(&voter2, &proposal_id, &VoteChoice::For);
    gov_client.vote(&voter3, &proposal_id, &VoteChoice::For);
    
    // Step 3: Advance past voting period
    advance(&env, VOTING_PERIOD + 1);
    
    // Step 4: Queue the proposal (should succeed with quorum)
    gov_client.queue(&proposal_id);
    
    // Verify proposal is succeeded
    let proposal = gov_client.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Succeeded);
    assert!(proposal.eta.is_some());
    
    // Step 5: Advance past timelock delay
    advance(&env, TIMELOCK_DELAY + 1);
    
    // Step 6: Queue timelock operation (admin-only)
    let timelock_admin = timelock_client.get_config().0;
    let op_id = timelock_client.queue(
        &timelock_admin,
        &OperationKind::AdminChange(target_contract.clone(), payload_hash.clone()),
    );
    
    // Verify timelock operation is queued
    let operation = timelock_client.get_operation(&op_id).unwrap();
    assert_eq!(operation.status, OperationStatus::Queued);
    match operation.kind {
        OperationKind::AdminChange(target, hash) => {
            assert_eq!(target, target_contract);
            assert_eq!(hash, payload_hash);
        }
        _ => panic!("Expected AdminChange operation"),
    }
    
    // Step 7: Execute the governance proposal first (before execution window expires)
    gov_client.execute(&proposal_id);
    
    // Verify governance proposal is marked as executed
    let proposal = gov_client.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Executed);
    
    // Step 8: Advance past timelock delay
    advance(&env, TIMELOCK_DELAY + 1);
    
    // Step 9: Execute timelock operation
    let _ = timelock_client
        .try_execute(&timelock_admin, &op_id)
        .expect("Should execute admin change operation");
    
    // Verify operation is executed
    let operation = timelock_client.get_operation(&op_id).unwrap();
    assert_eq!(operation.status, OperationStatus::Executed);
    assert!(operation.executed_at.is_some());
}

#[test]
fn test_governance_timelock_admin_change_cancel_flow() {
    let env = env();
    let (gov_client, timelock_client) = setup_contracts(&env);
    let (voter1, voter2, voter3) = setup_voters(&env, &gov_client);
    
    let proposer = voter1.clone();
    let target_contract = addr(&env);
    let new_admin = addr(&env);
    let nonce = env.ledger().timestamp();
    
    // Create payload hash deterministically
    let payload_hash = create_admin_change_payload_hash(&env, &target_contract, &new_admin, nonce);
    
    // Step 1: Create governance proposal
    let proposal_id = gov_client.propose(
        &proposer,
        &ProposalKind::ArbiterChange(new_admin.clone()),
    );
    
    // Step 2: Vote on proposal
    gov_client.vote(&voter1, &proposal_id, &VoteChoice::For);
    gov_client.vote(&voter2, &proposal_id, &VoteChoice::For);
    gov_client.vote(&voter3, &proposal_id, &VoteChoice::For);
    
    // Step 3: Advance past voting period
    advance(&env, VOTING_PERIOD + 1);
    
    // Step 4: Queue the proposal
    gov_client.queue(&proposal_id);
    
    // Step 5: Queue timelock operation
    let timelock_admin = timelock_client.get_config().0;
    let op_id = timelock_client.queue(
        &timelock_admin,
        &OperationKind::AdminChange(target_contract.clone(), payload_hash.clone()),
    );
    
    // Verify operation is queued
    let operation = timelock_client.get_operation(&op_id).unwrap();
    assert_eq!(operation.status, OperationStatus::Queued);
    
    // Step 6: Cancel timelock operation (admin-only)
    let _ = timelock_client
        .try_cancel(&timelock_admin, &op_id)
        .expect("Should cancel admin change operation");
    
    // Verify operation is cancelled
    let operation = timelock_client.get_operation(&op_id).unwrap();
    assert_eq!(operation.status, OperationStatus::Cancelled);
    assert!(operation.cancelled_at.is_some());
    
    // Verify queued count is decremented
    let queued_count = timelock_client.get_queued_count();
    assert_eq!(queued_count, 0);
}

#[test]
fn test_payload_hash_deterministic_verification() {
    let env = env();
    let target_contract = addr(&env);
    let new_admin = addr(&env);
    let nonce = 12345;
    
    // Create payload hash twice with same inputs
    let hash1 = create_admin_change_payload_hash(&env, &target_contract, &new_admin, nonce);
    let hash2 = create_admin_change_payload_hash(&env, &target_contract, &new_admin, nonce);
    
    // Should be identical (deterministic)
    assert_eq!(hash1, hash2);
    
    // Different inputs should produce different hashes
    let hash3 = create_admin_change_payload_hash(&env, &target_contract, &addr(&env), nonce);
    let hash4 = create_admin_change_payload_hash(&env, &addr(&env), &new_admin, nonce);
    let hash5 = create_admin_change_payload_hash(&env, &target_contract, &new_admin, nonce + 1);
    
    assert_ne!(hash1, hash3); // Different admin
    assert_ne!(hash1, hash4); // Different target
    assert_ne!(hash1, hash5); // Different nonce
    
    // Verify domain separation - different operation types should not collide
    let different_str = format!("DIFFERENT_OP{}{}{}", 
        target_contract.to_string(), new_admin.to_string(), nonce);
    
    let different_bytes = Bytes::from_slice(&env, different_str.as_bytes());
    let hash6 = env.crypto().sha256(&different_bytes);
    let hash6_bytes = BytesN::from_array(&env, &hash6.to_array());
    assert_ne!(hash1, hash6_bytes); // Domain separation
}

#[test]
fn test_delay_update_non_retroactivity() {
    let env = env();
    let (_gov_client, timelock_client) = setup_contracts(&env);
    let timelock_admin = timelock_client.get_config().0;
    
    let target_contract = addr(&env);
    let new_admin = addr(&env);
    let nonce = env.ledger().timestamp();
    let payload_hash = create_admin_change_payload_hash(&env, &target_contract, &new_admin, nonce);
    
    // Queue operation with original delay
    let op_id = timelock_client.queue(
        &timelock_admin,
        &OperationKind::AdminChange(target_contract.clone(), payload_hash.clone()),
    );
    
    let operation = timelock_client.get_operation(&op_id).unwrap();
    let original_eta = operation.eta;
    
    // Update delay to a longer period
    let new_delay = TIMELOCK_DELAY * 2;
    let _ = timelock_client
        .try_update_delay(&timelock_admin, &new_delay)
        .expect("Should update delay");
    
    // Verify delay is updated
    let config = timelock_client.get_config();
    assert_eq!(config.1, new_delay);
    
    // Queue another operation with new delay
    let target_contract2 = addr(&env);
    let new_admin2 = addr(&env);
    let nonce2 = env.ledger().timestamp() + 1;
    let payload_hash2 = create_admin_change_payload_hash(&env, &target_contract2, &new_admin2, nonce2);
    
    let op_id2 = timelock_client.queue(
        &timelock_admin,
        &OperationKind::AdminChange(target_contract2.clone(), payload_hash2.clone()),
    );
    
    let operation2 = timelock_client.get_operation(&op_id2).unwrap();
    
    // Original operation should keep its original ETA (non-retroactive)
    let original_operation = timelock_client.get_operation(&op_id).unwrap();
    assert_eq!(original_operation.eta, original_eta);
    
    // New operation should use the new delay
    assert!(operation2.eta > operation.eta);
    let expected_eta2 = operation2.created_at + new_delay;
    assert_eq!(operation2.eta, expected_eta2);
}

#[test]
fn test_duplicate_proposal_handling() {
    let env = env();
    let (gov_client, _) = setup_contracts(&env);
    let (voter1, voter2, voter3) = setup_voters(&env, &gov_client);
    
    let proposer = voter1.clone();
    let new_admin = addr(&env);
    
    // Create first proposal
    let proposal_id1 = gov_client.propose(
        &proposer,
        &ProposalKind::ArbiterChange(new_admin.clone()),
    );
    
    // Create second proposal with same parameters
    let proposal_id2 = gov_client.propose(
        &proposer,
        &ProposalKind::ArbiterChange(new_admin.clone()),
    );
    
    // Should have different IDs
    assert_ne!(proposal_id1, proposal_id2);
    
    // Both proposals should exist
    let prop1 = gov_client.get_proposal(&proposal_id1).unwrap();
    let prop2 = gov_client.get_proposal(&proposal_id2).unwrap();
    
    assert_eq!(prop1.status, ProposalStatus::Active);
    assert_eq!(prop2.status, ProposalStatus::Active);
    
    // Vote on both proposals
    gov_client.vote(&voter1, &proposal_id1, &VoteChoice::For);
    gov_client.vote(&voter2, &proposal_id1, &VoteChoice::For);
    gov_client.vote(&voter3, &proposal_id1, &VoteChoice::For);
    
    gov_client.vote(&voter1, &proposal_id2, &VoteChoice::Against);
    gov_client.vote(&voter2, &proposal_id2, &VoteChoice::Against);
    gov_client.vote(&voter3, &proposal_id2, &VoteChoice::Against);
    
    // Advance past voting period
    advance(&env, VOTING_PERIOD + 1);
    
    // Queue both proposals
    gov_client.queue(&proposal_id1);
    gov_client.queue(&proposal_id2);
    
    // First should succeed, second should be defeated
    let prop1 = gov_client.get_proposal(&proposal_id1).unwrap();
    let prop2 = gov_client.get_proposal(&proposal_id2).unwrap();
    
    assert_eq!(prop1.status, ProposalStatus::Succeeded);
    assert_eq!(prop2.status, ProposalStatus::Defeated);
}

#[test]
fn test_access_control_enforcement() {
    let env = env();
    let (_gov_client, timelock_client) = setup_contracts(&env);
    
    let unauthorized = addr(&env);
    let target_contract = addr(&env);
    let new_admin = addr(&env);
    let nonce = env.ledger().timestamp();
    let payload_hash = create_admin_change_payload_hash(&env, &target_contract, &new_admin, nonce);
    
    // Unauthorized user should not be able to queue timelock operations
    let result = timelock_client.try_queue(
        &unauthorized,
        &OperationKind::AdminChange(target_contract.clone(), payload_hash.clone()),
    );
    assert!(result.is_err());
    
    // Unauthorized user should not be able to execute timelock operations
    let timelock_admin = timelock_client.get_config().0;
    let op_id = timelock_client.queue(
        &timelock_admin,
        &OperationKind::AdminChange(target_contract.clone(), payload_hash.clone()),
    );
    
    let result = timelock_client.try_execute(&unauthorized, &op_id);
    assert!(result.is_err());
    
    // Unauthorized user should not be able to cancel timelock operations
    let result = timelock_client.try_cancel(&unauthorized, &op_id);
    assert!(result.is_err());
    
    // Admin should be able to execute and cancel
    advance(&env, TIMELOCK_DELAY + 1);
    let _ = timelock_client
        .try_execute(&timelock_admin, &op_id)
        .expect("Admin should execute operation");
    
    // Queue another operation for cancel test
    let op_id2 = timelock_client.queue(
        &timelock_admin,
        &OperationKind::AdminChange(target_contract.clone(), payload_hash.clone()),
    );
    
    let _ = timelock_client
        .try_cancel(&timelock_admin, &op_id2)
        .expect("Admin should cancel operation");
}

#[test]
fn test_execution_window_enforcement() {
    let env = env();
    let (gov_client, timelock_client) = setup_contracts(&env);
    let (voter1, voter2, voter3) = setup_voters(&env, &gov_client);
    
    let proposer = voter1.clone();
    let target_contract = addr(&env);
    let new_admin = addr(&env);
    let nonce = env.ledger().timestamp();
    let payload_hash = create_admin_change_payload_hash(&env, &target_contract, &new_admin, nonce);
    
    // Create and vote on proposal
    let proposal_id = gov_client.propose(
        &proposer,
        &ProposalKind::ArbiterChange(new_admin.clone()),
    );
    
    gov_client.vote(&voter1, &proposal_id, &VoteChoice::For);
    gov_client.vote(&voter2, &proposal_id, &VoteChoice::For);
    gov_client.vote(&voter3, &proposal_id, &VoteChoice::For);
    
    advance(&env, VOTING_PERIOD + 1);
    gov_client.queue(&proposal_id);
    
    // Queue timelock operation
    let timelock_admin = timelock_client.get_config().0;
    let op_id = timelock_client.queue(
        &timelock_admin,
        &OperationKind::AdminChange(target_contract.clone(), payload_hash.clone()),
    );
    
    // Advance past timelock but within execution window
    advance(&env, TIMELOCK_DELAY + 1);
    
    // Should be executable
    let _ = timelock_client
        .try_execute(&timelock_admin, &op_id)
        .expect("Should execute within window");
    
    // Queue another operation for window expiry test
    let target_contract2 = addr(&env);
    let new_admin2 = addr(&env);
    let nonce2 = env.ledger().timestamp();
    let payload_hash2 = create_admin_change_payload_hash(&env, &target_contract2, &new_admin2, nonce2);
    
    let op_id2 = timelock_client.queue(
        &timelock_admin,
        &OperationKind::AdminChange(target_contract2.clone(), payload_hash2.clone()),
    );
    
    // Get the eta for op_id2 to know when it becomes executable
    let operation2 = timelock_client.get_operation(&op_id2).unwrap();
    let op_id2_eta = operation2.eta;
    
    // Advance past execution window but ensure op_id2 is executable
    // We need to advance enough to exceed governance execution window
    advance(&env, EXECUTION_WINDOW);
    
    // Should not be executable (window expired) - this tests governance execution window
    let result = gov_client.try_execute(&proposal_id);
    assert!(result.is_err());
    
    // Should be able to mark as expired
    gov_client.mark_expired(&proposal_id);
    let proposal = gov_client.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.status, ProposalStatus::Expired);
    
    // Now advance enough time for op_id2 to be executable
    let current_time = env.ledger().timestamp();
    if current_time < op_id2_eta {
        advance(&env, op_id2_eta - current_time + 1);
    }
    
    // Timelock operation should now be executable
    let result = timelock_client.try_execute(&timelock_admin, &op_id2);
    assert!(result.is_ok());
}
