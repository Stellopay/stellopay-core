#![cfg(test)]

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};
use stello_pay_contract::audit::AuditEvent;
use stello_pay_contract::{PayrollContract, PayrollContractClient};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MockAuditLogEntry {
    pub id: u64,
    pub actor: Address,
    pub action: Symbol,
    pub subject: Option<Address>,
    pub amount: Option<i128>,
}

#[contracttype]
#[derive(Clone)]
enum MockAuditStorageKey {
    NextId,
    Log(u64),
}

#[contract]
pub struct MockAuditLoggerContract;

#[contractimpl]
impl MockAuditLoggerContract {
    pub fn append_log(
        env: Env,
        actor: Address,
        action: Symbol,
        subject: Option<Address>,
        amount: Option<i128>,
    ) -> u64 {
        actor.require_auth();
        let id = env
            .storage()
            .persistent()
            .get(&MockAuditStorageKey::NextId)
            .unwrap_or(1u64);
        let entry = MockAuditLogEntry {
            id,
            actor,
            action,
            subject,
            amount,
        };
        env.storage()
            .persistent()
            .set(&MockAuditStorageKey::Log(id), &entry);
        env.storage()
            .persistent()
            .set(&MockAuditStorageKey::NextId, &(id + 1));
        id
    }

    pub fn get_log(env: Env, id: u64) -> Option<MockAuditLogEntry> {
        env.storage()
            .persistent()
            .get(&MockAuditStorageKey::Log(id))
    }

    pub fn get_log_count(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&MockAuditStorageKey::NextId)
            .map(|next_id: u64| next_id - 1)
            .unwrap_or(0)
    }
}

fn setup() -> (
    Env,
    PayrollContractClient<'static>,
    MockAuditLoggerContractClient<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let payroll_id = env.register(PayrollContract, ());
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);
    let owner = Address::generate(&env);
    payroll_client.initialize(&owner);

    let audit_id = env.register(MockAuditLoggerContract, ());
    let audit_client = MockAuditLoggerContractClient::new(&env, &audit_id);
    payroll_client.set_audit_logger(&owner, &audit_id);

    (env, payroll_client, audit_client, owner)
}

#[test]
fn records_agreement_created_audit_entry() {
    let (env, payroll_client, audit_client, _) = setup();
    let employer = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &3600);

    assert_eq!(payroll_client.get_audit_entry_count(), 1);
    let entry = payroll_client.get_audit_entry(&1).unwrap();
    assert_eq!(entry.actor, employer);
    assert_eq!(entry.event, AuditEvent::AgreementCreated);
    assert_eq!(entry.agreement_id, agreement_id);
    assert_eq!(entry.subject, None);
    assert_eq!(entry.amount, Some(0));
    assert_eq!(entry.external_log_id, Some(1));

    let external = audit_client.get_log(&1).unwrap();
    assert_eq!(external.action, Symbol::new(&env, "agreement_created"));
    assert_eq!(audit_client.get_log_count(), 1);
}

#[test]
fn records_agreement_activated_and_cancelled_audit_entries() {
    let (env, payroll_client, audit_client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &3600);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &500);
    payroll_client.activate_agreement(&agreement_id);
    payroll_client.cancel_agreement(&agreement_id);

    assert_eq!(payroll_client.get_audit_entry_count(), 3);
    let activated = payroll_client.get_audit_entry(&2).unwrap();
    assert_eq!(activated.event, AuditEvent::AgreementActivated);
    assert_eq!(activated.amount, Some(500));
    assert_eq!(activated.external_log_id, Some(2));

    let cancelled = payroll_client.get_audit_entry(&3).unwrap();
    assert_eq!(cancelled.event, AuditEvent::AgreementCancelled);
    assert_eq!(cancelled.agreement_id, agreement_id);
    assert_eq!(cancelled.amount, Some(500));
    assert_eq!(cancelled.external_log_id, Some(3));

    assert_eq!(
        audit_client.get_log(&2).unwrap().action,
        Symbol::new(&env, "agreement_activated")
    );
    assert_eq!(
        audit_client.get_log(&3).unwrap().action,
        Symbol::new(&env, "agreement_cancelled")
    );
}

#[test]
fn records_dispute_raised_and_resolved_audit_entries() {
    let (env, payroll_client, audit_client, _) = setup();
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token = Address::generate(&env);

    payroll_client.set_arbiter(&employer, &arbiter);
    let agreement_id = payroll_client.create_payroll_agreement(&employer, &token, &3600);
    payroll_client.add_employee_to_agreement(&agreement_id, &employee, &1000);

    payroll_client.raise_dispute(&employee, &agreement_id);
    payroll_client.resolve_dispute(&arbiter, &agreement_id, &0, &0);

    // `set_arbiter` now records a lifecycle audit entry (ArbiterSet), so the
    // dispute flow produces 4 entries total:
    //   1 = ArbiterSet, 2 = AgreementCreated, 3 = DisputeRaised, 4 = DisputeResolved.
    assert_eq!(payroll_client.get_audit_entry_count(), 4);

    let arbiter_set = payroll_client.get_audit_entry(&1).unwrap();
    assert_eq!(arbiter_set.actor, employer);
    assert_eq!(arbiter_set.event, AuditEvent::ArbiterSet);
    assert_eq!(arbiter_set.agreement_id, 0);
    assert_eq!(arbiter_set.subject, Some(arbiter.clone()));
    assert_eq!(arbiter_set.amount, None);
    assert_eq!(arbiter_set.external_log_id, Some(1));

    let raised = payroll_client.get_audit_entry(&3).unwrap();
    assert_eq!(raised.actor, employee);
    assert_eq!(raised.event, AuditEvent::DisputeRaised);
    assert_eq!(raised.subject, Some(employer.clone()));
    assert_eq!(raised.amount, Some(1000));
    assert_eq!(raised.external_log_id, Some(3));

    let resolved = payroll_client.get_audit_entry(&4).unwrap();
    assert_eq!(resolved.actor, arbiter);
    assert_eq!(resolved.event, AuditEvent::DisputeResolved);
    assert_eq!(resolved.subject, Some(employer));
    assert_eq!(resolved.amount, Some(0));
    assert_eq!(resolved.external_log_id, Some(4));

    assert_eq!(
        audit_client.get_log(&3).unwrap().action,
        Symbol::new(&env, "dispute_raised")
    );
    assert_eq!(
        audit_client.get_log(&4).unwrap().action,
        Symbol::new(&env, "dispute_resolved")
    );
}

#[test]
fn set_audit_logger_requires_owner() {
    let env = Env::default();
    env.mock_all_auths();

    let payroll_id = env.register(PayrollContract, ());
    let payroll_client = PayrollContractClient::new(&env, &payroll_id);
    let owner = Address::generate(&env);
    let non_owner = Address::generate(&env);
    let audit_id = env.register(MockAuditLoggerContract, ());

    payroll_client.initialize(&owner);
    let result = payroll_client.try_set_audit_logger(&non_owner, &audit_id);

    assert!(result.is_err());
    assert_eq!(payroll_client.get_audit_logger(), None);
}
