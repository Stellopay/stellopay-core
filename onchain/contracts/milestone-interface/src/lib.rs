//! # `milestone-interface` — Stable Cross-Contract Trait for Milestone Queries
//!
//! This crate defines the canonical read-only interface that any contract may
//! use to inspect milestone state on a deployed `stello_pay_contract` (or any
//! compatible implementation) without linking the full cdylib.
//!
//! ## Crate role
//!
//! `stello_pay_contract` owns all milestone state and exposes mutating
//! entrypoints (fund, add, approve, reject, expire, claim).  This crate
//! exposes only:
//!
//! - **Query methods** — `get_milestone`, `get_milestone_count`
//! - **Extension hook** — `on_milestone_expired` (optional override, no-op default)
//! - **Shared types** — `MilestoneView`, `MilestoneAgreementView`,
//!   `MilestoneAgreementStatus`
//!
//! ## Usage
//!
//! ```ignore
//! use milestone_interface::{MilestoneContractClient, MilestoneView};
//!
//! let client = MilestoneContractClient::new(&env, &milestone_contract_address);
//! let milestone = client.get_milestone(&agreement_id, &milestone_id);
//! let count    = client.get_milestone_count(&agreement_id);
//! ```
//!
//! ## Conformance testing
//!
//! Contracts that implement this trait should include a conformance test that
//! exercises the full trait surface via `MilestoneContractClient` and compares
//! results against direct contract-client calls.  See
//! `test_milestone_interface_conformance` in
//! `stello_pay_contract/tests/test_milestones.rs` for the reference
//! implementation.
//!
//! ---
//!
//! # Versioning and Backward-Compatibility Policy
//!
//! ## Current interface version
//!
//! ```text
//! INTERFACE_VERSION = 1
//! ```
//!
//! The constant [`INTERFACE_VERSION`] is the single source of truth for the
//! interface generation.  It is a compile-time `u32` that implementors and
//! off-chain tooling may read to gate behavior at the call site.
//!
//! ## Stability levels
//!
//! Every item in this crate carries one of three stability labels:
//!
//! | Label | Meaning |
//! |-------|---------|
//! | **`@stable`** | Signature, semantics, and XDR encoding are frozen. Changes require a major version bump and a deprecation cycle. |
//! | **`@stable-default`** | Method has a provided default body (no-op). The *presence* of the method is stable; the *default body* may evolve between minor versions as long as the observable no-op contract is preserved. |
//! | **`@unstable`** | May change in any release without notice. Not suitable for third-party production use. |
//!
//! All items currently in this crate are **`@stable`** or
//! **`@stable-default`** unless explicitly annotated otherwise.
//!
//! ## What counts as a breaking change
//!
//! The following changes **require a major version bump** (`INTERFACE_VERSION`
//! incremented, old version supported in parallel for one release cycle):
//!
//! 1. **Removing any trait method** — existing implementors no longer compile.
//! 2. **Changing a method signature** — parameter type, parameter order, or
//!    return type change breaks both callers and implementors at compile time.
//! 3. **Changing XDR-encoded type layouts** — adding, removing, or reordering
//!    fields on `#[contracttype]` structs or enums used as method parameters or
//!    return values breaks cross-contract calls at runtime even when Rust code
//!    compiles cleanly.
//! 4. **Narrowing a method's contract** — e.g. changing a previously
//!    documented "returns `None` on unknown id" guarantee to "panics on unknown
//!    id" is a semantic breaking change even if the signature is unchanged.
//! 5. **Changing the discriminant value of an existing enum variant** — XDR
//!    decoding on the calling side will misinterpret the value.
//!
//! ## What counts as an additive (non-breaking) change
//!
//! The following changes are **backward-compatible** and do not require a
//! major version bump:
//!
//! 1. **Adding a new method with a provided default body** — existing
//!    implementors continue to compile without change; the default is silently
//!    inherited.  Callers that depend on this crate gain access to the new
//!    method immediately.
//! 2. **Widening a method's contract** — e.g. changing a "may panic on
//!    unknown id" guarantee to "returns `None` on unknown id" is strictly more
//!    permissive and cannot break existing callers.
//! 3. **Adding a new associated type or constant** — provided the addition does
//!    not break existing `impl` blocks (i.e., the new item has a default or is
//!    not required to be implemented).
//! 4. **Adding a new variant to an enum** — if and only if (a) the new variant
//!    is appended at the end so existing discriminants are unchanged, and (b)
//!    call sites that match on the enum already handle a wildcard arm `_`.
//! 5. **Adding new `#[contracttype]` structs** that are not used as method
//!    parameters or return values yet — they are inert until referenced.
//! 6. **Improving or expanding doc comments** — purely documentary; no runtime
//!    impact.
//!
//! ## Implementor guidance
//!
//! Third-party contracts that implement `MilestoneContractInterface` should:
//!
//! 1. **Pin the `milestone-interface` version** in `Cargo.toml` using an exact
//!    or `~` specifier (e.g. `milestone-interface = { version = "=0.0.0", … }`).
//!    Open `*` ranges silently pick up breaking changes on re-build.
//! 2. **Add a conformance test** (see `test_milestone_interface_conformance` in
//!    `stello_pay_contract/tests/test_milestones.rs`) that exercises every
//!    `@stable` method via `MilestoneContractClient` and compares output against
//!    direct client calls.
//! 3. **Check `INTERFACE_VERSION`** at deploy time if your contract needs to
//!    guard against future interface changes (e.g. via a `static_assert`-style
//!    test that pins the expected version value).
//! 4. **Do not override `on_milestone_expired` with a panicking body** unless
//!    your contract can guarantee the hook is always called in a valid state.
//!    A panic inside the hook rolls back the entire `expire_milestone`
//!    transaction in the calling contract.
//!
//! ## Cross-reference
//!
//! - Milestone system workflow and data structures:
//!   [`stello_pay_contract/MILESTONE_DOCS.md`](../../stello_pay_contract/MILESTONE_DOCS.md)
//! - Agreement state machine (Created/Active/Paused/Cancelled/Completed/Disputed):
//!   [`docs/state-machines.md`](../../../../../docs/state-machines.md)
//! - Conformance test suite:
//!   [`stello_pay_contract/tests/test_milestones.rs`](../../stello_pay_contract/tests/test_milestones.rs)
//! - Versioning and compatibility policy (extended prose):
//!   [`docs/state-machines.md § Milestone Interface Versioning`](../../../../../docs/state-machines.md)
//! - `PayrollError` discriminant stability convention (append-only):
//!   [`stello_pay_contract/src/storage.rs`](../../stello_pay_contract/src/storage.rs)

