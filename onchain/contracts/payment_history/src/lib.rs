//! # PaymentHistory — Stable Payment History Contract
//!
//! This contract records all completed payments within the StelloPay ecosystem
//! and exposes a stable, read-only query surface so off-chain indexers and UI
//! clients can reconstruct payment histories without recomputing payroll math.
//!
//! ## Design Overview
//!
//! Each payment is assigned a monotonically increasing **Global Payment ID**
//! (starting at 1) and a caller-supplied **32-byte payment hash** (typically
//! the Stellar transaction hash of the underlying token transfer). The record
//! is stored under its ID and simultaneously indexed by:
//! * **Hash** — O(1) reverse lookup from transaction hash to payment record.
//! * **Agreement** — paginate all payments for a specific employment agreement.
//! * **Employer (from)** — employer-level disbursement dashboards.
//! * **Employee (to)** — per-employee pay-stub / audit views.
//!
//! All indices are **append-only**: once written, no index entry is ever
//! mutated or removed. This preserves historical integrity and prevents
//! tampering.
//!
//! ## Pagination
//!
//! All paginated query functions accept a 1-based `start_index` and a `limit`.
//! The maximum page size is capped at [`MAX_PAGE_SIZE`] to prevent runaway
//! ledger reads. A `start_index` of `0` or greater than the total count
//! returns an empty vector.
//!
//! Example: to walk all records for an agreement in batches of 20:
//! ```text
//! page 1: start_index=1,  limit=20
//! page 2: start_index=21, limit=20
//! page 3: start_index=41, limit=20
//! ```
//!
//! ## Security Model
//!
//! * Only the **payroll contract** registered at initialization may call
//!   `record_payment`. Any other caller receives an `Auth(InvalidAction)` error.
//! * The contract may only be initialized **once**; subsequent calls panic with
//!   "Already initialized".
//! * Records are **immutable**: there is no update or delete path. Index
//!   entries are written once and never modified, preventing history tampering.
//! * Index counts can only increase, ensuring no entry can be silently replaced
//!   and no historical record can be pruned by an unauthorized party.
//! * `limit` is hard-capped at [`MAX_PAGE_SIZE`] (100) to bound ledger reads
//!   per invocation and prevent resource exhaustion by adversarial callers.
//! * `payment_hash` is stored verbatim from the payroll contract. Its integrity
//!   is the payroll contract's responsibility; this contract does not verify it.
//!
//! ## Integration with Indexers
//!
//! Subscribe to the `payment_recorded` event to keep an off-chain index in
//! sync. Each event carries both `payment_id` (sequential position key) and
//! `payment_hash` (transaction-level reference key). Because records are
//! immutable, indexers never need to handle update or delete messages.

#![no_std]

mod events;
mod storage;

use events::PaymentRecorded;
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Vec};
use storage::StorageKey;

/// Re-export `PaymentRecord` so consumers and tests can import it directly
/// from the crate root.
pub use storage::PaymentRecord;

/// Maximum number of records returned in a single paginated query.
///
/// Capping page size prevents runaway ledger-entry reads that could exhaust
/// the resource budget on agreements or accounts with very large histories.
/// Callers requesting a larger `limit` receive at most this many records
/// silently; no error is raised.
pub const MAX_PAGE_SIZE: u32 = 100;

#[contract]
pub struct PaymentHistoryContract;

#[contractimpl]
impl PaymentHistoryContract {
    /// Initialize the contract with an owner and the authorized payroll contract.
    ///
    /// @notice Must be called exactly once before any other function.
    /// @dev Stores `owner` and `payroll_contract` in persistent storage and
    /// seeds the global payment counter at zero.
    ///
    /// @param owner            Admin address reserved for future governance.
    /// @param payroll_contract The sole address permitted to call `record_payment`.
    ///
    /// @panics "Already initialized" if called more than once.
    pub fn initialize(env: Env, owner: Address, payroll_contract: Address) {
        if env.storage().persistent().has(&StorageKey::Owner) {
            panic!("Already initialized");
        }
        env.storage().persistent().set(&StorageKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&StorageKey::PayrollContract, &payroll_contract);
        env.storage()
            .persistent()
            .set(&StorageKey::GlobalPaymentCount, &0u128);
    }

