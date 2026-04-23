//! Tests for Hardened Payment Splitting Contract
#![cfg(test)]

use payment_splitter::{
    PaymentSplitterContract, PaymentSplitterContractClient, RecipientShare, ShareKind,
};
use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, Bytes, Env, Vec};

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

fn compare_addresses(env: &Env, left: &Address, right: &Address) -> i32 {
    let left_xdr: Bytes = left.clone().to_xdr(env);
    let right_xdr: Bytes = right.clone().to_xdr(env);
    let min_len = if left_xdr.len() < right_xdr.len() {
        left_xdr.len()
    } else {
        right_xdr.len()
    };

    for i in 0..min_len {
        let left_byte = left_xdr.get_unchecked(i);
        let right_byte = right_xdr.get_unchecked(i);
        if left_byte < right_byte {
            return -1;
        }
        if left_byte > right_byte {
            return 1;
        }
    }

    if left_xdr.len() < right_xdr.len() {
        -1
    } else if left_xdr.len() > right_xdr.len() {
        1
    } else {
        0
    }
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
    let out = client.compute_split(&id, &1);

    let a_amount = out.get(0).unwrap().1;
    let b_amount = out.get(1).unwrap().1;
    let a_wins_tie = compare_addresses(&env, &out.get(0).unwrap().0, &out.get(1).unwrap().0) < 0;

    assert_eq!(a_amount + b_amount, 1);
    if a_wins_tie {
        assert_eq!(a_amount, 1);
        assert_eq!(b_amount, 0);
    } else {
        assert_eq!(a_amount, 0);
        assert_eq!(b_amount, 1);
    }
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
#[should_panic(expected = "Total amount must be > 0")]
fn test_compute_split_zero_amount_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare { recipient: a, kind: ShareKind::Percent(5000) });
    recipients.push_back(RecipientShare { recipient: b, kind: ShareKind::Percent(5000) });

    let id = client.create_split(&creator, &recipients);
    client.compute_split(&id, &0);
}

#[test]
#[should_panic(expected = "Total amount must be > 0")]
fn test_compute_split_negative_amount_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare { recipient: a, kind: ShareKind::Percent(5000) });
    recipients.push_back(RecipientShare { recipient: b, kind: ShareKind::Percent(5000) });

    let id = client.create_split(&creator, &recipients);
    client.compute_split(&id, &-1);
}

#[test]
#[should_panic(expected = "Fixed split total must equal sum of fixed amounts")]
fn test_fixed_split_mismatched_total_rejected() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare { recipient: a, kind: ShareKind::Fixed(300) });
    recipients.push_back(RecipientShare { recipient: b, kind: ShareKind::Fixed(700) });

    let id = client.create_split(&creator, &recipients);
    client.compute_split(&id, &999);
}

#[test]
fn test_dust_tie_breaker_ignores_input_order() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let mut first_order = Vec::new(&env);
    first_order.push_back(RecipientShare { recipient: a.clone(), kind: ShareKind::Percent(5000) });
    first_order.push_back(RecipientShare { recipient: b.clone(), kind: ShareKind::Percent(5000) });

    let mut reversed_order = Vec::new(&env);
    reversed_order.push_back(RecipientShare { recipient: b.clone(), kind: ShareKind::Percent(5000) });
    reversed_order.push_back(RecipientShare { recipient: a.clone(), kind: ShareKind::Percent(5000) });

    let first_id = client.create_split(&creator, &first_order);
    let second_id = client.create_split(&creator, &reversed_order);

    let first_out = client.compute_split(&first_id, &1);
    let second_out = client.compute_split(&second_id, &1);

    let first_a = if first_out.get(0).unwrap().0 == a {
        first_out.get(0).unwrap().1
    } else {
        first_out.get(1).unwrap().1
    };
    let second_a = if second_out.get(0).unwrap().0 == a {
        second_out.get(0).unwrap().1
    } else {
        second_out.get(1).unwrap().1
    };

    assert_eq!(first_a, second_a);
    assert_eq!(first_out.get(0).unwrap().1 + first_out.get(1).unwrap().1, 1);
    assert_eq!(second_out.get(0).unwrap().1 + second_out.get(1).unwrap().1, 1);
}

#[test]
fn test_repeated_percent_splits_do_not_lose_or_create_value() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare { recipient: a, kind: ShareKind::Percent(3333) });
    recipients.push_back(RecipientShare { recipient: b, kind: ShareKind::Percent(3333) });
    recipients.push_back(RecipientShare { recipient: c, kind: ShareKind::Percent(3334) });

    let id = client.create_split(&creator, &recipients);

    let mut total_input = 0i128;
    let mut total_output = 0i128;
    for amount in 1..=257i128 {
        let out = client.compute_split(&id, &amount);
        let split_sum = out.iter().map(|entry| entry.1).sum::<i128>();
        assert_eq!(split_sum, amount);
        total_input += amount;
        total_output += split_sum;
    }

    assert_eq!(total_output, total_input);
}

#[test]
fn test_compute_split_extreme_recipient_count() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    for _ in 0..100u32 {
        recipients.push_back(RecipientShare {
            recipient: Address::generate(&env),
            kind: ShareKind::Percent(100),
        });
    }

    let id = client.create_split(&creator, &recipients);
    let out = client.compute_split(&id, &12_345);

    let mut recipients_with_extra_unit = 0u32;
    let mut total = 0i128;
    for entry in out.iter() {
        assert!(entry.1 == 123 || entry.1 == 124);
        if entry.1 == 124 {
            recipients_with_extra_unit += 1;
        }
        total += entry.1;
    }

    assert_eq!(out.len(), 100);
    assert_eq!(recipients_with_extra_unit, 45);
    assert_eq!(total, 12_345);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_reinitialize_fails() {
    let env = create_env();
    let (_, client) = setup(&env);
    let admin2 = Address::generate(&env);
    client.initialize(&admin2);
}
