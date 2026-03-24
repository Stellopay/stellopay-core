//! Tests for Hardened Payment Splitting Contract
#![cfg(test)]

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
    let contract_id = env.register_contract(None, PaymentSplitterContract);
    let client = PaymentSplitterContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, client)
}

#[test]
fn test_create_split_percent_success() {
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
    let def = client.get_split(&id);
    assert_eq!(def.recipients.len(), 2);
    assert!(def.is_percent);
}

#[test]
#[should_panic(expected = "Duplicate recipient address")]
fn test_create_split_duplicate_recipient() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Percent(5000),
    });
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Percent(5000),
    });
    client.create_split(&creator, &recipients);
}

#[test]
#[should_panic(expected = "Percentage-based share must be > 0")]
fn test_create_split_zero_percent() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Percent(10000),
    });
    recipients.push_back(RecipientShare {
        recipient: b,
        kind: ShareKind::Percent(0),
    });
    client.create_split(&creator, &recipients);
}

#[test]
#[should_panic(expected = "Split must be either all Percentage or all Fixed")]
fn test_create_split_mixed_modes() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Percent(5000),
    });
    recipients.push_back(RecipientShare {
        recipient: b,
        kind: ShareKind::Fixed(100),
    });
    client.create_split(&creator, &recipients);
}

#[test]
fn test_compute_split_rounding_dust() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    
    let mut recipients = Vec::new(&env);
    // 3333 + 3333 + 3334 = 10000
    recipients.push_back(RecipientShare { recipient: a.clone(), kind: ShareKind::Percent(3333) });
    recipients.push_back(RecipientShare { recipient: b.clone(), kind: ShareKind::Percent(3333) });
    recipients.push_back(RecipientShare { recipient: c.clone(), kind: ShareKind::Percent(3334) });
    
    let id = client.create_split(&creator, &recipients);
    
    // Total = 100
    // A: (3333 * 100) / 10000 = 33
    // B: (3333 * 100) / 10000 = 33
    // C: 100 - (33 + 33) = 34
    let out = client.compute_split(&id, &100);
    
    assert_eq!(out.get(0).unwrap().1, 33);
    assert_eq!(out.get(1).unwrap().1, 33);
    assert_eq!(out.get(2).unwrap().1, 34);
    
    let total_comp: i128 = out.iter().map(|x| x.1).sum();
    assert_eq!(total_comp, 100);
}

#[test]
fn test_compute_split_prime_number() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare { recipient: a, kind: ShareKind::Percent(6000) });
    recipients.push_back(RecipientShare { recipient: b, kind: ShareKind::Percent(4000) });
    
    let id = client.create_split(&creator, &recipients);
    
    // Total = 107 (prime)
    // A: (6000 * 107) / 10000 = 64.2 -> 64
    // B: 107 - 64 = 43
    let out = client.compute_split(&id, &107);
    assert_eq!(out.get(0).unwrap().1, 64);
    assert_eq!(out.get(1).unwrap().1, 43);
    assert_eq!(out.get(0).unwrap().1 + out.get(1).unwrap().1, 107);
}

#[test]
fn test_compute_split_one_stroop() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare { recipient: a, kind: ShareKind::Percent(5000) });
    recipients.push_back(RecipientShare { recipient: b, kind: ShareKind::Percent(5000) });
    
    let id = client.create_split(&creator, &recipients);
    
    // Total = 1
    // A: (5000 * 1) / 10000 = 0
    // B: 1 - 0 = 1
    let out = client.compute_split(&id, &1);
    assert_eq!(out.get(0).unwrap().1, 0);
    assert_eq!(out.get(1).unwrap().1, 1);
}

#[test]
fn test_fixed_split_validation() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare { recipient: a, kind: ShareKind::Fixed(300) });
    recipients.push_back(RecipientShare { recipient: b, kind: ShareKind::Fixed(700) });
    
    let id = client.create_split(&creator, &recipients);
    
    assert!(client.validate_split_for_amount(&id, &1000));
    assert!(!client.validate_split_for_amount(&id, &999));
    
    let out = client.compute_split(&id, &1000);
    assert_eq!(out.get(0).unwrap().1, 300);
    assert_eq!(out.get(1).unwrap().1, 700);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_reinitialize_fails() {
    let env = create_env();
    let (_, client) = setup(&env);
    let admin2 = Address::generate(&env);
    client.initialize(&admin2);
}