#![no_std]

use soroban_sdk::{contractclient, contracttype, Address, Env};

// ─────────────────────────────────────────────────────────────────────────────
// Interface version
// ─────────────────────────────────────────────────────────────────────────────

/// Monotonically increasing version of the `MilestoneContractInterface` trait.
///
/// # Stability: `@stable`
///
/// This constant is the single source of truth for the interface generation.
/// Off-chain indexers, CI pipelines, and third-party contracts may read this
/// value to assert they are compiled against the expected interface revision.
///
/// ## Version history
///
/// | Version | Changes |
/// |---------|---------|
/// | `1` | Initial stable release. Defines `get_milestone`, `get_milestone_count`, and the `on_milestone_expired` hook. |
///
/// ## Upgrade procedure
///
/// When a breaking change is required (see the crate-level policy):
///
/// 1. Increment `INTERFACE_VERSION` by 1.
/// 2. Keep the previous interface accessible under a versioned re-export or
///    sibling crate (`milestone-interface-v1`) for one full release cycle.
/// 3. Update `stello_pay_contract` to implement the new version.
/// 4. Update the conformance test in
///    `stello_pay_contract/tests/test_milestones.rs` to pin the new value.
/// 5. Update `docs/state-machines.md` with a changelog entry.
pub const INTERFACE_VERSION: u32 = 1;

