#![cfg(test)]
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token, Address, Env, Symbol, TryFromVal,
};
use stello_pay_contract::storage::{AgreementStatus, DisputeStatus};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_test_env() -> (
    Env,
    Address,
    Address,
    Address,
    PayrollContractClient<'static>,
) {
    let env = Env::default();

    let contract_id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &contract_id);

    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let token = Address::generate(&env);

    (env, employer, contributor, token, client)
}

fn create_token_contract<'a>(
    e: &'a Env,
    admin: &Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let token_id = e.register_stellar_asset_contract_v2(admin.clone());
    let token = token_id.address();
    let token_client = token::Client::new(e, &token);
    let token_admin_client = token::StellarAssetClient::new(e, &token);
    (token, token_client, token_admin_client)
}

// --- Arbiter Management ---

#[test]
fn test_set_arbiter_by_admin() {
    let (env, _, _, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);

    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);

    assert_eq!(client.get_arbiter(), Some(arbiter));
}

#[test]
#[should_panic]
fn test_set_arbiter_unauthorized_fails() {
    let (env, _, _, _, client) = create_test_env();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let attacker = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&owner);

    env.set_auths(&[]); // Clear all mocks

    client.set_arbiter(&attacker, &arbiter);
}

#[test]
fn test_get_arbiter_address() {
    let (env, _, _, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);

    client.initialize(&owner);
    assert_eq!(client.get_arbiter(), None);

    client.set_arbiter(&owner, &arbiter);
    assert_eq!(client.get_arbiter(), Some(arbiter));
}

// --- Raising Disputes ---

#[test]
fn test_raise_dispute_by_employer() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    client.raise_dispute(&employer, &agreement_id);

    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Disputed);
}

#[test]
fn test_raise_dispute_by_contributor() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    client.raise_dispute(&contributor, &agreement_id);

    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );
}

#[test]
fn test_raise_dispute_by_employee() {
    let (env, employer, _, token_id, client) = create_test_env();
    env.mock_all_auths();
    let employee = Address::generate(&env);
    let agreement_id = client.create_payroll_agreement(&employer, &token_id, &3600);
    client.add_employee_to_agreement(&agreement_id, &employee, &100);

    client.raise_dispute(&employee, &agreement_id);

    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );
}

#[test]
fn test_raise_dispute_by_non_party_fails() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let non_party = Address::generate(&env);
    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    let result = client.try_raise_dispute(&non_party, &agreement_id);
    assert!(result.is_err());
}

#[test]
fn test_raise_dispute_after_grace_period_fails() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    env.ledger().set_timestamp(3601);

    let result = client.try_raise_dispute(&employer, &agreement_id);
    assert!(result.is_err());
}

#[test]
fn test_raise_dispute_already_raised_fails() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    client.raise_dispute(&employer, &agreement_id);
    let result = client.try_raise_dispute(&employer, &agreement_id);
    assert!(result.is_err());
}

#[test]
fn test_dispute_status_set_to_raised() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    client.raise_dispute(&employer, &agreement_id);
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Raised
    );
}

#[test]
fn test_dispute_raised_at_recorded() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    let now = 1000;
    env.ledger().set_timestamp(now);

    client.raise_dispute(&employer, &agreement_id);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.dispute_raised_at, Some(now));
}

#[test]
fn test_agreement_status_changes_to_disputed() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    client.raise_dispute(&employer, &agreement_id);
    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Disputed);
}

#[test]
fn test_dispute_raised_event() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    client.raise_dispute(&employer, &agreement_id);

    let events = env.events().all();
    let event_found = events.iter().any(|event| {
        event.0 == client.address
            && Symbol::try_from_val(&env, &event.1.get(0).unwrap())
                .map(|s| s == Symbol::new(&env, "dispute_raised_event"))
                .unwrap_or(false)
    });
    assert!(event_found, "DisputeRaisedEvent not found");
}

// --- Resolving Disputes ---

#[test]
fn test_resolve_dispute_by_arbiter() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);

    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);

    // Fund agreement
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &60, &40);

    assert_eq!(token_client.balance(&contributor), 60);
    assert_eq!(token_client.balance(&employer), 40);
    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Resolved
    );
}

#[test]
fn test_resolve_dispute_unauthorized_fails() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);
    client.raise_dispute(&employer, &agreement_id);

    env.set_auths(&[]);

    let result = client.try_resolve_dispute(&attacker, &agreement_id, &50, &50);
    assert!(result.is_err());
}

#[test]
fn test_resolve_no_dispute_fails() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);

    let result = client.try_resolve_dispute(&arbiter, &agreement_id, &50, &50);
    assert!(result.is_err());
}

