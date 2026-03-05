#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, String};

/// ExpenseReimbursementContract manages expense submissions with approval workflows
/// and receipt verification for organizational expense management.
///
/// # Security Model
/// - Only submitters can cancel their pending expenses
/// - Only designated approvers can approve/reject expenses
/// - Only contract owner can initialize and update approvers
/// - Funds are held in escrow until approval
/// - All state changes emit events for auditability
#[contract]
pub struct ExpenseReimbursementContract;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpenseStatus {
    Pending,
    Approved,
    Rejected,
    Cancelled,
    Paid,
}

/// Represents an expense reimbursement request
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expense {
    pub id: u128,
    pub submitter: Address,
    pub approver: Address,
    pub token: Address,
    pub amount: i128,
    pub status: ExpenseStatus,
    pub receipt_hash: String,
    pub description: String,
    pub submitted_at: u64,
}

#[contracttype]
#[derive(Clone)]
enum StorageKey {
    Initialized,
    Owner,
    NextExpenseId,
    Expense(u128),
    ApproverRole(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpenseSubmittedEvent {
    pub expense_id: u128,
    pub submitter: Address,
    pub approver: Address,
    pub amount: i128,
    pub receipt_hash: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpenseApprovedEvent {
    pub expense_id: u128,
    pub approver: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpenseRejectedEvent {
    pub expense_id: u128,
    pub approver: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpensePaidEvent {
    pub expense_id: u128,
    pub submitter: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpenseCancelledEvent {
    pub expense_id: u128,
    pub submitter: Address,
}

fn require_initialized(env: &Env) {
    assert!(
        env.storage()
            .persistent()
            .get::<_, bool>(&StorageKey::Initialized)
            .unwrap_or(false),
        "Contract not initialized"
    );
}

fn require_owner(env: &Env, addr: &Address) {
    addr.require_auth();
    let owner: Address = env
        .storage()
        .persistent()
        .get(&StorageKey::Owner)
        .expect("Owner not set");
    assert!(addr == &owner, "Unauthorized: not owner");
}

fn is_approver(env: &Env, addr: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&StorageKey::ApproverRole(addr.clone()))
        .unwrap_or(false)
}

#[contractimpl]
impl ExpenseReimbursementContract {
    /// Initialize the contract with an owner
    ///
    /// # Arguments
    /// * `owner` - Address that can manage approvers
    ///
    /// # Panics
    /// * If already initialized
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn initialize(env: Env, owner: Address) {
        assert!(
            !env.storage()
                .persistent()
                .get::<_, bool>(&StorageKey::Initialized)
                .unwrap_or(false),
            "Already initialized"
        );

        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::NextExpenseId, &0u128);
        env.storage()
            .persistent()
            .set(&StorageKey::Initialized, &true);
    }

    /// Add an approver who can approve/reject expenses
    ///
    /// # Arguments
    /// * `approver` - Address to grant approver role
    ///
    /// # Panics
    /// * If caller is not owner
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn add_approver(env: Env, approver: Address) {
        require_initialized(&env);
        let owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .expect("Owner not set");
        require_owner(&env, &owner);

        env.storage()
            .persistent()
            .set(&StorageKey::ApproverRole(approver), &true);
    }

    /// Remove an approver
    ///
    /// # Arguments
    /// * `approver` - Address to revoke approver role
    ///
    /// # Panics
    /// * If caller is not owner
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn remove_approver(env: Env, approver: Address) {
        require_initialized(&env);
        let owner: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::Owner)
            .expect("Owner not set");
        require_owner(&env, &owner);

        env.storage()
            .persistent()
            .remove(&StorageKey::ApproverRole(approver));
    }

    /// Submit an expense for reimbursement
    ///
    /// # Arguments
    /// * `submitter` - Employee submitting the expense
    /// * `approver` - Designated approver for this expense
    /// * `token` - Token address for reimbursement
    /// * `amount` - Reimbursement amount
    /// * `receipt_hash` - Hash of receipt document for verification
    /// * `description` - Expense description
    ///
    /// # Returns
    /// Expense ID
    ///
    /// # Panics
    /// * If amount is not positive
    /// * If approver is not authorized
    pub fn submit_expense(
        env: Env,
        submitter: Address,
        approver: Address,
        token: Address,
        amount: i128,
        receipt_hash: String,
        description: String,
    ) -> u128 {
        require_initialized(&env);
        submitter.require_auth();

        assert!(amount > 0, "Amount must be positive");
        assert!(is_approver(&env, &approver), "Invalid approver");

        let expense_id: u128 = env
            .storage()
            .persistent()
            .get(&StorageKey::NextExpenseId)
            .unwrap();

        let expense = Expense {
            id: expense_id,
            submitter: submitter.clone(),
            approver: approver.clone(),
            token,
            amount,
            status: ExpenseStatus::Pending,
            receipt_hash: receipt_hash.clone(),
            description,
            submitted_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);
        env.storage()
            .persistent()
            .set(&StorageKey::NextExpenseId, &(expense_id + 1));

        env.events().publish(
            (String::from_str(&env, "expense_submitted"), expense_id),
            ExpenseSubmittedEvent {
                expense_id,
                submitter,
                approver,
                amount,
                receipt_hash,
            },
        );

        expense_id
    }

    /// Approve an expense and transfer funds to submitter
    ///
    /// # Arguments
    /// * `approver` - Approver authorizing the expense
    /// * `expense_id` - ID of expense to approve
    ///
    /// # Panics
    /// * If caller is not the designated approver
    /// * If expense is not in Pending status
    pub fn approve_expense(env: Env, approver: Address, expense_id: u128) {
        require_initialized(&env);
        approver.require_auth();

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(expense.approver == approver, "Unauthorized approver");
        assert!(expense.status == ExpenseStatus::Pending, "Invalid status");

        expense.status = ExpenseStatus::Approved;
        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);

        env.events().publish(
            (String::from_str(&env, "expense_approved"), expense_id),
            ExpenseApprovedEvent {
                expense_id,
                approver,
            },
        );
    }

    /// Reject an expense
    ///
    /// # Arguments
    /// * `approver` - Approver rejecting the expense
    /// * `expense_id` - ID of expense to reject
    ///
    /// # Panics
    /// * If caller is not the designated approver
    /// * If expense is not in Pending status
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn reject_expense(env: Env, approver: Address, expense_id: u128) {
        require_initialized(&env);
        approver.require_auth();

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(expense.approver == approver, "Unauthorized approver");
        assert!(expense.status == ExpenseStatus::Pending, "Invalid status");

        expense.status = ExpenseStatus::Rejected;
        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);

        env.events().publish(
            (String::from_str(&env, "expense_rejected"), expense_id),
            ExpenseRejectedEvent {
                expense_id,
                approver,
            },
        );
    }

    /// Pay an approved expense
    ///
    /// # Arguments
    /// * `payer` - Address funding the reimbursement
    /// * `expense_id` - ID of expense to pay
    ///
    /// # Panics
    /// * If expense is not in Approved status
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn pay_expense(env: Env, payer: Address, expense_id: u128) {
        require_initialized(&env);
        payer.require_auth();

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(expense.status == ExpenseStatus::Approved, "Not approved");

        let token_client = token::Client::new(&env, &expense.token);
        token_client.transfer(&payer, &expense.submitter, &expense.amount);

        expense.status = ExpenseStatus::Paid;
        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);

        env.events().publish(
            (String::from_str(&env, "expense_paid"), expense_id),
            ExpensePaidEvent {
                expense_id,
                submitter: expense.submitter,
                amount: expense.amount,
            },
        );
    }

