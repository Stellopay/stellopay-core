#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, xdr::ToXdr, Address, Bytes, BytesN,
    Env, IntoVal, String, Symbol, Val, Vec,
};

/// Default period duration in seconds (30 days).
const DEFAULT_PERIOD_DURATION: u64 = 2_592_000;

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

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// Escrow balance overflowed during funding. This prevents silent integer wrapping.
    EscrowOverflow = 1,
}

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
    /// receipt document, allowing off-chain auditing of original receipts corresponding to
    /// on-chain payouts.
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
    /// Per-employee per-period spending cap. Maps employee address to
    /// maximum total reimbursable amount in a single period.
    /// A value of 0 (or absent) means no cap is enforced.
    EmployeeCap(Address),
    /// Duration of a spending cap period in seconds.
    PeriodDuration,
    /// Cumulative amount spent per employee within a given period.
    /// Key is (employee_address, period_identifier).
    PeriodSpent(Address, u64),
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
    let maybe_audit_logger: Option<Address> =
        env.storage().persistent().get(&StorageKey::AuditLogger);
    maybe_audit_logger.map(|audit_logger| {
        let mut args = Vec::<Val>::new(env);
        args.push_back(approver.clone().into_val(env));
        args.push_back(Symbol::new(env, "expense_approved").into_val(env));
        args.push_back(Some(subject.clone()).into_val(env));
        args.push_back(Some(approved_amount).into_val(env));

        env.invoke_contract::<u64>(&audit_logger, &Symbol::new(env, "append_log"), args)
    })
}

// ─── Spending Cap Helpers ──────────────────────────────────────────────────

/// Returns the current period identifier based on ledger timestamp.
fn current_period(env: &Env) -> u64 {
    let duration = env
        .storage()
        .persistent()
        .get::<_, u64>(&StorageKey::PeriodDuration)
        .unwrap_or(DEFAULT_PERIOD_DURATION);
    env.ledger().timestamp() / duration
}

/// Reads the per-employee spending cap (0 means no cap).
fn read_employee_cap(env: &Env, employee: &Address) -> i128 {
    env.storage()
        .persistent()
        .get::<_, i128>(&StorageKey::EmployeeCap(employee.clone()))
        .unwrap_or(0)
}

/// Reads the cumulative amount an employee has spent in a given period.
fn read_period_spent(env: &Env, employee: &Address, period: u64) -> i128 {
    env.storage()
        .persistent()
        .get::<_, i128>(&StorageKey::PeriodSpent(employee.clone(), period))
        .unwrap_or(0)
}

/// Adds an amount to the employee's cumulative period spending.
fn add_period_spent(env: &Env, employee: &Address, period: u64, amount: i128) {
    let current = read_period_spent(env, employee, period);
    env.storage().persistent().set(
        &StorageKey::PeriodSpent(employee.clone(), period),
        &(current + amount),
    );
}