    /// Record a completed payment. Only callable by the registered payroll contract.
    ///
    /// @notice Idempotently records a completed payment keyed by `payment_hash`.
    /// On first-seen hash: assigns a globally unique, monotonically increasing ID,
    /// writes the full `PaymentRecord`, updates reverse/hash lookup and all three
    /// append-only indices, then emits `payment_recorded`.
    /// On duplicate hash: returns the existing ID without mutating storage.
    ///
    /// @dev Security: `payroll_contract.require_auth()` enforces that only the address
    /// registered at initialization may invoke this function. No other caller can write
    /// to the history, preventing unauthorized record injection. The global counter is
    /// incremented before index writes so that a partial failure never aliases two
    /// records to the same ID.
    ///
    /// @param agreement_id  The agreement this payment belongs to.
    /// @param payment_hash  32-byte reference hash (e.g. Stellar transaction hash)
    ///                      supplied by the payroll contract. Stored verbatim and
    ///                      indexed for O(1) reverse lookup by hash.
    /// @param token         Stellar asset contract address used for the transfer.
    /// @param amount        Transfer amount in the token's base unit (positive).
    /// @param from          Employer address (payer).
    /// @param to            Employee address (payee).
    /// @param timestamp     Unix timestamp (seconds) provided by the payroll contract.
    ///
    /// @return The existing Global Payment ID for duplicate hash input, otherwise
    ///         the newly assigned ID (starts at 1, increments by 1).
    ///
    /// @panics "HostError: Error(Auth, InvalidAction)" when called by any address
    ///         other than the registered payroll contract.
    pub fn record_payment(
        env: Env,
        agreement_id: u128,
        payment_hash: BytesN<32>,
        token: Address,
        amount: i128,
        from: Address,
        to: Address,
        timestamp: u64,
    ) -> u128 {
        // Only the registered payroll contract may inject payment records.
        let payroll_contract: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::PayrollContract)
            .unwrap();
        payroll_contract.require_auth();

