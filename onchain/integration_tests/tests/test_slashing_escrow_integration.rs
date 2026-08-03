#![cfg(test)]
#![allow(deprecated)]

use payroll_escrow::{PayrollEscrowContract, PayrollEscrowContractClient};
use slashing_penalty::{Offense, SlashingPenaltyContract, SlashingPenaltyContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env,
};

// ─────────────────────────────────────────────────────────────────────────────
// Shared constants
// ─────────────────────────────────────────────────────────────────────────────

const PENALTY_BPS: u32 = 1000; // 10%
const STAKE_AMOUNT: i128 = 10_000;
const ESCROW_FUND: i128 = 50_000;

// ─────────────────────────────────────────────────────────────────────────────
// Test environment helpers
// ─────────────────────────────────────────────────────────────────────────────

fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e.ledger().with_mut(|li| li.timestamp = 100_000);
    e
}

fn addr(e: &Env) -> Address {
    Address::generate(e)
}

fn advance(e: &Env, delta: u64) {
    e.ledger().with_mut(|li| li.timestamp += delta);
}

fn create_token(e: &Env, admin: &Address) -> Address {
    e.register_stellar_asset_contract_v2(admin.clone())
        .address()
}

// ─────────────────────────────────────────────────────────────────────────────
// Orchestrator mock
// ─────────────────────────────────────────────────────────────────────────────
// This models an off-chain or on-chain orchestrator that bridges the two contracts.
// It verifies the slash was executed in slashing_penalty, then deducts from payroll_escrow.
struct MockOrchestrator<'a> {
    env: &'a Env,
    slashing_client: SlashingPenaltyContractClient<'a>,
    escrow_client: PayrollEscrowContractClient<'a>,
    orchestrator_addr: Address,
}

impl<'a> MockOrchestrator<'a> {
    fn new(
        env: &'a Env,
        slashing_id: &Address,
        escrow_id: &Address,
        orchestrator_addr: Address,
    ) -> Self {
        Self {
            env,
            slashing_client: SlashingPenaltyContractClient::new(env, slashing_id),
            escrow_client: PayrollEscrowContractClient::new(env, escrow_id),
            orchestrator_addr,
        }
    }

