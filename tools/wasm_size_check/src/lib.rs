//! # wasm_size_check
//!
//! Pure checker for WASM binary-size regressions in StellopayCore's
//! Soroban contracts. The CI workflow is responsible for invoking
//! `cargo build --release --target wasm32-unknown-unknown` first; this
//! crate only inspects the produced `.wasm` artifacts and compares them
//! against a committed JSON baseline.
//!
//! Splitting build from check keeps build errors (compiler failures)
//! distinct from size-regression failures in CI logs and keeps the
//! checker fast, deterministic, and easy to unit-test without any
//! Soroban toolchain present.
//!
//! See `README.md` for the user-facing documentation and
//! `docs/ci.md` at the repository root for the policy.

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

pub mod baseline;
pub mod command;
pub mod error;
pub mod inventory;
pub mod report;

pub use baseline::{Baseline, BaselineEntry};
pub use command::{compare, refresh};
pub use error::{Error, Result};
pub use inventory::{scan, Measurement};
pub use report::{Comparison, ComparisonStatus, Report};