// ─────────────────────────────────────────────────────────────────────────────
// Shared types
// ─────────────────────────────────────────────────────────────────────────────

/// Lifecycle states for milestone agreements.
///
/// # Stability: `@stable`
///
/// ## XDR encoding note
///
/// This enum is encoded by the Soroban host as a `SCVal::SCV_VEC` with the
/// variant name as the discriminant.  The **order of variants must never be
/// changed** and **new variants must always be appended** so that existing
/// XDR streams remain decodable by older contract versions.
///
/// ## Variant stability
///
/// | Variant | Discriminant (0-based) | Since version |
/// |---------|------------------------|---------------|
/// | `Created` | 0 | 1 |
/// | `Active` | 1 | 1 |
/// | `Paused` | 2 | 1 |
/// | `Cancelled` | 3 | 1 |
/// | `Completed` | 4 | 1 |
/// | `Disputed` | 5 | 1 |
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MilestoneAgreementStatus {
    Created,
    Active,
    Paused,
    Cancelled,
    Completed,
    Disputed,
}

/// A single milestone within an agreement, as seen through the interface.
///
/// # Stability: `@stable`
///
/// ## Field stability
///
/// | Field | Type | Since version | Notes |
/// |-------|------|---------------|-------|
/// | `id` | `u32` | 1 | 1-based sequential identifier within the agreement. |
/// | `amount` | `i128` | 1 | Token units claimable for this milestone; always > 0. |
/// | `approved` | `bool` | 1 | `true` once the employer has approved. |
/// | `claimed` | `bool` | 1 | `true` once the contributor has claimed payment. |
///
/// ## XDR encoding note
///
/// Fields are encoded in declaration order.  **Field order must never change**
/// and **new fields must always be appended** to preserve backward
/// compatibility with callers compiled against an older version of this struct.
///
/// ## Relationship to `stello_pay_contract::storage::Milestone`
///
/// `MilestoneView` carries the same four scalar fields as the internal
/// `Milestone` struct.  The conformance test
/// (`test_milestone_interface_conformance`) asserts field-for-field equality
/// between the two types at every lifecycle stage, ensuring no divergence
/// creeps in between the storage type and the interface type.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneView {
    /// 1-based milestone identifier within the agreement.
    pub id: u32,
    /// Token amount claimable for this milestone.
    pub amount: i128,
    /// `true` once the employer has approved this milestone.
    pub approved: bool,
    /// `true` once the contributor has claimed this milestone's payment.
    pub claimed: bool,
}