/// Subtracts an amount from the employee's cumulative period spending.
fn sub_period_spent(env: &Env, employee: &Address, period: u64, amount: i128) {
    let current = read_period_spent(env, employee, period);
    env.storage().persistent().set(
        &StorageKey::PeriodSpent(employee.clone(), period),
        &(current - amount),
    );
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

    /// Add an approver who can approve/reject expenses.
    ///
    /// # Authorization
    /// Authorizes the live `caller`: it requires `caller`'s signature via
    /// `require_auth` and asserts that `caller` is the contract owner. Any
    /// non-owner caller is rejected, so only the owner can mutate the approver set.
    pub fn add_approver(env: Env, caller: Address, approver: Address) {
        require_initialized(&env);
        require_owner(&env, &caller);

        env.storage()
            .persistent()
            .set(&StorageKey::ApproverRole(approver), &true);
    }

    /// Remove an approver from the active approver set.
    ///
    /// NatSpec: Removal prevents this address from approving or rejecting any
    /// pending expense going forward. It does not alter approval decisions
    /// already recorded on expenses; those decisions remain valid and payable.
    pub fn remove_approver(env: Env, caller: Address, approver: Address) {
        require_initialized(&env);
        require_owner(&env, &caller);

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

        // Enforce per-period spending cap (if configured).
        let cap = read_employee_cap(&env, &submitter);
        if cap > 0 {
            let period = current_period(&env);
            let spent = read_period_spent(&env, &submitter, period);
            assert!(
                spent.checked_add(amount).expect("Spending overflow") <= cap,
                "Expense would exceed per-period spending cap"
            );
        }

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

        // Track the full submitted amount against the period cap.
        // If the expense is later rejected, cancelled, or partially approved,
        // the over-counted amount is decremented from period spent.
        if cap > 0 {
            let period = current_period(&env);
            add_period_spent(&env, &submitter, period, amount);
        }

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
    pub fn fund_expense(
        env: Env,
        payer: Address,
        expense_id: u128,
        amount: i128,
    ) -> Result<(), Error> {
        require_initialized(&env);
        payer.require_auth();
        assert!(amount > 0, "Amount must be positive");

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(
            expense.status == ExpenseStatus::Pending,
            "Expense not pending"
        );

        expense.escrow_amount = expense
            .escrow_amount
            .checked_add(amount)
            .ok_or(Error::EscrowOverflow)?;

        // Register the payer if none exists; else require same payer for refunds to be coherent
        if expense.payer.is_none() {
            expense.payer = Some(payer.clone());
        } else {
            assert!(
                expense.payer.unwrap() == payer,
                "Only initial payer can add funds"
            );
            expense.payer = Some(payer.clone());
        }

        env.storage()
            .persistent()
            .set(&StorageKey::Expense(expense_id), &expense);

        let token_client = token::Client::new(&env, &expense.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);

        env.events().publish(
            (String::from_str(&env, "expense_funded"), expense_id),
            ExpenseFundedEvent {
                expense_id,
                payer,
                amount,
            },
        );

        Ok(())
    }

    /// Approve an expense, with support for partial approval.
    ///
    /// NatSpec: The approver must both be the expense's designated approver and
    /// currently hold the approver role. Once recorded, this approval is part of
    /// the expense's immutable lifecycle state and is not invalidated if the
    /// owner later removes the approver role.
    pub fn approve_expense(env: Env, approver: Address, expense_id: u128, approved_amount: i128) {
        require_initialized(&env);
        approver.require_auth();

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(is_approver(&env, &approver), "Unauthorized approver");
        assert!(expense.approver == approver, "Unauthorized approver");
        assert!(expense.status == ExpenseStatus::Pending, "Invalid status");
        assert!(approved_amount > 0, "Approved amount must be positive");
        assert!(
            approved_amount <= expense.amount,
            "Cannot approve more than requested"
        );
        assert!(
            expense.escrow_amount >= approved_amount,
            "Insufficient escrowed funds"
        );

        expense.approved_amount = Some(approved_amount);
        expense.status = ExpenseStatus::Approved;
        let audit_log_id =
            append_approval_audit_log(&env, &approver, &expense.submitter, approved_amount);
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

    /// Reject an expense, refunding escrowed funds to the employer safely.
    ///
    /// NatSpec: The designated approver must still hold the active approver
    /// role when rejecting, matching the authorization rule for approval.
    pub fn reject_expense(env: Env, approver: Address, expense_id: u128) {
        require_initialized(&env);
        approver.require_auth();

        let mut expense: Expense = env
            .storage()
            .persistent()
            .get(&StorageKey::Expense(expense_id))
            .expect("Expense not found");

        assert!(is_approver(&env, &approver), "Unauthorized approver");
        assert!(expense.approver == approver, "Unauthorized approver");
        assert!(expense.status == ExpenseStatus::Pending, "Invalid status");

        expense.status = ExpenseStatus::Rejected;

        // Refund any escrowed funds securely
        if expense.escrow_amount > 0 {
            if let Some(payer) = expense.payer.clone() {
                let token_client = token::Client::new(&env, &expense.token);
                token_client.transfer(
                    &env.current_contract_address(),
                    &payer,
                    &expense.escrow_amount,
                );
            }
        }

        // Must sync escrow reduction if we refund
        expense.escrow_amount = 0;

        // Decrement period spending cap since this expense is no longer active.
        let cap = read_employee_cap(&env, &expense.submitter);
        if cap > 0 {
            let period = current_period(&env);
            sub_period_spent(&env, &expense.submitter, period, expense.amount);
        }

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
    ///
    /// # Double-Payment Guard
    ///
    /// This function implements a checks-effects-interactions pattern to prevent
    /// double-payment of the same expense:
    /// 1. **Checks**: Verifies the expense is in `Approved` status.
    /// 2. **Effects**: Atomically transitions the expense to `Paid` and zeroes `escrow_amount`
    ///    **before** any token transfer occurs.
    /// 3. **Interactions**: Only after the terminal state is committed does the function perform
    ///    token transfers (payout to submitter, surplus refund to payer).
    ///
    /// Once `Paid`, any subsequent call will fail the status check at step 1,
    /// guaranteeing the expense cannot be paid more than once.
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
        token_client.transfer(
            &env.current_contract_address(),
            &expense.submitter,
            &amount_to_pay,
        );

        // Refund any unapproved surplus
        let surplus = escrow_before - amount_to_pay;
        if surplus > 0 {
            if let Some(payer) = expense.payer.clone() {
                token_client.transfer(&env.current_contract_address(), &payer, &surplus);
            }
        }

        // Adjust period spending cap for partial approval: the difference
        // between the original submitted amount and the paid amount is released.
        let cap = read_employee_cap(&env, &expense.submitter);
        if cap > 0 {
            let over_counted = expense.amount - amount_to_pay;
            if over_counted > 0 {
                let period = current_period(&env);
                sub_period_spent(&env, &expense.submitter, period, over_counted);
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
                token_client.transfer(
                    &env.current_contract_address(),
                    &payer,
                    &expense.escrow_amount,
                );
            }
        }
        expense.escrow_amount = 0;

        // Decrement period spending cap since this expense is no longer active.
        let cap = read_employee_cap(&env, &expense.submitter);
        if cap > 0 {
            let period = current_period(&env);
            sub_period_spent(&env, &expense.submitter, period, expense.amount);
        }

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

    // ── Spending Cap Management ───────────────────────────────────────────────

    /// Sets a per-employee per-period spending cap.
    ///
    /// When `cap` is 0 (or the key is absent), no cap is enforced for that
    /// employee. Only the contract owner may call this function.
    pub fn set_employee_cap(env: Env, caller: Address, employee: Address, cap: i128) {
        require_initialized(&env);
        require_owner(&env, &caller);
        assert!(cap >= 0, "Cap cannot be negative");
        if cap == 0 {
            env.storage()
                .persistent()
                .remove(&StorageKey::EmployeeCap(employee));
        } else {
            env.storage()
                .persistent()
                .set(&StorageKey::EmployeeCap(employee), &cap);
        }
    }

    /// Sets the period duration in seconds (e.g. 2_592_000 for 30 days).
    /// Only the contract owner may call this function.
    pub fn set_period_duration(env: Env, caller: Address, duration: u64) {
        require_initialized(&env);
        require_owner(&env, &caller);
        assert!(duration > 0, "Period duration must be positive");
        env.storage()
            .persistent()
            .set(&StorageKey::PeriodDuration, &duration);
    }

    /// Returns the per-employee spending cap (0 means no cap).
    pub fn get_employee_cap(env: Env, employee: Address) -> i128 {
        require_initialized(&env);
        read_employee_cap(&env, &employee)
    }

    /// Returns the cumulative amount the employee has spent in the current period.
    pub fn get_employee_period_spent(env: Env, employee: Address) -> i128 {
        require_initialized(&env);
        let period = current_period(&env);
        read_period_spent(&env, &employee, period)
    }
}
