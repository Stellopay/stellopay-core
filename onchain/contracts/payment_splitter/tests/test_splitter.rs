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
    recipients.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Percent(3333),
    });
    recipients.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Percent(3333),
    });
    recipients.push_back(RecipientShare {
        recipient: c.clone(),
        kind: ShareKind::Percent(3334),
    });

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
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Percent(6000),
    });
    recipients.push_back(RecipientShare {
        recipient: b,
        kind: ShareKind::Percent(4000),
    });

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
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Percent(5000),
    });
    recipients.push_back(RecipientShare {
        recipient: b,
        kind: ShareKind::Percent(5000),
    });

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
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Fixed(300),
    });
    recipients.push_back(RecipientShare {
        recipient: b,
        kind: ShareKind::Fixed(700),
    });

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
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Percent(5000),
    });
    recipients.push_back(RecipientShare {
        recipient: b,
        kind: ShareKind::Percent(5000),
    });

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
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Percent(5000),
    });
    recipients.push_back(RecipientShare {
        recipient: b,
        kind: ShareKind::Percent(5000),
    });

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
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Fixed(300),
    });
    recipients.push_back(RecipientShare {
        recipient: b,
        kind: ShareKind::Fixed(700),
    });

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
    first_order.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Percent(5000),
    });
    first_order.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Percent(5000),
    });

    let mut reversed_order = Vec::new(&env);
    reversed_order.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Percent(5000),
    });
    reversed_order.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Percent(5000),
    });

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
    assert_eq!(
        second_out.get(0).unwrap().1 + second_out.get(1).unwrap().1,
        1
    );
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
    recipients.push_back(RecipientShare {
        recipient: a,
        kind: ShareKind::Percent(3333),
    });
    recipients.push_back(RecipientShare {
        recipient: b,
        kind: ShareKind::Percent(3333),
    });
    recipients.push_back(RecipientShare {
        recipient: c,
        kind: ShareKind::Percent(3334),
    });

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

    // Use the contract's actual maximum (`MAX_RECIPIENTS = 50`); the count
    // previously used here (100) always panics with "Recipient count
    // exceeds maximum" and this test never verified anything beyond that
    // panic (there was no `#[should_panic]`), so it had never actually run
    // successfully. Each share is 200 bps (2%) so 50 of them sum to exactly
    // 10_000 bps (100%), same as the original 100 recipients at 100 bps each.
    let mut recipients = Vec::new(&env);
    for _ in 0..50u32 {
        recipients.push_back(RecipientShare {
            recipient: Address::generate(&env),
            kind: ShareKind::Percent(200),
        });
    }

    let id = client.create_split(&creator, &recipients);
    let out = client.compute_split(&id, &12_345);

    let mut recipients_with_extra_unit = 0u32;
    let mut total = 0i128;
    for entry in out.iter() {
        assert!(entry.1 == 246 || entry.1 == 247);
        if entry.1 == 247 {
            recipients_with_extra_unit += 1;
        }
        total += entry.1;
    }

    assert_eq!(out.len(), 50);
    assert_eq!(recipients_with_extra_unit, 45);
    assert_eq!(total, 12_345);
}

#[test]
fn test_property_conservation_and_bound_many_recipients() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    let mut total_bps = 0;

    for _ in 1..=49u32 {
        recipients.push_back(RecipientShare {
            recipient: Address::generate(&env),
            kind: ShareKind::Percent(100),
        });
        total_bps += 100;
    }
    // Remaining bps to ensure it sums to 10000
    recipients.push_back(RecipientShare {
        recipient: Address::generate(&env),
        kind: ShareKind::Percent(10000 - total_bps),
    });

    let id = client.create_split(&creator, &recipients);

    // Property: for various amounts, sum(parts) == total
    // and dust bound is never violated (the contract won't panic).
    let test_amounts = [1, 10, 199, 200, 201, 1000, 9999, 10000, 10001, 123456789];

    for amount in test_amounts.iter() {
        let out = client.compute_split(&id, amount);
        let sum: i128 = out.iter().map(|entry| entry.1).sum();
        assert_eq!(sum, *amount, "Conservation failed for amount: {}", amount);
    }
}