/// Summary view of a milestone agreement.
///
/// # Stability: `@stable`
///
/// ## Field stability
///
/// | Field | Type | Since version |
/// |-------|------|---------------|
/// | `id` | `u128` | 1 |
/// | `employer` | `Address` | 1 |
/// | `contributor` | `Address` | 1 |
/// | `token` | `Address` | 1 |
/// | `status` | `MilestoneAgreementStatus` | 1 |
/// | `total_amount` | `i128` | 1 |
/// | `escrow_balance` | `i128` | 1 |
/// | `milestone_count` | `u32` | 1 |
#[contracttype]
#[derive(Clone, Debug)]
pub struct MilestoneAgreementView {
    pub id: u128,
    pub employer: Address,
    pub contributor: Address,
    pub token: Address,
    pub status: MilestoneAgreementStatus,
    pub total_amount: i128,
    /// Accounted escrow balance (tokens deposited via `fund_milestone_agreement`).
    pub escrow_balance: i128,
    /// Number of milestones added to this agreement.
    pub milestone_count: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Trait definition
// ─────────────────────────────────────────────────────────────────────────────

/// Thin cross-contract interface for milestone agreement queries and lifecycle
/// extension hooks.
///
/// # Stability: `@stable` (interface version [`INTERFACE_VERSION`] = 1)
///
/// This trait is the **stable contract surface** that third-party
/// milestone-capable contracts build against.  All `@stable` methods are
/// guaranteed not to change signature or semantics without a major version
/// bump.
///
/// ## Scope
///
/// Only query / view methods and optional hooks are exposed here.  Mutating
/// calls (fund, add, approve, reject, expire, claim) are performed directly on
/// `stello_pay_contract`, which owns the state.
///
/// ## Method surface (version 1)
///
/// | Method | Stability | Description |
/// |--------|-----------|-------------|
/// | `get_milestone` | `@stable` | Returns a specific milestone by (agreement_id, milestone_id), or `None`. |
/// | `get_milestone_count` | `@stable` | Returns the number of milestones in an agreement, or `0` for an unknown agreement. |
/// | `on_milestone_expired` | `@stable-default` | Hook invoked after a milestone is expired. Default body is a no-op. |
///
/// ## Extension hooks
///
/// Hooks are methods with a provided no-op default body.  Implementors can
/// override them to react to lifecycle events without breaking the interface
/// contract for callers that do not need the behaviour.
///
/// ## `on_milestone_expired`
///
/// **Convention:** `stello_pay_contract` calls this hook from its
/// `expire_milestone` entry-point immediately after persisting the
/// `MilestoneKey::MilestoneExpired` flag and emitting the
/// `MilestoneExpiredEvent`.  Because Soroban traits cannot enforce call-site
/// ordering at the type level, the contract that implements this trait is
/// responsible for ensuring the hook is only invoked once per milestone and
/// only after expiry has been durably recorded.
///
/// Implementors should treat the hook as best-effort: if it panics the whole
/// `expire_milestone` transaction is rolled back, so implementations should
/// be defensive and avoid panicking on unexpected state.
///
/// ## Security assumptions
///
/// - **No mutable state access** — query methods must be read-only.  Any
///   implementation that writes state in a query method violates the
///   interface contract and may expose reentrancy attack surfaces.
/// - **No panicking on valid input** — query methods must return `None` / `0`
///   for unknown IDs rather than panicking, so callers can distinguish "not
///   found" from "error" without needing `try_` variants.
/// - **Hook idempotency** — `on_milestone_expired` may be retried by the host
///   on transient failures.  Implementations should be idempotent or
///   guard against double-invocation.
///
/// ## Cross-reference
///
/// - Crate-level versioning policy: see the module-level documentation above.
/// - Milestone workflow: `stello_pay_contract/MILESTONE_DOCS.md`
/// - Agreement state machines: `docs/state-machines.md`
/// - Conformance test: `stello_pay_contract/tests/test_milestones.rs`
///   (`test_milestone_interface_conformance`)
#[contractclient(name = "MilestoneContractClient")]
pub trait MilestoneContractInterface {
    /// Returns a specific milestone, or `None` if the milestone does not exist.
    ///
    /// # Stability: `@stable` (since interface version 1)
    ///
    /// @notice Returns the full state of a single milestone within an agreement.
    /// @param  agreement_id  The 128-bit agreement identifier.
    /// @param  milestone_id  The 1-based milestone identifier within the agreement.
    /// @return               `Some(MilestoneView)` if the milestone exists;
    ///                       `None` if `agreement_id` is unrecognized or
    ///                       `milestone_id` is out of range (0 or > count).
    ///
    /// # Arguments
    /// * `agreement_id` - The agreement to query.
    /// * `milestone_id` - The 1-based milestone identifier within the agreement.
    ///
    /// # Returns
    /// `Some(MilestoneView)` if the milestone exists; `None` if the
    /// `agreement_id` is unrecognized or the `milestone_id` is out of range.
    ///
    /// # Errors / panics
    /// Implementors **must not panic** on invalid input.  Return `None` for any
    /// caller error (unknown agreement, out-of-range id, etc.) and let the
    /// caller decide how to handle the missing value.
    ///
    /// # Backward-compatibility guarantee
    /// - The method name, parameter types, parameter order, and return type are
    ///   frozen for the lifetime of interface version 1.
    /// - The field set of `MilestoneView` (`id`, `amount`, `approved`,
    ///   `claimed`) is frozen; new fields may only be appended in a future
    ///   major version.
    fn get_milestone(env: Env, agreement_id: u128, milestone_id: u32) -> Option<MilestoneView>;

