#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, Symbol, TryFromVal,
};
use stello_pay_contract::storage::{DataKey, GracePeriodExtensionPolicy, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn setup(env: &Env) -> (Address, PayrollContractClient<'static>, Address) {
    let id = env.register(PayrollContract, ());
    let client = PayrollContractClient::new(env, &id);
    let owner = Address::generate(env);
    client.initialize(&owner);
    (id, client, owner)
}

fn cancel_payroll_agreement(
    env: &Env,
    client: &PayrollContractClient,
    employer: &Address,
    token: &Address,
    base_grace: u64,
) -> u128 {
    let aid = client.create_payroll_agreement(employer, token, &base_grace);
    let emp = Address::generate(env);
    client.add_employee_to_agreement(&aid, &emp, &1000_i128);
    client.activate_agreement(&aid);
    client.cancel_agreement(&aid);
    aid
}

#[test]
fn test_employer_extend_updates_end_and_extension_storage() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let base = 1000_u64;
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, base);

    assert_eq!(client.get_grace_extension_seconds(&aid), 0);
    let end_before = client.get_grace_period_end(&aid).unwrap();

    client.extend_grace_period(&employer, &aid, &500_u64);

    assert_eq!(client.get_grace_extension_seconds(&aid), 500);
    let end_after = client.get_grace_period_end(&aid).unwrap();
    assert_eq!(end_after, end_before + 500);
    assert!(client.is_grace_period_active(&aid));
}

#[test]
fn test_owner_can_extend() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, owner) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, 2000);

    client.extend_grace_period(&owner, &aid, &100);
    assert_eq!(client.get_grace_extension_seconds(&aid), 100);
}

#[test]
fn test_non_party_cannot_extend() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, 1000);
    let stranger = Address::generate(&env);

    let e = client
        .try_extend_grace_period(&stranger, &aid, &10_u64)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::Unauthorized);
}

#[test]
fn test_extend_active_agreement_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let aid = client.create_payroll_agreement(&employer, &token, &86400_u64);
    let emp = Address::generate(&env);
    client.add_employee_to_agreement(&aid, &emp, &1000_i128);
    client.activate_agreement(&aid);

    let e = client
        .try_extend_grace_period(&employer, &aid, &10_u64)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::GraceExtensionInvalid);
}

#[test]
fn test_extend_zero_seconds_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, 100);

    let e = client
        .try_extend_grace_period(&employer, &aid, &0_u64)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::GraceExtensionInvalid);
}

#[test]
fn test_extend_unknown_agreement_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);

    let e = client
        .try_extend_grace_period(&employer, &999_u128, &1_u64)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::AgreementNotFound);
}

#[test]
fn test_per_call_cap_enforced() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, owner) = setup(&env);
    client.set_grace_extension_policy(
        &owner,
        &GracePeriodExtensionPolicy {
            max_cumulative_extension_bps: 100_000,
            max_extension_per_call_seconds: 50,
        },
    );

    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, 1000);

    let e = client
        .try_extend_grace_period(&employer, &aid, &51_u64)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::GraceExtensionInvalid);
}

#[test]
fn test_cumulative_cap_enforced() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, owner) = setup(&env);
    // Max extra = base * 10000/10000 = base
    client.set_grace_extension_policy(
        &owner,
        &GracePeriodExtensionPolicy {
            max_cumulative_extension_bps: 10_000,
            max_extension_per_call_seconds: 600,
        },
    );

    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let base = 100_u64;
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, base);

    client.extend_grace_period(&employer, &aid, &60);
    client.extend_grace_period(&employer, &aid, &40);
    assert_eq!(client.get_grace_extension_seconds(&aid), 100);

    let e = client
        .try_extend_grace_period(&employer, &aid, &1_u64)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::GraceExtensionCapExceeded);
}