#[test]
fn test_dust_remainder_goes_to_highest_fractional_remainder() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);

    // 33.3% / 66.7% split on amount=100
    // A: 3333 bps → (3333*100)/10000 = 33.33 → floor=33, remainder=3300
    // B: 6667 bps → (6667*100)/10000 = 66.67 → floor=66, remainder=6700
    // B has larger remainder → gets the dust unit
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Percent(3333),
    });
    recipients.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Percent(6667),
    });

    let id = client.create_split(&creator, &recipients);
    let out = client.compute_split(&id, &100);

    // B should get the extra dust unit (larger fractional remainder)
    let a_amount = if out.get(0).unwrap().0 == a {
        out.get(0).unwrap().1
    } else {
        out.get(1).unwrap().1
    };
    let b_amount = if out.get(0).unwrap().0 == b {
        out.get(0).unwrap().1
    } else {
        out.get(1).unwrap().1
    };

    assert_eq!(a_amount, 33);
    assert_eq!(b_amount, 67);
    assert_eq!(a_amount + b_amount, 100);
}

#[test]
fn test_remainder_tie_goes_to_lexicographically_smaller_address() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);

    // 50/50 split on amount=1 → both have remainder=5000 (tie)
    // Dust=1, tie goes to smaller address
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let a_lt_b = compare_addresses(&env, &a, &b) < 0;

    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Percent(5000),
    });
    recipients.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Percent(5000),
    });

    let id = client.create_split(&creator, &recipients);
    let out = client.compute_split(&id, &1);

    let a_amount = if out.get(0).unwrap().0 == a {
        out.get(0).unwrap().1
    } else {
        out.get(1).unwrap().1
    };
    let b_amount = if out.get(0).unwrap().0 == b {
        out.get(0).unwrap().1
    } else {
        out.get(1).unwrap().1
    };

    if a_lt_b {
        assert_eq!(a_amount, 1);
        assert_eq!(b_amount, 0);
    } else {
        assert_eq!(a_amount, 0);
        assert_eq!(b_amount, 1);
    }
    assert_eq!(a_amount + b_amount, 1);
}

#[test]
fn test_sum_preservation_property_diverse_splits_and_amounts() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);

    // Test multiple split configurations across a range of amounts
    let split_configs = [
        vec![10000],                  // 1 recipient, 100%
        vec![5000, 5000],             // 2 recipients, equal
        vec![6000, 4000],             // 2 recipients, unequal
        vec![3333, 3333, 3334],       // 3 recipients, unequal
        vec![2500, 2500, 2500, 2500], // 4 recipients, equal
        vec![1000, 2000, 3000, 4000], // 4 recipients, unequal
        vec![200; 5],                 // 5 recipients, equal (5*200=1000 ≠ 10000)
    ];

    for (config_idx, bps_list) in split_configs.iter().enumerate() {
        // Skip configs that don't sum to 10000
        let sum: u32 = bps_list.iter().sum();
        if sum != 10000 {
            continue;
        }

        let mut recipients = Vec::new(&env);
        for bps in bps_list.iter() {
            recipients.push_back(RecipientShare {
                recipient: Address::generate(&env),
                kind: ShareKind::Percent(*bps),
            });
        }

        let id = client.create_split(&creator, &recipients);

        // Test every amount from 1 to 200, plus some larger edge values
        for amount in 1..=200i128 {
            let out = client.compute_split(&id, &amount);
            let sum: i128 = out.iter().map(|entry| entry.1).sum();
            assert_eq!(
                sum, amount,
                "Sum preservation failed for config {} amount {}",
                config_idx, amount
            );
        }

        // Edge values
        let edge_amounts = [999, 1000, 5000, 10000, 100000, 999999, i128::MAX / 100];
        for &amount in &edge_amounts {
            let out = client.compute_split(&id, &amount);
            let sum: i128 = out.iter().map(|entry| entry.1).sum();
            assert_eq!(
                sum, amount,
                "Sum preservation failed for config {} amount {}",
                config_idx, amount
            );
        }
    }
}

#[test]
fn test_no_dust_lost_or_created_fixed_splits() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);

    // Fixed splits should pass through exactly (no rounding)
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Fixed(123),
    });
    recipients.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Fixed(456),
    });
    recipients.push_back(RecipientShare {
        recipient: c.clone(),
        kind: ShareKind::Fixed(789),
    });

    let id = client.create_split(&creator, &recipients);
    let total = 123 + 456 + 789;
    let out = client.compute_split(&id, &total);

    let sum: i128 = out.iter().map(|entry| entry.1).sum();
    assert_eq!(sum, total);
    assert_eq!(out.get(0).unwrap().1, 123);
    assert_eq!(out.get(1).unwrap().1, 456);
    assert_eq!(out.get(2).unwrap().1, 789);
}

