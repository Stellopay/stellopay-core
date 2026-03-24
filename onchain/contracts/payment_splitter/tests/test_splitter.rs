//! Tests for Payment Splitting Contract (#206).

#![cfg(test)]
#![allow(deprecated)]

use payment_splitter::{
    PaymentSplitterContract, PaymentSplitterContractClient, RecipientShare, ShareKind,
};
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

fn create_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup(env: &Env) -> (Address, PaymentSplitterContractClient<'_>) {
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, PaymentSplitterContract);
    let client = PaymentSplitterContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, client)
}

#[test]
fn test_create_split_percent() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Percent(6000),
    });
    recipients.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Percent(4000),
    });
    let id = client.create_split(&creator, &recipients);
    assert_eq!(id, 1);
    let def = client.get_split(&id);
    assert_eq!(def.recipients.len(), 2);
}

#[test]
#[should_panic(expected = "Percent shares must sum to 10000")]
fn test_create_split_percent_invalid_sum() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Percent(5000),
    });
    client.create_split(&creator, &recipients);
}

#[test]
fn test_compute_split_percent() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Percent(6000),
    });
    recipients.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Percent(4000),
    });
    let id = client.create_split(&creator, &recipients);
    let out = client.compute_split(&id, &1000);
    assert_eq!(out.len(), 2);
    let first = out.get(0).unwrap();
    let second = out.get(1).unwrap();
    assert_eq!(first.0, a);
    assert_eq!(first.1, 600);
    assert_eq!(second.0, b);
    assert_eq!(second.1, 400);
}

#[test]
fn test_compute_split_fixed() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Fixed(300),
    });
    recipients.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Fixed(700),
    });
    let id = client.create_split(&creator, &recipients);
    let out = client.compute_split(&id, &1000);
    assert_eq!(out.get(0).unwrap().1, 300);
    assert_eq!(out.get(1).unwrap().1, 700);
    assert!(client.validate_split_for_amount(&id, &1000));
    assert!(!client.validate_split_for_amount(&id, &500));
}