#[test]
fn test_revive_window_after_base_grace_expired() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, owner) = setup(&env);
    // Default cap is 100% of base; allow 200s extra when base grace is 100s.
    client.set_grace_extension_policy(
        &owner,
        &GracePeriodExtensionPolicy {
            max_cumulative_extension_bps: 30_000,
            max_extension_per_call_seconds: 500,
        },
    );
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let base = 100_u64;
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, base);

    env.ledger().with_mut(|li| {
        li.timestamp += base + 10;
    });
    assert!(!client.is_grace_period_active(&aid));

    client.extend_grace_period(&employer, &aid, &200);
    assert!(client.is_grace_period_active(&aid));
}

#[test]
fn test_raise_dispute_respects_extension_after_cancel() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, owner) = setup(&env);
    client.set_grace_extension_policy(
        &owner,
        &GracePeriodExtensionPolicy {
            max_cumulative_extension_bps: 250_000,
            max_extension_per_call_seconds: 500,
        },
    );
    let employer = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    client.set_arbiter(&employer, &arbiter);

    let base = 50_u64;
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, base);

    env.ledger().with_mut(|li| {
        li.timestamp += base + 5;
    });
    let e = client
        .try_raise_dispute(&employer, &aid)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::NotInGracePeriod);

    client.extend_grace_period(&employer, &aid, &100);
    client.raise_dispute(&employer, &aid);
}

#[test]
fn test_emergency_pause_blocks_extend() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, 1000);

    client.emergency_pause();
    let e = client
        .try_extend_grace_period(&employer, &aid, &10_u64)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::EmergencyPaused);

    client.emergency_unpause();
    client.extend_grace_period(&employer, &aid, &10);
}

#[test]
fn test_set_grace_extension_policy_owner_only() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, owner) = setup(&env);
    let stranger = Address::generate(&env);
    let p = GracePeriodExtensionPolicy {
        max_cumulative_extension_bps: 5000,
        max_extension_per_call_seconds: 3600,
    };
    let e = client
        .try_set_grace_extension_policy(&stranger, &p)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::Unauthorized);

    client.set_grace_extension_policy(&owner, &p);
    let got = client.get_grace_extension_policy();
    assert_eq!(got.max_cumulative_extension_bps, 5000);
    assert_eq!(got.max_extension_per_call_seconds, 3600);
}

#[test]
fn test_set_grace_extension_policy_rejects_absurd_bps() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, owner) = setup(&env);
    let p = GracePeriodExtensionPolicy {
        max_cumulative_extension_bps: 500_001,
        max_extension_per_call_seconds: 3600,
    };
    let e = client
        .try_set_grace_extension_policy(&owner, &p)
        .unwrap_err()
        .unwrap();
    assert_eq!(e, PayrollError::GraceExtensionInvalid);
}

#[test]
fn test_grace_period_extended_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();

    let (_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, 500);

    client.extend_grace_period(&employer, &aid, &123);

    let events = env.events().all();
    let found = events.iter().any(|e| {
        if e.1.len() > 0 {
            let topic = e.1.get(0).unwrap();
            if let Ok(sym) = Symbol::try_from_val(&env, &topic) {
                return sym.to_string() == "grace_period_extended_event";
            }
        }
        false
    });
    assert!(found);
}

#[test]
fn test_finalize_after_extended_grace() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, client, _owner) = setup(&env);
    let employer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_sac = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    let base = 100_u64;
    let aid = cancel_payroll_agreement(&env, &client, &employer, &token, base);

    env.as_contract(&contract_id, || {
        DataKey::set_agreement_escrow_balance(&env, aid, &token, 5000_i128);
    });
    token_sac.mint(&contract_id, &5000_i128);

    client.extend_grace_period(&employer, &aid, &50);

    env.ledger().with_mut(|li| {
        li.timestamp += base + 50 + 1;
    });
    assert!(!client.is_grace_period_active(&aid));

    client.finalize_grace_period(&aid);
}