#[test]
fn test_compute_split_is_deterministic_idempotent() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    let mut recipients = Vec::new(&env);
    // 3333 + 3333 + 3334 = 10000 with a complex rounding case
    recipients.push_back(RecipientShare {
        recipient: a.clone(),
        kind: ShareKind::Percent(3333),
    });
    recipients.push_back(RecipientShare {
        recipient: b.clone(),
        kind: ShareKind::Percent(3333),
    });
    recipients.push_back(RecipientShare {
        recipient: c.clone(),
        kind: ShareKind::Percent(3334),
    });

    let id = client.create_split(&creator, &recipients);

    // Call compute_split multiple times with the exact same inputs
    let first_out = client.compute_split(&id, &100);
    let second_out = client.compute_split(&id, &100);
    let third_out = client.compute_split(&id, &100);

    // Verify share vectors are byte-identical across repeated calls
    assert_eq!(first_out, second_out, "Second call output differs from first");
    assert_eq!(first_out, third_out, "Third call output differs from first");

    // Verify stable rounding allocation (the specific amounts)
    assert_eq!(first_out.get(0).unwrap().1, 33);
    assert_eq!(first_out.get(1).unwrap().1, 33);
    assert_eq!(first_out.get(2).unwrap().1, 34);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_reinitialize_fails() {
    let env = create_env();
    let (_, client) = setup(&env);
    let admin2 = Address::generate(&env);
    client.initialize(&admin2);
}

// ── Single-recipient edge-case tests ─────────────────────────────────────────
//
// A degenerate split with exactly one recipient at the full 10 000 bp
// (100 %) allocation is a valid configuration. The contract should round-trip
// the entire input amount to that single recipient without triggering any of
// the remainder-distribution logic that is only meaningful for multi-recipient
// splits.
//
// Security note: Because `(10000 * total_amount) % 10000 == 0` for every
// integer `total_amount`, the fractional remainder is always zero. This means
// `dust == 0`, which trivially satisfies the `dust < recipient_count` (0 < 1)
// invariant without entering the dust-distribution loop. No value is created
// or destroyed — the entire input amount passes through unchanged.

/// Verify that `create_split` accepts a single recipient at the full 10 000 bp
/// (100 %) allocation and that `compute_split` returns the entire input amount
/// to that recipient with zero remainder for several representative amounts.
///
/// This covers the degenerate percentage-mode path: `bps = 10_000`, so
/// `exact_numerator = 10_000 * total_amount`, `floored = total_amount`,
/// `remainder = 0`, `dust = 0`. No dust-distribution step is entered.
#[test]
fn test_single_recipient_percent_100_no_dust() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let sole = Address::generate(&env);

    // Build a single-recipient split at 100 % (10 000 bps).
    let mut recipients = Vec::new(&env);
    recipients.push_back(RecipientShare {
        recipient: sole.clone(),
        kind: ShareKind::Percent(10000),
    });

    let split_id = client.create_split(&creator, &recipients);

    // Confirm the definition was stored correctly.
    let def = client.get_split(&split_id);
    assert_eq!(def.recipients.len(), 1, "Expected exactly 1 recipient");
    assert!(def.is_percent, "Split should be marked as percentage-based");

    // `validate_split_for_amount` must return true for any positive amount.
    assert!(
        client.validate_split_for_amount(&split_id, &1),
        "validate_split_for_amount should return true for percent splits"
    );
    assert!(
        client.validate_split_for_amount(&split_id, &(i128::MAX / 10000)),
        "validate_split_for_amount should return true for large percent splits"
    );

    // Test a diverse set of amounts: minimum (1), powers-of-ten, prime, large.
    let test_amounts: [i128; 8] = [1, 7, 100, 999, 10_000, 100_003, 1_000_000, i128::MAX / 10_000];

    for &amount in &test_amounts {
        let out = client.compute_split(&split_id, &amount);

        // Exactly one output entry.
        assert_eq!(
            out.len(),
            1,
            "Expected 1 output entry for amount {}",
            amount
        );

        let (addr, allocated) = out.get(0).unwrap();

        // The single output must map to the sole recipient.
        assert_eq!(
            addr, sole,
            "Output recipient mismatch for amount {}",
            amount
        );

        // The entire amount must be returned — no dust lost or created.
        assert_eq!(
            allocated, amount,
            "Single-recipient 100 % split: expected full amount {} but got {}",
            amount, allocated
        );

        // Explicit conservation check (sum of all outputs == input).
        let total_out: i128 = out.iter().map(|e| e.1).sum();
        assert_eq!(
            total_out, amount,
            "Conservation invariant failed for amount {}",
            amount
        );
    }
}

