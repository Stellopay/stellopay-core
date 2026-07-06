#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};
use stello_pay_contract::storage::{
    Agreement, AgreementMode, AgreementStatus, DataKey, Milestone, PayrollError, StorageKey,
};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> (Address, PayrollContractClient<'static>) {
    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &contract_id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (contract_id, client)
}

fn setup_token<'a>(
    env: &'a Env,
    admin: &Address,
) -> (
    Address,
    soroban_sdk::token::Client<'a>,
    soroban_sdk::token::StellarAssetClient<'a>,
) {
    let token_contract = env.register_stellar_asset_contract_v2(admin.clone());
    let token_client = soroban_sdk::token::Client::new(env, &token_contract.address());
    let token_admin_client =
        soroban_sdk::token::StellarAssetClient::new(env, &token_contract.address());
    (token_contract.address(), token_client, token_admin_client)
}

#[test]
fn test_invariant_escrow_claimed_periods_limit() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, token_admin_client) = setup_token(&env, &token_admin);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    // Create escrow agreement with 2 periods
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token_id,
        &1000i128,
        &3600u64,
        &2u32,
    );

    // Fund contract
    token_admin_client.mint(&contract_id, &2000i128);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_id, 2000i128);
    });
    client.activate_agreement(&agreement_id);

    // Jump 1 hour - claim 1 period
    env.ledger().with_mut(|l| l.timestamp = 3600);
    client.claim_time_based(&agreement_id);

    // Jump another hour - claim 2nd period
    env.ledger().with_mut(|l| l.timestamp = 7200);
    client.claim_time_based(&agreement_id);

    // Verification: agreement is completed
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
}

#[test]
fn test_invariant_milestone_balance_insufficient() {
    let env = create_test_env();
    let (_contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, _token_admin_client) = setup_token(&env, &token_admin);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token_id);
    client.add_milestone(&agreement_id, &1000i128);
    client.add_milestone(&agreement_id, &2000i128);

    // Total unclaimed = 3000. Contract balance = 0.
    // Approving a milestone should trigger the invariant check.
    let result = client.try_approve_milestone(&agreement_id, &1u32);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}

#[test]
fn test_invariant_milestone_balance_sufficient() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, token_admin_client) = setup_token(&env, &token_admin);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token_id);
    client.add_milestone(&agreement_id, &1000i128);

    // Fund contract
    token_admin_client.mint(&employer, &1000i128);
    client.fund_milestone_agreement(&agreement_id, &employer, &1000i128);

    // Should succeed
    client.approve_milestone(&agreement_id, &1u32);
}

#[test]
fn test_invariant_milestone_claim_insufficient_balance() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, token_admin_client) = setup_token(&env, &token_admin);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let agreement_id = client.create_milestone_agreement(&employer, &contributor, &token_id);
    client.add_milestone(&agreement_id, &1000i128);

    // Fund contract
    token_admin_client.mint(&employer, &1000i128);
    client.fund_milestone_agreement(&agreement_id, &employer, &1000i128);
    client.approve_milestone(&agreement_id, &1u32);

    // Now someone steals the funds from the contract (mocked by manual balance update).
    // `claim_milestone` reads this key from *persistent* storage, so the
    // override must target the same storage type or it's invisible to the check.
    env.as_contract(&contract_id, || {
        use stello_pay_contract::storage::MilestoneKey;
        env.storage()
            .persistent()
            .set(&MilestoneKey::MilestoneEscrowBalance(agreement_id), &0i128);
    });

    // Claim should fail due to invariant
    let result = client.try_claim_milestone(&agreement_id, &1u32);
    assert_eq!(result, Err(Ok(PayrollError::InsufficientEscrowBalance)));
}

