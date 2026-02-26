use expense_reimbursement::{
    ExpenseReimbursementContract, ExpenseReimbursementContractClient, ExpenseStatus,
};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env, String};

fn create_token<'a>(env: &Env, admin: &Address) -> token::Client<'a> {
    let token_address = env.register_stellar_asset_contract(admin.clone());
    token::Client::new(env, &token_address)
}

fn create_contract<'a>(env: &Env) -> ExpenseReimbursementContractClient<'a> {
    let contract_id = env.register_contract(None, ExpenseReimbursementContract);
    ExpenseReimbursementContractClient::new(env, &contract_id)
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
    assert_eq!(expense.status, ExpenseStatus::Pending);
    assert_eq!(
        expense.receipt_hash,
        String::from_str(&env, "receipt_hash_123")
    );
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
#[should_panic(expected = "Invalid approver")]
fn test_submit_expense_invalid_approver_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let invalid_approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    client.initialize(&owner);

    client.submit_expense(
        &submitter,
        &invalid_approver,
        &token_client.address,
        &100,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Test"),
    );
}

#[test]
fn test_approve_expense() {
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

    client.approve_expense(&approver, &expense_id);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Approved);
}

#[test]
#[should_panic(expected = "Unauthorized approver")]
fn test_approve_expense_wrong_approver_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let wrong_approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    client.initialize(&owner);
    client.add_approver(&approver);
    client.add_approver(&wrong_approver);

    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &500,
        &String::from_str(&env, "receipt_hash"),
        &String::from_str(&env, "Travel"),
    );

    client.approve_expense(&wrong_approver, &expense_id);
}

#[test]
fn test_reject_expense() {
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

    client.reject_expense(&approver, &expense_id);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Rejected);
}

#[test]
#[should_panic(expected = "Invalid status")]
fn test_approve_rejected_expense_fails() {
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

    client.reject_expense(&approver, &expense_id);
    client.approve_expense(&approver, &expense_id);
}

#[test]
fn test_pay_expense() {
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

    client.approve_expense(&approver, &expense_id);
    client.pay_expense(&payer, &expense_id);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Paid);
    assert_eq!(token_client.balance(&submitter), 500);
    assert_eq!(token_client.balance(&payer), 500);
}

#[test]
#[should_panic(expected = "Not approved")]
fn test_pay_pending_expense_fails() {
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

    client.pay_expense(&payer, &expense_id);
}

#[test]
fn test_cancel_expense() {
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

    client.cancel_expense(&submitter, &expense_id);

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Cancelled);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_cancel_expense_wrong_submitter_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let other_user = Address::generate(&env);
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

    client.cancel_expense(&other_user, &expense_id);
}

#[test]
#[should_panic(expected = "Invalid status")]
fn test_cancel_approved_expense_fails() {
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

    client.approve_expense(&approver, &expense_id);
    client.cancel_expense(&submitter, &expense_id);
}

#[test]
fn test_multiple_expenses() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter1 = Address::generate(&env);
    let submitter2 = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

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
        &submitter2,
        &approver,
        &token_client.address,
        &700,
        &String::from_str(&env, "receipt2"),
        &String::from_str(&env, "Expense 2"),
    );

    assert_eq!(expense_id1, 0);
    assert_eq!(expense_id2, 1);

    let expense1 = client.get_expense(&expense_id1).unwrap();
    let expense2 = client.get_expense(&expense_id2).unwrap();

    assert_eq!(expense1.amount, 300);
    assert_eq!(expense2.amount, 700);
    assert_eq!(expense1.submitter, submitter1);
    assert_eq!(expense2.submitter, submitter2);
}

#[test]
fn test_get_nonexistent_expense() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let client = create_contract(&env);

    client.initialize(&owner);

    let result = client.get_expense(&999);
    assert!(result.is_none());
}

#[test]
fn test_full_expense_workflow() {
    let env = Env::default();
    env.mock_all_auths();

    let owner = Address::generate(&env);
    let submitter = Address::generate(&env);
    let approver = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_client = create_token(&env, &token_admin);
    let client = create_contract(&env);

    token::StellarAssetClient::new(&env, &token_client.address).mint(&payer, &10_000);

    client.initialize(&owner);
    client.add_approver(&approver);

    // Submit expense
    let expense_id = client.submit_expense(
        &submitter,
        &approver,
        &token_client.address,
        &2_500,
        &String::from_str(&env, "sha256_receipt_hash"),
        &String::from_str(&env, "Conference travel and accommodation"),
    );

    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Pending);

    // Approve expense
    client.approve_expense(&approver, &expense_id);
    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Approved);

    // Pay expense
    client.pay_expense(&payer, &expense_id);
    let expense = client.get_expense(&expense_id).unwrap();
    assert_eq!(expense.status, ExpenseStatus::Paid);

    // Verify balances
    assert_eq!(token_client.balance(&submitter), 2_500);
    assert_eq!(token_client.balance(&payer), 7_500);
}