/// Verify that a single-recipient `Fixed` split also round-trips the full
/// amount without loss or remainder distribution.
///
/// Fixed splits bypass the dust-distribution path entirely — each recipient
/// receives exactly their pre-declared fixed amount. For a single recipient
/// whose fixed amount equals `total_amount` this is a strict identity: no
/// arithmetic is performed beyond verifying `fixed_sum == total_amount`.
#[test]
fn test_single_recipient_fixed_full_amount_no_dust() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let sole = Address::generate(&env);

    // Representative amounts to test.
    let test_amounts: [i128; 6] = [1, 50, 1_000, 99_999, 10_000_000, 999_999_999_999];

    for &amount in &test_amounts {
        // Each iteration creates a fresh Fixed split whose declared amount
        // exactly matches the total we will supply to `compute_split`.
        let mut recipients = Vec::new(&env);
        recipients.push_back(RecipientShare {
            recipient: sole.clone(),
            kind: ShareKind::Fixed(amount),
        });

        let split_id = client.create_split(&creator, &recipients);

        // Definition sanity checks.
        let def = client.get_split(&split_id);
        assert_eq!(def.recipients.len(), 1, "Expected exactly 1 recipient");
        assert!(!def.is_percent, "Split should NOT be marked as percentage-based");

        // `validate_split_for_amount` must be true only when total == fixed.
        assert!(
            client.validate_split_for_amount(&split_id, &amount),
            "validate_split_for_amount should be true when total matches fixed amount"
        );

        let out = client.compute_split(&split_id, &amount);

        assert_eq!(
            out.len(),
            1,
            "Expected 1 output entry for fixed amount {}",
            amount
        );

        let (addr, allocated) = out.get(0).unwrap();

        assert_eq!(
            addr, sole,
            "Output recipient mismatch for fixed amount {}",
            amount
        );
        assert_eq!(
            allocated, amount,
            "Single-recipient Fixed split: expected full amount {} but got {}",
            amount, allocated
        );

        // Conservation check.
        let total_out: i128 = out.iter().map(|e| e.1).sum();
        assert_eq!(
            total_out, amount,
            "Conservation invariant failed for fixed amount {}",
            amount
        );
    }
}

/// Assert that both split modes (Percent 10 000 bp and Fixed) behave
/// *identically* for the same input amount: both must deliver the entire
/// amount to the sole recipient, confirming the two code paths are
/// semantically equivalent in the degenerate single-recipient case.
#[test]
fn test_single_recipient_percent_and_fixed_are_equivalent() {
    let env = create_env();
    let (_, client) = setup(&env);
    let creator = Address::generate(&env);
    let sole = Address::generate(&env);

    let amount: i128 = 42_000;

    // --- Percent path ---
    let mut pct_recipients = Vec::new(&env);
    pct_recipients.push_back(RecipientShare {
        recipient: sole.clone(),
        kind: ShareKind::Percent(10000),
    });
    let pct_id = client.create_split(&creator, &pct_recipients);
    let pct_out = client.compute_split(&pct_id, &amount);

    // --- Fixed path ---
    let mut fix_recipients = Vec::new(&env);
    fix_recipients.push_back(RecipientShare {
        recipient: sole.clone(),
        kind: ShareKind::Fixed(amount),
    });
    let fix_id = client.create_split(&creator, &fix_recipients);
    let fix_out = client.compute_split(&fix_id, &amount);

    // Both must return exactly one entry.
    assert_eq!(pct_out.len(), 1);
    assert_eq!(fix_out.len(), 1);

    let (pct_addr, pct_allocated) = pct_out.get(0).unwrap();
    let (fix_addr, fix_allocated) = fix_out.get(0).unwrap();

    // Recipients must be the same.
    assert_eq!(pct_addr, sole);
    assert_eq!(fix_addr, sole);

    // Both must deliver the entire amount.
    assert_eq!(pct_allocated, amount);
    assert_eq!(fix_allocated, amount);

    // Semantic equivalence: the allocated amounts are identical.
    assert_eq!(
        pct_allocated, fix_allocated,
        "Percent and Fixed single-recipient splits must return identical amounts"
    );
}