        // Idempotency guard: replaying the same payment hash must not create
        // a new record or mutate indices.
        let existing_id: Option<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::PaymentByHash(payment_hash.clone()));
        if let Some(id) = existing_id {
            return id;
        }

        // Assign a new globally unique, monotonically increasing ID.
        let mut global_count: u128 = env
            .storage()
            .persistent()
            .get(&StorageKey::GlobalPaymentCount)
            .unwrap_or(0);
        global_count += 1;
        env.storage()
            .persistent()
            .set(&StorageKey::GlobalPaymentCount, &global_count);

        let id = global_count;

        // Persist the canonical payment record keyed by its global ID.
        let record = PaymentRecord {
            id,
            agreement_id,
            payment_hash: payment_hash.clone(),
            token: token.clone(),
            amount,
            from: from.clone(),
            to: to.clone(),
            timestamp,
        };
        env.storage()
            .persistent()
            .set(&StorageKey::Payment(id), &record);

        // ── Reverse-lookup index: Hash → Global ID ───────────────────────────
        // StorageKey::PaymentByHash(hash) → global_id enables O(1) point reads
        // by transaction hash. Indexers that receive a Stellar transaction hash
        // from the network can look up the associated PaymentRecord directly
        // without scanning any sequential index.
        env.storage()
            .persistent()
            .set(&StorageKey::PaymentByHash(payment_hash.clone()), &id);

        // ── Append-only index: Agreement ─────────────────────────────────────
        // StorageKey::AgreementPaymentCount(agreement_id) tracks how many
        // payments exist for this agreement and acts as the 1-based position
        // counter. Incrementing it before writing the pointer means the count
        // always reflects the highest valid position.
        //
        // Pagination key: AgreementPayment(agreement_id, position) → global_id
        // Consumers iterate positions [start_index, start_index + limit) and
        // dereference each position to the canonical Payment(global_id) record.
        let mut agg_count: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::AgreementPaymentCount(agreement_id))
            .unwrap_or(0);
        agg_count += 1;
        env.storage()
            .persistent()
            .set(&StorageKey::AgreementPaymentCount(agreement_id), &agg_count);
        env.storage()
            .persistent()
            .set(&StorageKey::AgreementPayment(agreement_id, agg_count), &id);

        // ── Append-only index: Employer (from) ───────────────────────────────
        // Mirrors the agreement index strategy, partitioned by employer address.
        //
        // Pagination key: EmployerPayment(employer, position) → global_id
        let mut from_count: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployerPaymentCount(from.clone()))
            .unwrap_or(0);
        from_count += 1;
        env.storage()
            .persistent()
            .set(&StorageKey::EmployerPaymentCount(from.clone()), &from_count);
        env.storage()
            .persistent()
            .set(&StorageKey::EmployerPayment(from.clone(), from_count), &id);

        // ── Append-only index: Employee (to) ─────────────────────────────────
        // Mirrors the agreement index strategy, partitioned by employee address.
        //
        // Pagination key: EmployeePayment(employee, position) → global_id
        let mut to_count: u32 = env
            .storage()
            .persistent()
            .get(&StorageKey::EmployeePaymentCount(to.clone()))
            .unwrap_or(0);
        to_count += 1;
        env.storage()
            .persistent()
            .set(&StorageKey::EmployeePaymentCount(to.clone()), &to_count);
        env.storage()
            .persistent()
            .set(&StorageKey::EmployeePayment(to.clone(), to_count), &id);

        // Emit event so indexers can build real-time payment feeds without
        // polling storage. Both payment_id and payment_hash are included so
        // indexers can key their off-chain tables by either dimension.
        events::emit_payment_recorded(
            &env,
            PaymentRecorded {
                payment_id: id,
                payment_hash,
                agreement_id,
                token,
                amount,
                from,
                to,
                timestamp,
            },
        );

        id
    }

    /// Look up a payment record by its 32-byte reference hash.
    ///
    /// @notice Returns `None` if no payment with the given hash has been recorded.
    /// @dev Performs an O(1) lookup via the `PaymentByHash` reverse index. If
    /// the payroll contract stored the Stellar transaction hash, this function
    /// lets indexers navigate directly from any on-chain transaction to its
    /// `PaymentRecord` without scanning sequential indices.
    ///
    /// @param payment_hash  The 32-byte hash to look up.
    /// @return              The matching `PaymentRecord`, or `None` if not found.
    pub fn get_payment_by_hash(env: Env, payment_hash: BytesN<32>) -> Option<PaymentRecord> {
        let global_id: Option<u128> = env
            .storage()
            .persistent()
            .get(&StorageKey::PaymentByHash(payment_hash));
        global_id.and_then(|id| {
            env.storage()
                .persistent()
                .get(&StorageKey::Payment(id))
        })
    }

    /// Fetch a single payment record by its global ID.
    ///
    /// @notice IDs are assigned sequentially from 1. The maximum valid ID equals
    /// `get_global_payment_count`. Returns `None` for IDs that have not been
    /// assigned yet (including 0).
    ///
    /// @param payment_id  The global payment ID to look up.
    /// @return            The matching `PaymentRecord`, or `None` if not found.
    pub fn get_payment_by_id(env: Env, payment_id: u128) -> Option<PaymentRecord> {
        env.storage()
            .persistent()
            .get(&StorageKey::Payment(payment_id))
    }

    /// Return the total number of payments recorded across all agreements.
    ///
    /// @dev The return value is also the highest currently assigned Global
    /// Payment ID. It starts at 0 before any payment is recorded and
    /// increments monotonically with each successful `record_payment` call.
    ///
    /// @return Total number of recorded payments (0 if none recorded yet).
    pub fn get_global_payment_count(env: Env) -> u128 {
        env.storage()
            .persistent()
            .get(&StorageKey::GlobalPaymentCount)
            .unwrap_or(0)
    }

    /// Return the number of payments recorded for a specific agreement.
    ///
    /// @param agreement_id  The agreement to query.
    /// @return              Total payment count for this agreement (0 if none).
    pub fn get_agreement_payment_count(env: Env, agreement_id: u128) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::AgreementPaymentCount(agreement_id))
            .unwrap_or(0)
    }

    /// Return a paginated slice of payment records for a specific agreement.
    ///
    /// @notice `start_index` is 1-based and inclusive. A value of `0` or greater
    /// than the total count returns an empty vector.
    ///
    /// @dev `limit` is silently capped to [`MAX_PAGE_SIZE`] (100).
    /// Storage key: `AgreementPayment(agreement_id, position)` maps each 1-based
    /// position to a global payment ID which is then dereferenced to the full record.
    ///
    /// @param agreement_id  The agreement to query.
    /// @param start_index   1-based start position (inclusive).
    /// @param limit         Maximum records to return; capped at 100.
    /// @return              Ordered slice of `PaymentRecord`s, oldest-first.
    pub fn get_payments_by_agreement(
        env: Env,
        agreement_id: u128,
        start_index: u32,
        limit: u32,
    ) -> Vec<PaymentRecord> {
        let count = Self::get_agreement_payment_count(env.clone(), agreement_id);
        let mut result = Vec::new(&env);

        if start_index == 0 || start_index > count {
            return result;
        }

        let effective_limit = limit.min(MAX_PAGE_SIZE);
        let end = start_index.saturating_add(effective_limit).min(count.saturating_add(1));

        for i in start_index..end {
            let global_id: u128 = env
                .storage()
                .persistent()
                .get(&StorageKey::AgreementPayment(agreement_id, i))
                .unwrap();
            let record: PaymentRecord = env
                .storage()
                .persistent()
                .get(&StorageKey::Payment(global_id))
                .unwrap();
            result.push_back(record);
        }
        result
    }

    /// Return the number of payments recorded where `from` is the given employer.
    ///
    /// @param employer  The employer address to query.
    /// @return          Total payment count for this employer (0 if none).
    pub fn get_employer_payment_count(env: Env, employer: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployerPaymentCount(employer))
            .unwrap_or(0)
    }

    /// Return a paginated slice of payment records for a specific employer.
    ///
    /// @notice `start_index` is 1-based and inclusive. A value of `0` or greater
    /// than the total count returns an empty vector.
    ///
    /// @dev `limit` is silently capped to [`MAX_PAGE_SIZE`] (100).
    /// Storage key: `EmployerPayment(employer, position)` maps each 1-based
    /// position to a global payment ID, partitioned by employer address.
    ///
    /// @param employer    The employer address to query.
    /// @param start_index 1-based start position (inclusive).
    /// @param limit       Maximum records to return; capped at 100.
    /// @return            Ordered slice of `PaymentRecord`s, oldest-first.
    pub fn get_payments_by_employer(
        env: Env,
        employer: Address,
        start_index: u32,
        limit: u32,
    ) -> Vec<PaymentRecord> {
        let count = Self::get_employer_payment_count(env.clone(), employer.clone());
        let mut result = Vec::new(&env);

        if start_index == 0 || start_index > count {
            return result;
        }

        let effective_limit = limit.min(MAX_PAGE_SIZE);
        let end = start_index.saturating_add(effective_limit).min(count.saturating_add(1));

        for i in start_index..end {
            let global_id: u128 = env
                .storage()
                .persistent()
                .get(&StorageKey::EmployerPayment(employer.clone(), i))
                .unwrap();
            let record: PaymentRecord = env
                .storage()
                .persistent()
                .get(&StorageKey::Payment(global_id))
                .unwrap();
            result.push_back(record);
        }
        result
    }

    /// Return the number of payments recorded where `to` is the given employee.
    ///
    /// @param employee  The employee address to query.
    /// @return          Total payment count for this employee (0 if none).
    pub fn get_employee_payment_count(env: Env, employee: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::EmployeePaymentCount(employee))
            .unwrap_or(0)
    }

    /// Return a paginated slice of payment records for a specific employee.
    ///
    /// @notice `start_index` is 1-based and inclusive. A value of `0` or greater
    /// than the total count returns an empty vector.
    ///
    /// @dev `limit` is silently capped to [`MAX_PAGE_SIZE`] (100).
    /// Storage key: `EmployeePayment(employee, position)` maps each 1-based
    /// position to a global payment ID, partitioned by employee address.
    ///
    /// @param employee    The employee address to query.
    /// @param start_index 1-based start position (inclusive).
    /// @param limit       Maximum records to return; capped at 100.
    /// @return            Ordered slice of `PaymentRecord`s, oldest-first.
    pub fn get_payments_by_employee(
        env: Env,
        employee: Address,
        start_index: u32,
        limit: u32,
    ) -> Vec<PaymentRecord> {
        let count = Self::get_employee_payment_count(env.clone(), employee.clone());
        let mut result = Vec::new(&env);

        if start_index == 0 || start_index > count {
            return result;
        }

        let effective_limit = limit.min(MAX_PAGE_SIZE);
        let end = start_index.saturating_add(effective_limit).min(count.saturating_add(1));

        for i in start_index..end {
            let global_id: u128 = env
                .storage()
                .persistent()
                .get(&StorageKey::EmployeePayment(employee.clone(), i))
                .unwrap();
            let record: PaymentRecord = env
                .storage()
                .persistent()
                .get(&StorageKey::Payment(global_id))
                .unwrap();
            result.push_back(record);
        }
        result
    }
}
