use soroban_sdk::{contracttype, Address, BytesN};

/// Canonical record of a single completed payment.
///
/// Once written to storage under `StorageKey::Payment(id)`, this record is
/// never modified. Immutability is enforced at the contract level: there is no
/// update or delete code path. Any discrepancy between this record and an
/// off-chain index is therefore always an off-chain artifact, never a
/// contract-side rewrite.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentRecord {
    /// Globally unique payment identifier (1-based, monotonically increasing).
    /// This is the primary key used by all three indices.
    pub id: u128,

    /// The employment agreement this payment belongs to.
    pub agreement_id: u128,

    /// 32-byte reference hash supplied by the payroll contract at record time.
    ///
    /// @dev Typically the Stellar transaction hash of the token transfer, so
    /// indexers and UI clients can deep-link directly to the on-chain transaction
    /// without recomputing payroll math. The contract stores it verbatim and does
    /// not verify its content; integrity depends on the trustworthy payroll caller.
    /// A reverse-lookup index (`StorageKey::PaymentByHash`) enables O(1) queries
    /// by hash in addition to queries by ID, agreement, employer, and employee.
    pub payment_hash: BytesN<32>,

    /// Stellar asset contract address of the token transferred.
    pub token: Address,

    /// Transfer amount in the token's smallest base unit.
    pub amount: i128,

    /// Employer address that originated the payment.
    pub from: Address,

    /// Employee address that received the payment.
    pub to: Address,

    /// Unix timestamp (seconds) recorded by the payroll contract at the time
    /// of the transfer.
    pub timestamp: u64,
}

/// Enumeration of all persistent storage keys used by this contract.
///
/// Key layout is designed for O(1) point reads and O(n) sequential page reads:
///
/// ```text
/// Owner                                → Address
/// PayrollContract                      → Address
/// GlobalPaymentCount                   → u128   (highest assigned ID)
/// Payment(global_id)                   → PaymentRecord
/// PaymentByHash(hash)                  → u128   (global_id for reverse lookup)
///
/// AgreementPaymentCount(agreement_id)  → u32    (# payments for agreement)
/// AgreementPayment(agreement_id, pos)  → u128   (global_id at 1-based pos)
///
/// EmployerPaymentCount(employer)       → u32    (# payments by employer)
/// EmployerPayment(employer, pos)       → u128   (global_id at 1-based pos)
///
/// EmployeePaymentCount(employee)       → u32    (# payments to employee)
/// EmployeePayment(employee, pos)       → u128   (global_id at 1-based pos)
/// ```
///
/// The three index families (Agreement, Employer, Employee) share the same
/// pagination pattern: a `*Count` key records the total and the maximum valid
/// position, while `*(entity, position)` keys are set once and never mutated.
#[contracttype]
pub enum StorageKey {
    /// Address of the contract owner (reserved for future governance).
    Owner,

    /// The only address permitted to call `record_payment`.
    PayrollContract,

    /// Monotonically increasing counter of all recorded payments.
    /// Also the highest currently valid Global Payment ID.
    GlobalPaymentCount,

    /// Primary record store: Global Payment ID → full `PaymentRecord`.
    /// Written once at record time; never updated.
    Payment(u128),

    /// Reverse-lookup index: payment hash → Global Payment ID.
    ///
    /// @dev Written once per payment alongside the primary record. Enables
    /// O(1) hash-based lookups without scanning. If the payroll contract
    /// supplies the Stellar transaction hash, indexers can navigate from any
    /// on-chain transaction directly to its `PaymentRecord`.
    PaymentByHash(BytesN<32>),

    // ── Agreement index ────────────────────────────────────────────────────

    /// Total number of payments recorded for this agreement.
    /// Doubles as the 1-based position of the most recently appended entry.
    AgreementPaymentCount(u128), // key: agreement_id

    /// Pagination pointer for the agreement index.
    /// `AgreementPayment(agreement_id, position)` maps each 1-based position
    /// to a global payment ID. Written once per entry; never mutated.
    AgreementPayment(u128, u32), // key: (agreement_id, 1-based position)

    // ── Employer (from) index ──────────────────────────────────────────────

    /// Total number of payments originating from this employer address.
    EmployerPaymentCount(Address), // key: employer

    /// Pagination pointer for the employer index.
    /// `EmployerPayment(employer, position)` maps each 1-based position to a
    /// global payment ID, partitioned by employer address.
    EmployerPayment(Address, u32), // key: (employer, 1-based position)

    // ── Employee (to) index ────────────────────────────────────────────────

    /// Total number of payments received by this employee address.
    EmployeePaymentCount(Address), // key: employee

    /// Pagination pointer for the employee index.
    /// `EmployeePayment(employee, position)` maps each 1-based position to a
    /// global payment ID, partitioned by employee address.
    EmployeePayment(Address, u32), // key: (employee, 1-based position)
}
