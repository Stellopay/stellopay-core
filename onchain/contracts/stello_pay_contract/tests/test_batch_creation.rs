#![cfg(test)]
#![allow(deprecated)]

use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal, Vec,
};
use stello_pay_contract::storage::{EscrowCreateParams, PayrollCreateParams, PayrollError};
use stello_pay_contract::{PayrollContract, PayrollContractClient};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn addr(env: &Env) -> Address {
    Address::generate(env)
}

fn setup(env: &Env) -> (Address, PayrollContractClient<'static>) {
    #[allow(deprecated)]
    let id = env.register_contract(None, PayrollContract);
    let client = PayrollContractClient::new(env, &id);
    let owner = addr(env);
    client.initialize(&owner);
    (id, client)
}

#[test]
fn batch_create_payroll_success() {
    let env = create_test_env();
    let (_id, client) = setup(&env);

    let employer = addr(&env);
    let token1 = addr(&env);
    let token2 = addr(&env);
    let token3 = addr(&env);

    let mut items = soroban_sdk::Vec::<PayrollCreateParams>::new(&env);
    items.push_back(PayrollCreateParams {
        token: token1,
        grace_period_seconds: 3600,
    });
    items.push_back(PayrollCreateParams {
        token: token2,
        grace_period_seconds: 7200,
    });
    items.push_back(PayrollCreateParams {
        token: token3,
        grace_period_seconds: 0,
    });

    let res = client.batch_create_payroll_agreements(&employer, &items);
    assert_eq!(res.total_created, 3);
    assert_eq!(res.total_failed, 0);
    assert_eq!(res.results.len(), 3);
    assert_eq!(res.agreement_ids.len(), 3);

    // Each created agreement should emit agreement_created_event
    let created_events = env
        .events()
        .all()
        .iter()
        .filter(|e| {
            if e.1.len() > 0 {
                let topic = e.1.get(0).unwrap();
                if let Ok(sym) = Symbol::try_from_val(&env, &topic) {
                    return sym.to_string() == "agreement_created_event";
                }
            }
            false
        })
        .count();
    assert!(created_events >= 3);
}

#[test]
fn batch_create_payroll_empty_err() {
    let env = create_test_env();
    let (_id, client) = setup(&env);

    let employer = addr(&env);
    let items = soroban_sdk::Vec::<PayrollCreateParams>::new(&env);
    let result = client.try_batch_create_payroll_agreements(&employer, &items);
    assert_eq!(result, Err(Ok(PayrollError::InvalidData)));
}

#[test]
fn batch_create_escrow_partial_success() {
    let env = create_test_env();
    let (_id, client) = setup(&env);

    let employer = addr(&env);
    let contributor_ok = addr(&env);
    let contributor_bad = addr(&env);
    let token = addr(&env);

    let mut items = soroban_sdk::Vec::<EscrowCreateParams>::new(&env);
    // Valid
    items.push_back(EscrowCreateParams {
        contributor: contributor_ok,
        token: token.clone(),
        amount_per_period: 1000,
        period_seconds: 3600,
        num_periods: 4,
    });
    // Invalid: zero period
    items.push_back(EscrowCreateParams {
        contributor: contributor_bad,
        token,
        amount_per_period: 1000,
        period_seconds: 0,
        num_periods: 4,
    });

    let res = client.batch_create_escrow_agreements(&employer, &items);
    assert_eq!(res.total_created, 1);
    assert_eq!(res.total_failed, 1);
    assert_eq!(res.results.len(), 2);

    let failure = res
        .results
        .iter()
        .find(|r| !r.success)
        .expect("one failure expected");
    assert_eq!(failure.error_code, PayrollError::ZeroPeriodDuration as u32);

    // Verify events: one agreement_created + one employee_added for success
    let created_events = env
        .events()
        .all()
        .iter()
        .filter(|e| {
            if e.1.len() > 0 {
                let topic = e.1.get(0).unwrap();
                if let Ok(sym) = Symbol::try_from_val(&env, &topic) {
                    return sym.to_string() == "agreement_created_event";
                }
            }
            false
        })
        .count();
    assert!(created_events >= 1);

    let employee_added_events = env
        .events()
        .all()
        .iter()
        .filter(|e| {
            if e.1.len() > 0 {
                let topic = e.1.get(0).unwrap();
                if let Ok(sym) = Symbol::try_from_val(&env, &topic) {
                    return sym.to_string() == "employee_added_event";
                }
            }
            false
        })
        .count();
    assert!(employee_added_events >= 1);
}
