#![cfg(test)]

use expense_reimbursement::{
    ExpenseReimbursementContract, ExpenseReimbursementContractClient, ExpenseStatus,
};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, String, Symbol,
};

#[contracttype]
#[derive(Clone)]
enum MockAuditStorageKey {
    NextId,
}

#[contract]
pub struct MockAuditLoggerContract;

#[contractimpl]
impl MockAuditLoggerContract {
    pub fn append_log(
        env: Env,
        actor: Address,
        _action: Symbol,
        _subject: Option<Address>,
        _amount: Option<i128>,
    ) -> u64 {
        actor.require_auth();
        let id: u64 = env
            .storage()
            .persistent()
            .get(&MockAuditStorageKey::NextId)
            .unwrap_or(1u64);
        env.storage()
            .persistent()
            .set(&MockAuditStorageKey::NextId, &(id + 1));
        id
    }
}

fn create_token<'a>(env: &Env, admin: &Address) -> token::Client<'a> {
    let token_address = env.register_stellar_asset_contract(admin.clone());
    token::Client::new(env, &token_address)
}

fn create_contract<'a>(env: &Env) -> ExpenseReimbursementContractClient<'a> {
    let contract_id = env.register_contract(None, ExpenseReimbursementContract);
    ExpenseReimbursementContractClient::new(env, &contract_id)
}

fn create_mock_audit_logger(env: &Env) -> Address {
    env.register_contract(None, MockAuditLoggerContract)
}

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);
}

#[test]
fn test_initialize_requires_owner_auth() {
    let env = Env::default();

    let owner = Address::generate(&env);
    let client = create_contract(&env);

    let result = client.try_initialize(&owner);
    assert!(result.is_err());
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_initialize_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.initialize(&owner);
}

#[test]
fn test_add_and_check_approver() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&approver);

    assert!(client.is_approver(&approver));
}

#[test]
fn test_remove_approver() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let approver = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&approver);
    assert!(client.is_approver(&approver));

    client.remove_approver(&approver);
    assert!(!client.is_approver(&approver));
}

#[test]
fn test_submit_expense() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash_123"),
        &String::from_str(&env, "Office supplies"),
    );

    assert_eq!(expense_id, 0);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.submitter, submitter);
    assert_eq!(expense.approver, approver);
    assert_eq!(expense.amount, 500);
    assert_eq!(expense.escrow_amount, 0);
    assert_eq!(expense.status, ExpenseStatus::Pending);
    assert_eq!(expense.audit_log_id, None);
}

#[test]
#[should_panic(expected = "Receipt payload cannot be empty")]
fn test_submit_expense_empty_receipt_payload_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&approver);

    client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, ""),
        &String::from_str(&env, "Invalid"),
    );
}

#[test]
#[should_panic(expected = "Receipt payload too large")]
fn test_submit_expense_oversized_receipt_payload_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    let oversized = "x".repeat(4097);

    client.initialize(&owner);
    client.add_approver(&approver);

    client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, &oversized),
        &String::from_str(&env, "Too big"),
    );
}

#[test]
#[should_panic(expected = "Receipt already reimbursed")]
fn test_same_receipt_payload_rejected_on_second_submission() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter_a = Address::generate(&env);
    let submitter_b = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&approver);

    let payload = String::from_str(&env, "same-receipt-payload");

    client.submit_expense(
        &submitter_a,
        &approver,
        &token_client.address,
        &300,
        &payload,
        &String::from_str(&env, "Expense A"),
    );

    client.submit_expense(
        &submitter_b,
        &approver,
        &token_client.address,
        &300,
        &payload,
        &String::from_str(&env, "Expense B"),
    );
}

#[test]
fn test_different_receipt_payloads_allowed() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter_a = Address::generate(&env);
    let submitter_b = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&approver);

    let id1 = client.submit_expense(
        &submitter_a,
        &approver,
        &token_client.address,
        &300,
        &String::from_str(&env, "receipt-payload-1"),
        &String::from_str(&env, "Expense 1"),
    );

    let id2 = client.submit_expense(
        &submitter_b,
        &approver,
        &token_client.address,
        &300,
        &String::from_str(&env, "receipt-payload-2"),
        &String::from_str(&env, "Expense 2"),
    );

    assert_eq!(id1, 0);
    assert_eq!(id2, 1);
}

#[test]
#[should_panic(expected = "Amount must be positive")]
fn test_submit_expense_zero_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&approver);

    client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &0,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Invalid"),
    );
}

#[test]
#[should_panic(expected = "Approver cannot be submitter")]
fn test_submit_expense_self_approve_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter_and_approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&submitter_and_approver);

    client.submit_expense(
        &submitter_and_approver,
        &submitter_and_approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Invalid"),
    );
}

#[test]
fn test_fund_expense() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.escrow_amount, 500);
    assert_eq!(expense.payer, Some(payer.clone()));
    
    // contract holds tokens
    assert_eq!(token_client.balance(&client.address), 500);
    assert_eq!(token_client.balance(&payer), 500);
}

#[test]
fn test_approve_expense_full() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);
    client.approve_expense(&approver, &expense_id, &500);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Approved);
    assert_eq!(expense.approved_amount, Some(500));
}

#[test]
fn test_approve_expense_partial() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);
    client.approve_expense(&approver, &expense_id, &300); // partial approval

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Approved);
    assert_eq!(expense.approved_amount, Some(300));
}