/// **Conservation invariant: Total escrow must equal paid + remaining at all times**
///
/// This test verifies the fundamental accounting equation for escrow agreements
/// across the full lifecycle: creation, funding, claims, and completion.
#[test]
fn test_invariant_escrow_conservation_across_lifecycle() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, token_admin_client) = setup_token(&env, &token_admin);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);

    let amount_per_period = 500i128;
    let num_periods = 4u32;
    let total_amount = amount_per_period * (num_periods as i128);

    // Create agreement
    let agreement_id = client.create_escrow_agreement(
        &employer,
        &contributor,
        &token_id,
        &amount_per_period,
        &3600u64,
        &num_periods,
    );

    // Fund and activate
    token_admin_client.mint(&contract_id, &total_amount);
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_id, total_amount);
    });
    client.activate_agreement(&agreement_id);

    // Helper: Assert conservation at every step
    let assert_conservation = || {
        env.as_contract(&contract_id, || {
            let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token_id);
            let paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
            assert_eq!(
                remaining + paid,
                total_amount,
                "Conservation violated: remaining={}, paid={}, total={}",
                remaining,
                paid,
                total_amount
            );
        });
    };

    assert_conservation();

    // Claim each period and verify conservation
    for i in 1..=num_periods {
        env.ledger().with_mut(|l| l.timestamp = 3600 * (i as u64));
        client.claim_time_based(&agreement_id);
        assert_conservation();
    }
}

/// **Conservation invariant: Payroll with multiple employees**
///
/// Tests that payroll agreements with multiple employees maintain conservation
/// across individual and batch claims.
#[test]
fn test_invariant_payroll_multi_employee_conservation() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, token_admin_client) = setup_token(&env, &token_admin);

    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let employee3 = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token_id, &604800);

    let salary1 = 1000i128;
    let salary2 = 1500i128;
    let salary3 = 2000i128;
    let total_salary = salary1 + salary2 + salary3;

    client.add_employee_to_agreement(&agreement_id, &employee1, &salary1);
    client.add_employee_to_agreement(&agreement_id, &employee2, &salary2);
    client.add_employee_to_agreement(&agreement_id, &employee3, &salary3);

    let initial_escrow = total_salary * 10;
    token_admin_client.mint(&contract_id, &initial_escrow);

    // Setup DataKey storage
    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, 86400);
        DataKey::set_agreement_token(&env, agreement_id, &token_id);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_id, initial_escrow);
        DataKey::set_employee_count(&env, agreement_id, 3);
        DataKey::set_employee(&env, agreement_id, 0, &employee1);
        DataKey::set_employee_salary(&env, agreement_id, 0, salary1);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
        DataKey::set_employee(&env, agreement_id, 1, &employee2);
        DataKey::set_employee_salary(&env, agreement_id, 1, salary2);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 1, 0);
        DataKey::set_employee(&env, agreement_id, 2, &employee3);
        DataKey::set_employee_salary(&env, agreement_id, 2, salary3);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 2, 0);
    });

    client.activate_agreement(&agreement_id);

    let assert_conservation = || {
        env.as_contract(&contract_id, || {
            let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token_id);
            let paid = DataKey::get_agreement_paid_amount(&env, agreement_id);
            assert_eq!(
                remaining + paid,
                initial_escrow,
                "Conservation violated: {}  + {} != {}",
                remaining,
                paid,
                initial_escrow
            );
        });
    };

    // Claim for different employees across periods
    for _ in 0..3 {
        env.ledger().with_mut(|l| l.timestamp += 86400);

        client.try_claim_payroll(&employee1, &agreement_id, &0).ok();
        assert_conservation();

        client.try_claim_payroll(&employee2, &agreement_id, &1).ok();
        assert_conservation();

        client.try_claim_payroll(&employee3, &agreement_id, &2).ok();
        assert_conservation();
    }
}