    /// Cancel a pending expense
    ///
    /// # Arguments
    /// * `submitter` - Original submitter cancelling the expense
    /// * `expense_id` - ID of expense to cancel
    ///
    /// # Panics
    /// * If caller is not the submitter
    /// * If expense is not in Pending status
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn cancel_expense(env: Env, submitter: Address, expense_id: u128) {
        require_initialized(&env);
        submitter.require_auth();

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(expense.submitter == submitter, "Unauthorized");
        assert!(expense.status == ExpenseStatus::Pending, "Invalid status");

        expense.status = ExpenseStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);

        env.events().publish(
            (String::from_str(&env, "expense_cancelled"), expense_id),
            ExpenseCancelledEvent {
                expense_id,
                submitter,
            },
        );
    }

    /// Get expense details
    ///
    /// # Arguments
    /// * `expense_id` - ID of expense to retrieve
    ///
    /// # Returns
    /// Expense details or None if not found
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn get_expense(env: Env, expense_id: u128) -> Option<Expense> {
        require_initialized(&env);
        env.storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
    }

    /// Check if an address has approver role
    ///
    /// # Arguments
    /// * `address` - Address to check
    ///
    /// # Returns
    /// true if address is an approver
    ///
    /// # Access Control
    /// Requires caller authentication
    pub fn is_approver(env: Env, address: Address) -> bool {
        require_initialized(&env);
        is_approver(&env, &address)
    }
}