    /// Returns the number of milestones in an agreement.
    ///
    /// # Stability: `@stable` (since interface version 1)
    ///
    /// @notice Returns the total count of milestones registered for an agreement.
    /// @param  agreement_id  The 128-bit agreement identifier.
    /// @return               The milestone count, or `0` for an unknown agreement.
    ///
    /// # Arguments
    /// * `agreement_id` - The agreement to query.
    ///
    /// # Returns
    /// The milestone count for the agreement, or `0` if the `agreement_id` is
    /// unrecognized.  Callers that need to distinguish "no agreement" from "an
    /// agreement with zero milestones" should perform a separate existence
    /// check.
    ///
    /// # Errors / panics
    /// Implementors **must not panic** on any input.  Return `0` for an unknown
    /// `agreement_id`.
    ///
    /// # Backward-compatibility guarantee
    /// - The method name, parameter types, and return type are frozen for the
    ///   lifetime of interface version 1.
    /// - The return value `0` for unknown agreements is a documented semantic
    ///   guarantee, not an implementation detail.  Changing it to a panic
    ///   would be a breaking change.
    fn get_milestone_count(env: Env, agreement_id: u128) -> u32;

    /// Hook called when a milestone expires without being approved, claimed, or
    /// rejected.
    ///
    /// # Stability: `@stable-default` (since interface version 1)
    ///
    /// @notice  Called by `stello_pay_contract::expire_milestone` after the
    ///          expiry flag has been durably persisted and the
    ///          `MilestoneExpiredEvent` has been emitted.
    /// @dev     Default body is a no-op.  Override only if your contract needs
    ///          to react to expiry events.  A panic here rolls back the entire
    ///          `expire_milestone` transaction.
    /// @param   agreement_id  The agreement that contains the expired milestone.
    /// @param   milestone_id  The 1-based identifier of the expired milestone.
    ///
    /// # Semantics
    ///
    /// This method is invoked by the payroll contract's `expire_milestone`
    /// entry-point after it has:
    ///
    /// 1. Verified that the milestone is eligible for expiry (not already
    ///    approved, claimed, rejected, or previously expired).
    /// 2. Persisted the expiry flag (`MilestoneKey::MilestoneExpired`) to
    ///    durable storage.
    /// 3. Emitted the `MilestoneExpiredEvent` for off-chain indexers.
    ///
    /// Implementors may use this hook to trigger additional on-chain reactions
    /// such as releasing escrowed funds back to the employer, notifying a
    /// governance contract, or recording an audit entry.
    ///
    /// # Default implementation
    ///
    /// The default body is a no-op (`{}`), so existing implementors that do
    /// not override this method will continue to compile and run without
    /// change.  This is the defining property of an **additive** change:
    /// the new method is introduced with a default, so no existing `impl`
    /// block breaks.
    ///
    /// # Arguments
    ///
    /// * `env`          – Contract environment provided by the Soroban host.
    /// * `agreement_id` – The milestone agreement that contains the expired
    ///   milestone.
    /// * `milestone_id` – The 1-based identifier of the expired milestone
    ///   within `agreement_id`.
    ///
    /// # Panics
    ///
    /// The default implementation never panics.  Custom implementations should
    /// avoid panicking, as a panic here rolls back the entire
    /// `expire_milestone` transaction in the calling contract.
    ///
    /// # Backward-compatibility guarantee
    /// - The method **signature** (`env`, `agreement_id`, `milestone_id`) is
    ///   frozen for the lifetime of interface version 1.
    /// - The **default body** (no-op) may be adjusted in minor versions as long
    ///   as the observable no-op contract (no state mutation, no events, no
    ///   panics) is preserved.
    /// - Upgrading from "no override" to "custom override" is always safe.
    /// - Removing an override (reverting to the default) is always safe.
    fn on_milestone_expired(_env: Env, _agreement_id: u128, _milestone_id: u32) {
        // no-op default — existing implementors are unaffected
    }
}