/// **Invariant: Dispute resolution respects escrow balance bounds**
///
/// Tests that resolve_dispute never transfers more than the available escrow balance,
/// and that integer division in multi-employee splits doesn't create or lose funds.
#[test]
fn test_invariant_dispute_resolution_bounds() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, token_client, token_admin_client) = setup_token(&env, &token_admin);

    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let employee3 = Address::generate(&env);

    client.set_arbiter(&employer, &arbiter);

    let agreement_id = client.create_payroll_agreement(&employer, &token_id, &604800);

    client.add_employee_to_agreement(&agreement_id, &employee1, &100);
    client.add_employee_to_agreement(&agreement_id, &employee2, &100);
    client.add_employee_to_agreement(&agreement_id, &employee3, &100);

    // Use an amount that creates integer division dust (1000 / 3 = 333.33...)
    let escrow_balance = 1000i128;
    token_admin_client.mint(&contract_id, &escrow_balance);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_id, escrow_balance);
        // `resolve_dispute_core` bounds the payout against `agreement.total_amount`
        // (not just the escrow balance), so the nominal total must be raised to
        // match the escrow this test funds directly, bypassing the normal
        // funding flow that would otherwise keep the two in sync.
        let mut agreement: Agreement = env
            .storage()
            .persistent()
            .get(&StorageKey::Agreement(agreement_id))
            .unwrap();
        agreement.total_amount = escrow_balance;
        env.storage()
            .persistent()
            .set(&StorageKey::Agreement(agreement_id), &agreement);
    });

    client.activate_agreement(&agreement_id);

    // Raise dispute
    client.raise_dispute(&employer, &agreement_id);

    // Resolve: 700 to employees, 300 to employer
    let employee_payout = 700i128;
    let employer_refund = 300i128;

    let initial_total = employee_payout + employer_refund;
    assert_eq!(initial_total, escrow_balance);

    // Track balances before
    let emp1_before = token_client.balance(&employee1);
    let emp2_before = token_client.balance(&employee2);
    let emp3_before = token_client.balance(&employee3);
    let employer_before = token_client.balance(&employer);

    client.resolve_dispute(&arbiter, &agreement_id, &employee_payout, &employer_refund);

    // Verify total distributed equals escrow (no creation or loss of funds)
    let emp1_after = token_client.balance(&employee1);
    let emp2_after = token_client.balance(&employee2);
    let emp3_after = token_client.balance(&employee3);
    let employer_after = token_client.balance(&employer);

    let total_distributed = (emp1_after - emp1_before)
        + (emp2_after - emp2_before)
        + (emp3_after - emp3_before)
        + (employer_after - employer_before);

    // **Invariant: Total distributed must equal initial escrow**
    assert_eq!(
        total_distributed, escrow_balance,
        "Total distributed ({}) != escrow balance ({})",
        total_distributed, escrow_balance
    );

    // Verify minimal remaining dust
    env.as_contract(&contract_id, || {
        let remaining = DataKey::get_agreement_escrow_balance(&env, agreement_id, &token_id);
        assert!(
            remaining <= 2, // At most employee_count - 1 dust
            "Excessive remaining balance: {}",
            remaining
        );
    });
}

/// **Invariant: Claimed periods is monotonic and bounded**
///
/// Verifies that claimed_periods for each employee:
/// 1. Never decreases (monotonic)
/// 2. Never exceeds the number of available periods
#[test]
fn test_invariant_claimed_periods_monotonic_bounded() {
    let env = create_test_env();
    let (contract_id, client) = setup_contract(&env);
    let token_admin = Address::generate(&env);
    let (token_id, _token_client, token_admin_client) = setup_token(&env, &token_admin);

    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let agreement_id = client.create_payroll_agreement(&employer, &token_id, &604800);
    client.add_employee_to_agreement(&agreement_id, &employee, &1000);

    let max_periods = 5u32;
    let escrow = 1000 * (max_periods as i128);
    token_admin_client.mint(&contract_id, &escrow);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_activation_time(&env, agreement_id, env.ledger().timestamp());
        DataKey::set_agreement_period_duration(&env, agreement_id, 3600);
        DataKey::set_agreement_token(&env, agreement_id, &token_id);
        DataKey::set_agreement_escrow_balance(&env, agreement_id, &token_id, escrow);
        DataKey::set_employee_count(&env, agreement_id, 1);
        DataKey::set_employee(&env, agreement_id, 0, &employee);
        DataKey::set_employee_salary(&env, agreement_id, 0, 1000);
        DataKey::set_employee_claimed_periods(&env, agreement_id, 0, 0);
    });

    client.activate_agreement(&agreement_id);

    let mut prev_claimed = 0u32;

    // Attempt to claim beyond max_periods
    for _ in 0..max_periods + 3 {
        env.ledger().with_mut(|l| l.timestamp += 3600);
        client.try_claim_payroll(&employee, &agreement_id, &0).ok();

        env.as_contract(&contract_id, || {
            let claimed = DataKey::get_employee_claimed_periods(&env, agreement_id, 0);

            // **Invariant 1: Monotonic**
            assert!(
                claimed >= prev_claimed,
                "claimed_periods decreased: {} < {}",
                claimed,
                prev_claimed
            );

            // **Invariant 2: Bounded**
            assert!(
                claimed <= max_periods,
                "claimed_periods ({}) exceeded max ({})",
                claimed,
                max_periods
            );

            prev_claimed = claimed;
        });
    }
}
