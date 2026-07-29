//! Error type for the WASM size regression checker.

use std::path::PathBuf;
use thiserror::Error;

/// All errors emitted by `wasm_size_check`. Each variant carries enough
/// context to be actionable from a CI log line without re-running the
/// tool.
#[derive(Debug, Error)]
pub enum Error {
    /// Baseline JSON exists but could not be read from disk.
    #[error("could not read baseline file '{path}': {source}")]
    BaselineRead {
        /// Path the tool attempted to read.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// Baseline JSON exists but is not valid JSON or does not match the
    /// expected schema.
    #[error("could not parse baseline file '{path}': {source}")]
    BaselineParse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// Baseline JSON could not be written back to disk (refresh mode).
    #[error("could not serialize baseline '{path}': {source}")]
    BaselineSerialize {
        /// Path the tool attempted to write.
        path: PathBuf,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// Baseline JSON could not be flushed to disk (refresh mode).
    #[error("could not write baseline file '{path}': {source}")]
    BaselineWrite {
        /// Path the tool attempted to write.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// The WASM output directory cannot be inspected.
    #[error("could not read WASM directory '{path}': {source}")]
    WasmDirRead {
        /// Directory the tool attempted to scan.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// The user-supplied tolerance is negative, non-finite, or
    /// otherwise unusable.
    #[error("invalid tolerance percentage {0}: must be a non-negative, finite number")]
    InvalidTolerance(f64),

    /// The user opted into strict comparison-mode but did not provide
    /// a baseline file that exists. This is treated as a hard error so
    /// the operator is forced to bootstrap consciously
    /// (`--update-baseline`) instead of silently passing on an empty
    /// baseline.
    #[error(
        "baseline file '{path}' does not exist; rerun the command with --update-baseline to bootstrap it"
    )]
    BaselineMissingForCompare {
        /// Path the tool expected to read.
        path: PathBuf,
    },
}

/// Convenience alias for `Result<T, wasm_size_check::Error>`.
pub type Result<T> = std::result::Result<T, Error>;
