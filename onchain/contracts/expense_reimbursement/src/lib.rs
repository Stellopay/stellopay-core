#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, xdr::ToXdr, Address, Bytes, BytesN, Env,
    IntoVal, String, Symbol, Val, Vec,
};

/// ExpenseReimbursementContract manages expense submissions with approval workflows
/// and receipt verification with escrow capabilities for organizational expense management.
///
/// # Security Model
/// - Only submitters can cancel their pending expenses.
/// - Only designated approvers can approve/reject expenses.
/// - Only contract owner can initialize and update approvers.
/// - Funds are held in escrow within the contract until approval or rejection.
/// - Employer funds are protected and refunded reliably on rejection or cancellation.
/// - Approvers cannot self-approve their own submitted expenses to prevent collusion.
/// - All state changes emit events for auditability.
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
    pub escrow_amount: i128,
    pub approved_amount: Option<i128>,
    pub payer: Option<Address>,
    pub status: ExpenseStatus,
    /// NatSpec: `receipt_hash` is a deterministic commitment (e.g., SHA-256) of the
    /// receipt document, allowing off-chain auditing of original receipts corresponding to on-chain payouts.
    pub receipt_hash: BytesN<32>,
    pub audit_log_id: Option<u64>,
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
    ReceiptHash(BytesN<32>),
    AuditLogger,
    ApproverRole(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpenseSubmittedEvent {
    pub expense_id: u128,
    pub submitter: Address,
    pub approver: Address,
    pub amount: i128,
    pub receipt_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpenseFundedEvent {
    pub expense_id: u128,
    pub payer: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpenseApprovedEvent {
    pub expense_id: u128,
    pub approver: Address,
    pub approved_amount: i128,
    pub receipt_hash: BytesN<32>,
    pub audit_log_id: Option<u64>,
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

const RECEIPT_HASH_DOMAIN: &[u8] = b"stello.expense.receipt.v1";
const MAX_RECEIPT_PAYLOAD_BYTES: u32 = 4096;

fn compute_receipt_hash(env: &Env, receipt_payload: &String) -> BytesN<32> {
    let payload_len = receipt_payload.len();
    assert!(payload_len > 0, "Receipt payload cannot be empty");
    assert!(
        payload_len <= MAX_RECEIPT_PAYLOAD_BYTES,
        "Receipt payload too large"
    );

    let mut preimage = Bytes::new(env);
    preimage.append(&Bytes::from_slice(env, RECEIPT_HASH_DOMAIN));
    preimage.push_back(0u8);
    preimage.append(&receipt_payload.clone().to_xdr(env));
    env.crypto().sha256(&preimage).into()
}

fn append_approval_audit_log(
    env: &Env,
    approver: &Address,
    subject: &Address,
    approved_amount: i128,
) -> Option<u64> {
    let maybe_audit_logger: Option<Address> = env.storage().persistent().get(&StorageKey::AuditLogger);
    maybe_audit_logger.map(|audit_logger| {
        let mut args = Vec::<Val>::new(env);
        args.push_back(approver.clone().into_val(env));
        args.push_back(Symbol::new(env, "expense_approved").into_val(env));
        args.push_back(Some(subject.clone()).into_val(env));
        args.push_back(Some(approved_amount).into_val(env));

        env.invoke_contract::<u64>(
            &audit_logger,
            &Symbol::new(env, "append_log"),
            args,
        )
    })
}

#[contractimpl]
impl ExpenseReimbursementContract {
    /// Initialize the contract with an owner
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
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
    pub fn submit_expense(
        env: Env,
        submitter: Address,
        approver: Address,
        token: Address,
        amount: i128,
        receipt_payload: String,
        description: String,
    ) -> u128 {
        require_initialized(&env);
        submitter.require_auth();

        assert!(amount > 0, "Amount must be positive");
        assert!(is_approver(&env, &approver), "Invalid approver");
        assert!(submitter != approver, "Approver cannot be submitter");

        let receipt_hash = compute_receipt_hash(&env, &receipt_payload);
        assert!(
            !env.storage()
                .persistent()
                .has(&StorageKey::ReceiptHash(receipt_hash.clone())),
            "Receipt already reimbursed"
        );

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
            escrow_amount: 0,
            approved_amount: None,
            payer: None,
            status: ExpenseStatus::Pending,
            receipt_hash: receipt_hash.clone(),
            audit_log_id: None,
            description,
            submitted_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);
        env.storage()
            .persistent()
            .set(&StorageKey::ReceiptHash(receipt_hash.clone()), &expense_id);
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

    /// Extends a pending claim by escrowing funds
    pub fn fund_expense(env: Env, payer: Address, expense_id: u128, amount: i128) {
        require_initialized(&env);
        payer.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(expense.status == ExpenseStatus::Pending, "Expense not pending");

        let token_client = token::Client::new(&env, &expense.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);

        expense.escrow_amount += amount;
        
        // Register the payer if none exists; else require same payer for refunds to be coherent
        if expense.payer.is_none() {
            expense.payer = Some(payer.clone());
        } else {
            assert!(expense.payer.unwrap() == payer, "Only initial payer can add funds");
            expense.payer = Some(payer.clone());
        }

        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);

        env.events().publish(
            (String::from_str(&env, "expense_funded"), expense_id),
            ExpenseFundedEvent {
                expense_id,
                payer,
                amount,
            },
        );
    }

    /// Approve an expense, with support for partial approval.
    pub fn approve_expense(env: Env, approver: Address, expense_id: u128, approved_amount: i128) {
        require_initialized(&env);
        approver.require_auth();

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(expense.approver == approver, "Unauthorized approver");
        assert!(expense.status == ExpenseStatus::Pending, "Invalid status");
        assert!(approved_amount > 0, "Approved amount must be positive");
        assert!(approved_amount <= expense.amount, "Cannot approve more than requested");
        assert!(expense.escrow_amount >= approved_amount, "Insufficient escrowed funds");

        expense.approved_amount = Some(approved_amount);
        expense.status = ExpenseStatus::Approved;
        let audit_log_id = append_approval_audit_log(
            &env,
            &approver,
            &expense.submitter,
            approved_amount,
        );
        expense.audit_log_id = audit_log_id;
        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);

        env.events().publish(
            (String::from_str(&env, "expense_approved"), expense_id),
            ExpenseApprovedEvent {
                expense_id,
                approver,
                approved_amount,
                receipt_hash: expense.receipt_hash,
                audit_log_id,
            },
        );
    }

    /// Reject an expense, refunding escrowed funds to the employer safely
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

        // Refund any escrowed funds securely
        if expense.escrow_amount > 0 {
            if let Some(payer) = expense.payer.clone() {
                let token_client = token::Client::new(&env, &expense.token);
                token_client.transfer(&env.current_contract_address(), &payer, &expense.escrow_amount);
            }
        }

        // Must sync escrow reduction if we refund
        expense.escrow_amount = 0;

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

    /// Pay an approved expense to the employee. Any surplus escrow goes back to the payer.
    pub fn pay_expense(env: Env, expense_id: u128) {
        require_initialized(&env);
        
        // Anyone can execute the token payout if it's approved

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(expense.status == ExpenseStatus::Approved, "Not approved");
        let amount_to_pay = expense.approved_amount.unwrap();
        let escrow_before = expense.escrow_amount;

        // Checks-effects-interactions:
        // commit terminal state before token transfers to prevent reentrant
        // double-pay attempts from observing Approved state.
        expense.escrow_amount = 0; // all dispersed by this execution path
        expense.status = ExpenseStatus::Paid;
        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);

        let token_client = token::Client::new(&env, &expense.token);
        
        // Payout to employee
        token_client.transfer(&env.current_contract_address(), &expense.submitter, &amount_to_pay);

        // Refund any unapproved surplus
        let surplus = escrow_before - amount_to_pay;
        if surplus > 0 {
            if let Some(payer) = expense.payer.clone() {
                token_client.transfer(&env.current_contract_address(), &payer, &surplus);
            }
        }

        env.events().publish(
            (String::from_str(&env, "expense_paid"), expense_id),
            ExpensePaidEvent {
                expense_id,
                submitter: expense.submitter.clone(),
                amount: amount_to_pay,
            },
        );
    }

    /// Cancel a pending expense, triggering refund
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

        // Refund any escrowed funds
        if expense.escrow_amount > 0 {
            if let Some(payer) = expense.payer.clone() {
                let token_client = token::Client::new(&env, &expense.token);
                token_client.transfer(&env.current_contract_address(), &payer, &expense.escrow_amount);
            }
        }
        expense.escrow_amount = 0;

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
    pub fn get_expense(env: Env, expense_id: u128) -> Option<Expense> {
        env.storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
    }

    /// Check if an address has approver role
    pub fn is_approver(env: Env, address: Address) -> bool {
        require_initialized(&env);
        is_approver(&env, &address)
    }

    /// Configure the optional external audit logger contract for approval traceability.
    pub fn set_audit_logger(env: Env, owner: Address, audit_logger: Address) {
        require_initialized(&env);
        require_owner(&env, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::AuditLogger, &audit_logger);
    }

    /// Return currently configured audit logger contract address.
    pub fn get_audit_logger(env: Env) -> Option<Address> {
        require_initialized(&env);
        env.storage().persistent().get(&StorageKey::AuditLogger)
    }
}