#[test]
fn test_approve_expense_requires_designated_approver() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let unauthorized_approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);
    client.add_approver(&unauthorized_approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);

    let result = client.try_approve_expense(&unauthorized_approver, &expense_id, &500);
    assert!(result.is_err());
}

#[test]
fn test_approval_links_to_audit_logger_when_configured() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);
    let audit_logger = create_mock_audit_logger(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);
    client.set_audit_logger(&owner, &audit_logger);
    assert_eq!(client.get_audit_logger(), Some(audit_logger));

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);
    client.approve_expense(&approver, &expense_id, &500);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.audit_log_id, Some(1));
}

#[test]
fn test_set_audit_logger_requires_owner_auth() {
    let env = Env::default();

    let owner = Address::generate(&env);
    let non_owner = Address::generate(&env);
    let audit_logger = create_mock_audit_logger(&env);
    let client = create_contract(&env);

    env.mock_all_auths();
    client.initialize(&owner);

    let result = client.try_set_audit_logger(&non_owner, &audit_logger);
    assert!(result.is_err());
}

#[test]
#[should_panic(expected = "Insufficient escrowed funds")]
fn test_approve_expense_unfunded_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    // Fails because escrow_amount is 0 -> 0 is not >= 500
    client.approve_expense(&approver, &expense_id, &500);
}

#[test]
fn test_reject_expense_refunds() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);
    assert_eq!(token_client.balance(&client.address), 500);
    
    client.reject_expense(&approver, &expense_id);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Rejected);
    assert_eq!(expense.escrow_amount, 0);

    // Funds refunded to payer
    assert_eq!(token_client.balance(&client.address), 0);
    assert_eq!(token_client.balance(&payer), 1_000);
}

#[test]
fn test_pay_expense_full() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);
    client.approve_expense(&approver, &expense_id, &500);
    client.pay_expense(&expense_id);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Paid);
    
    // Funds disbursed fully to submitter
    assert_eq!(token_client.balance(&submitter), 500);
    assert_eq!(token_client.balance(&payer), 500); // the remaining 500 out of initial 1_000
    assert_eq!(token_client.balance(&client.address), 0);
}

#[test]
fn test_pay_expense_cannot_be_paid_twice() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);
    client.approve_expense(&approver, &expense_id, &500);
    client.pay_expense(&expense_id);

    let second = client.try_pay_expense(&expense_id);
    assert!(second.is_err());
}

#[test]
fn test_pay_expense_partial() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);
    client.approve_expense(&approver, &expense_id, &300); // Only approve 300
    client.pay_expense(&expense_id);

    // Verify partial disbursement and refund
    assert_eq!(token_client.balance(&submitter), 300); // Gets approved amount
    assert_eq!(token_client.balance(&payer), 700); // 500 remaining + 200 returned
    assert_eq!(token_client.balance(&client.address), 0);
}

#[test]
fn test_cancel_expense_refunds() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &1_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.fund_expense(&payer, &expense_id, &500);
    client.cancel_expense(&submitter, &expense_id);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Cancelled);
    assert_eq!(expense.escrow_amount, 0);

    // Funds refunded
    assert_eq!(token_client.balance(&client.address), 0);
    assert_eq!(token_client.balance(&payer), 1_000);
}

#[test]
fn test_multiple_expenses_with_unique_receipts_work_end_to_end() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter1 = Address::generate(&env);
    let submitter2 = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &5_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    let expense_id1 = client.submit_expense(
        &submitter1,
        &approver,
        &token_client.address,
        &300,
        &String::from_str(&env, "receipt1"),
        &String::from_str(&env, "Expense 1"),
    );
    let expense_id2 = client.submit_expense(
        &submitter1,
        &approver,
        &token_client.address,
        &300,
        &String::from_str(&env, "receipt1b"),
        &String::from_str(&env, "Expense 1_duplicate"),
    );
    let expense_id3 = client.submit_expense(
        &submitter2,
        &approver,
        &token_client.address,
        &700,
        &String::from_str(&env, "receipt2"),
        &String::from_str(&env, "Expense 2"),
    );

    assert_eq!(expense_id1, 0);
    assert_eq!(expense_id2, 1);
    assert_eq!(expense_id3, 2);

    let expense1 = client.get_expense(&expense_id1).unwrap();
    let expense2 = client.get_expense(&expense_id2).unwrap();
    let expense3 = client.get_expense(&expense_id3).unwrap();

    assert_eq!(expense1.amount, 300);
    assert_eq!(expense2.amount, 300);
    assert_eq!(expense3.amount, 700);
    assert_eq!(expense1.submitter, submitter1);
    assert_eq!(expense2.submitter, submitter1);
    assert_eq!(expense3.submitter, submitter2);

    // Pay expenses
    client.fund_expense(&payer, &expense_id1, &300);
    client.fund_expense(&payer, &expense_id3, &700);

    client.approve_expense(&approver, &expense_id1, &300);
    client.approve_expense(&approver, &expense_id3, &700);

    client.pay_expense(&expense_id1);
    client.pay_expense(&expense_id3);

    assert_eq!(token_client.balance(&submitter1), 300);
    assert_eq!(token_client.balance(&submitter2), 700);
    assert_eq!(token_client.balance(&client.address), 0);
    assert_eq!(token_client.balance(&payer), 4000);
}