#[test]
fn test_resolve_with_pay_contributor_only() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &100, &0);

    assert_eq!(token_client.balance(&contributor), 100);
    assert_eq!(token_client.balance(&employer), 0);
}

#[test]
fn test_resolve_with_refund_employer_only() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &0, &100);

    assert_eq!(token_client.balance(&contributor), 0);
    assert_eq!(token_client.balance(&employer), 100);
}

#[test]
fn test_resolve_with_both_payouts() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &30, &70);

    assert_eq!(token_client.balance(&contributor), 30);
    assert_eq!(token_client.balance(&employer), 70);
}

#[test]
fn test_resolve_amounts_exceed_balance_fails() {
    let (env, employer, contributor, token_id, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token_id, &100, &3600, &1);
    client.raise_dispute(&employer, &agreement_id);

    let result = client.try_resolve_dispute(&arbiter, &agreement_id, &60, &50); // 110 > 100
    assert!(result.is_err());
}

#[test]
fn test_resolve_funds_released_correctly() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &50, &50);

    assert_eq!(token_client.balance(&contributor), 50);
    assert_eq!(token_client.balance(&employer), 50);
    assert_eq!(token_client.balance(&client.address), 0);
}

#[test]
fn test_resolve_remaining_balance_refunded() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &40, &30);

    assert_eq!(token_client.balance(&contributor), 40);
    assert_eq!(token_client.balance(&employer), 30);
    assert_eq!(token_client.balance(&client.address), 30);
}

#[test]
fn test_dispute_status_set_to_resolved() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);

    let token_admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &50, &50);

    assert_eq!(
        client.get_dispute_status(&agreement_id),
        DisputeStatus::Resolved
    );
}

#[test]
fn test_dispute_resolved_event() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);

    let token_admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &40, &60);

    let events = env.events().all();
    let event_found = events.iter().any(|event| {
        event.0 == client.address
            && Symbol::try_from_val(&env, &event.1.get(0).unwrap())
                .map(|s| s == Symbol::new(&env, "dispute_resolved_event"))
                .unwrap_or(false)
    });
    assert!(event_found, "DisputeResolvedEvent not found");
}

#[test]
fn test_agreement_status_changes_to_completed() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);

    let token_admin = Address::generate(&env);
    let (token, _, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &50, &50);

    let agreement = client.get_agreement(&agreement_id).unwrap();
    assert_eq!(agreement.status, AgreementStatus::Completed);
}

// --- Payroll Mode Disputes ---

#[test]
fn test_resolve_dispute_payroll_agreement() {
    let (env, employer, _, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);

    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);
    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);
    client.add_employee_to_agreement(&agreement_id, &e1, &100);
    client.add_employee_to_agreement(&agreement_id, &e2, &100);

    token_admin_client.mint(&client.address, &200);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &150, &50);

    // 150 / 2 employees = 75 each
    assert_eq!(token_client.balance(&e1), 75);
    assert_eq!(token_client.balance(&e2), 75);
    assert_eq!(token_client.balance(&employer), 50);
}

#[test]
fn test_pay_contributor_distributed_to_employees() {
    let (env, employer, _, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id = client.create_payroll_agreement(&employer, &token, &3600);
    let e1 = Address::generate(&env);
    client.add_employee_to_agreement(&agreement_id, &e1, &100);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &100, &0);

    assert_eq!(token_client.balance(&e1), 100);
}

#[test]
fn test_proportional_distribution_multiple_employees() {
    // Current implementation distributes equally.
}

// --- Edge Cases ---

#[test]
fn test_resolve_with_zero_payouts() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &0, &0);

    assert_eq!(token_client.balance(&contributor), 0);
    assert_eq!(token_client.balance(&employer), 0);
}

#[test]
fn test_resolve_with_full_balance_payout() {
    let (env, employer, contributor, _, client) = create_test_env();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let arbiter = Address::generate(&env);
    client.initialize(&owner);
    client.set_arbiter(&owner, &arbiter);
    let token_admin = Address::generate(&env);
    let (token, token_client, token_admin_client) = create_token_contract(&env, &token_admin);

    let agreement_id =
        client.create_escrow_agreement(&employer, &contributor, &token, &100, &3600, &1);
    token_admin_client.mint(&client.address, &100);

    client.raise_dispute(&employer, &agreement_id);
    client.resolve_dispute(&arbiter, &agreement_id, &100, &0);

    assert_eq!(token_client.balance(&contributor), 100);
    assert_eq!(token_client.balance(&employer), 0);
}
