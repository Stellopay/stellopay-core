//! Dispute resolution integration tests (payroll multi-employee split + funded escrow resolve).
//! Replaces disabled suite; uses same env pattern as former test_disputes.rs.disabled.
#![cfg(test)]

use soroban_sdk::{testutils::Address as _, token, Address, Env};
use stello_pay_contract::storage::{AgreementStatus, DisputeStatus};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn env_client() -> (Env, Address, PayrollContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(&env, &id);
    (env, id, client)
}

fn stellar_token<'a>(
    e: &'a Env,
    admin: &Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let t = e.register_stellar_asset_contract_v2(admin.clone());
    (t.address(), token::Client::new(e, &t.address()), token::StellarAssetClient::new(e, &t.address()))
}

/// Payroll mode: arbiter split distributes pay_employee equally among employees.
#[test]
fn test_dispute_payroll_multi_employee_split() {
    let (env, cid, client) = env_client();
    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (tok, tok_client, tok_admin) = stellar_token(&env, &token_admin);
    let e1 = Address::generate(&env);
    let e2 = Address::generate(&env);

    client.initialize(&employer);
    client.set_arbiter(&employer, &arbiter);
    let aid = client.create_payroll_agreement(&employer, &tok, &86400);
    client.add_employee_to_agreement(&aid, &e1, &100);
    client.add_employee_to_agreement(&aid, &e2, &100);
    tok_admin.mint(&cid, &200);

    client.raise_dispute(&employer, &aid);
    client.resolve_dispute(&arbiter, &aid, &150, &50);

    assert_eq!(tok_client.balance(&e1), 75);
    assert_eq!(tok_client.balance(&e2), 75);
    assert_eq!(tok_client.balance(&employer), 50);
    assert_eq!(client.get_dispute_status(&aid), DisputeStatus::Resolved);
    assert_eq!(
        client.get_agreement(&aid).unwrap().status,
        AgreementStatus::Completed
    );
}

/// Escrow mode: funded contract resolves with contributor/employer token split.
#[test]
fn test_dispute_escrow_funded_resolve_split() {
    let (env, cid, client) = env_client();
    let employer = Address::generate(&env);
    let contributor = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (tok, tok_client, tok_admin) = stellar_token(&env, &token_admin);

    client.initialize(&employer);
    client.set_arbiter(&employer, &arbiter);
    let aid = client.create_escrow_agreement(&employer, &contributor, &tok, &1000, &3600, &1);
    tok_admin.mint(&cid, &1000);

    client.raise_dispute(&employer, &aid);
    client.resolve_dispute(&arbiter, &aid, &600, &400);

    assert_eq!(tok_client.balance(&contributor), 600);
    assert_eq!(tok_client.balance(&employer), 400);
    assert_eq!(client.get_dispute_status(&aid), DisputeStatus::Resolved);
}