    /// Syncs a slash event to the escrow contract, effectively reducing the available balance.
    fn sync_slash_to_escrow(
        &self,
        evidence_hash: BytesN<32>,
        agreement_id: u128,
        treasury: &Address,
    ) {
        // 1. Verify the slash is actually executed in slashing_penalty
        let record = self
            .slashing_client
            .get_slash_record(&evidence_hash)
            .expect("Slash record not found");

        // Use matching instead of direct comparison to avoid importing SlashStatus
        let is_executed = match record.status {
            slashing_penalty::SlashStatus::Executed => true,
            _ => false,
        };
        assert!(is_executed, "Slash is not executed");

        // 2. Reduce the party's payroll_escrow balance by releasing the escrowed amount
        // to a treasury/burn address.
        self.escrow_client.release(
            &self.orchestrator_addr,
            &agreement_id,
            treasury,
            &record.escrowed_amount,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_slashing_reduces_payroll_escrow_available_balance() {
    let env = env();
    let token_admin = addr(&env);
    let token = create_token(&env, &token_admin);
    let token_admin_client = StellarAssetClient::new(&env, &token);

    // Setup accounts
    let admin = addr(&env);
    let employer = addr(&env);
    let employee = addr(&env); // The party to be slashed
    let slasher = addr(&env);
    let orchestrator_addr = addr(&env); // Manager of payroll_escrow
    let treasury = addr(&env);

    // Setup token balances
    token_admin_client.mint(&employer, &ESCROW_FUND);
    token_admin_client.mint(&employee, &STAKE_AMOUNT);

    // 1. Deploy & Init Slashing Penalty
    let slashing_id = env.register(SlashingPenaltyContract, ());
    let slashing_client = SlashingPenaltyContractClient::new(&env, &slashing_id);
    slashing_client.initialize(
        &admin,
        &token,
        &2u32,        // quorum
        &5_000u32,    // per_event_bps_cap
        &100_000i128, // per_period_amount_cap
        &100_000i128, // lifetime_amount_cap
        &86400u64,    // period_secs
    );
    slashing_client.add_slasher(&admin, &slasher);

    // Employee stakes in slashing_penalty
    slashing_client.stake(&employee, &STAKE_AMOUNT);
    assert_eq!(slashing_client.get_stake_balance(&employee), STAKE_AMOUNT);

    // 2. Deploy & Init Payroll Escrow
    let escrow_id = env.register(PayrollEscrowContract, ());
    let escrow_client = PayrollEscrowContractClient::new(&env, &escrow_id);
    // Initialize with the orchestrator as the manager
    escrow_client.initialize(&admin, &token, &orchestrator_addr);

    // Employer funds an agreement for the employee
    let agreement_id = 1u128;
    escrow_client.fund_agreement(&employer, &agreement_id, &employer, &ESCROW_FUND);

    // Verify initial escrow available balance
    let initial_balance = escrow_client.get_agreement_balance(&agreement_id);
    assert_eq!(initial_balance, ESCROW_FUND);

    // 3. Execute Slash
    let mut evidence_data = [0u8; 32];
    evidence_data[0] = 1; // dummy hash
    let evidence_hash = BytesN::from_array(&env, &evidence_data);
    let offense_ts = env.ledger().timestamp();

    // Slasher slashes employee
    slashing_client.slash_with_evidence(
        &slasher,
        &employee,
        &Offense::DoubleSigning,
        &PENALTY_BPS,
        &evidence_hash,
        &offense_ts,
    );

    // Advance past appeal window (7 days = 604_800 secs)
    advance(&env, 700_000);

    // Finalize the slash
    slashing_client.execute_slash(&evidence_hash);

    let slash_record = slashing_client.get_slash_record(&evidence_hash).unwrap();
    let slashed_amount = slash_record.escrowed_amount;
    assert_eq!(
        slashed_amount,
        (STAKE_AMOUNT * (PENALTY_BPS as i128)) / 10000
    );

    // 4. Orchestrator syncs the slash to payroll_escrow
    let orchestrator = MockOrchestrator::new(&env, &slashing_id, &escrow_id, orchestrator_addr);
    orchestrator.sync_slash_to_escrow(evidence_hash.clone(), agreement_id, &treasury);

    // 5. Assert payroll_escrow's available-balance reflects the post-slash reduced amount
    let final_balance = escrow_client.get_agreement_balance(&agreement_id);
    assert_eq!(final_balance, ESCROW_FUND - slashed_amount);

    // Verify treasury received the slashed funds
    let token_client = TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&treasury), slashed_amount);
}

#[test]
fn test_unrelated_party_escrow_unaffected_by_slash() {
    let env = env();
    let token_admin = addr(&env);
    let token = create_token(&env, &token_admin);
    let token_admin_client = StellarAssetClient::new(&env, &token);

    let admin = addr(&env);
    let employer = addr(&env);
    let employee1 = addr(&env); // To be slashed
    let employee2 = addr(&env); // Unrelated party
    let slasher = addr(&env);
    let orchestrator_addr = addr(&env);
    let treasury = addr(&env);

    token_admin_client.mint(&employer, &(ESCROW_FUND * 2));
    token_admin_client.mint(&employee1, &STAKE_AMOUNT);

    // Deploy Slashing Penalty
    let slashing_id = env.register(SlashingPenaltyContract, ());
    let slashing_client = SlashingPenaltyContractClient::new(&env, &slashing_id);
    slashing_client.initialize(
        &admin,
        &token,
        &2u32,
        &5_000u32,
        &100_000i128,
        &100_000i128,
        &86400u64,
    );
    slashing_client.add_slasher(&admin, &slasher);
    slashing_client.stake(&employee1, &STAKE_AMOUNT);

    // Deploy Payroll Escrow
    let escrow_id = env.register(PayrollEscrowContract, ());
    let escrow_client = PayrollEscrowContractClient::new(&env, &escrow_id);
    escrow_client.initialize(&admin, &token, &orchestrator_addr);

    let agreement_id_1 = 1u128;
    let agreement_id_2 = 2u128;
    escrow_client.fund_agreement(&employer, &agreement_id_1, &employer, &ESCROW_FUND);
    escrow_client.fund_agreement(&employer, &agreement_id_2, &employer, &ESCROW_FUND);

    // Slash employee1
    let evidence_hash = BytesN::from_array(&env, &[2u8; 32]);
    slashing_client.slash_with_evidence(
        &slasher,
        &employee1,
        &Offense::DoubleSigning,
        &PENALTY_BPS,
        &evidence_hash,
        &env.ledger().timestamp(),
    );

    advance(&env, 700_000);
    slashing_client.execute_slash(&evidence_hash);

    let orchestrator = MockOrchestrator::new(&env, &slashing_id, &escrow_id, orchestrator_addr);
    orchestrator.sync_slash_to_escrow(evidence_hash.clone(), agreement_id_1, &treasury);

    // Assert employee1's balance is reduced
    let record = slashing_client.get_slash_record(&evidence_hash).unwrap();
    assert_eq!(
        escrow_client.get_agreement_balance(&agreement_id_1),
        ESCROW_FUND - record.escrowed_amount
    );

    // Assert unrelated party (employee2) is unaffected
    assert_eq!(
        escrow_client.get_agreement_balance(&agreement_id_2),
        ESCROW_FUND
    );
}
