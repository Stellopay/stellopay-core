//! On-disk baseline format and IO for the WASM size regression
//! checker.
//!
//! The committed baseline is a JSON object with two top-level keys:
//!
//! * `version` — schema version, currently `1`. Bumping this is a
//!   breaking change to the file format and must be coordinated with a
//!   concurrent `--update-baseline` run.
//! * `captured_at` — ISO `YYYY-MM-DD` date the baseline was last
//!   refreshed, written when `--update-baseline` is invoked.
//! * `contracts` — map of contract name → [`BaselineEntry`].
//!
//! ```json
//! {
//!   "version": 1,
//!   "captured_at": "2026-07-28",
//!   "contracts": {
//!     "stello_pay_contract": {
//!       "size_bytes": 12345,
//!       "sha256": "sha256:abcdef…",
//!       "captured_at": "2026-07-28"
//!     }
//!   }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};

/// Current schema version of the on-disk baseline format.
pub const BASELINE_VERSION: u32 = 1;

/// Top-level baseline document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// Schema version of this baseline. Always written as
    /// [`BASELINE_VERSION`] when refreshed; read with a default of
    /// `1` to keep older files forward-compatible.
    #[serde(default = "default_version")]
    pub version: u32,

    /// ISO date (`YYYY-MM-DD`) when the baseline was last refreshed.
    /// Optional so v0 files without this field still load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,

    /// Per-contract records keyed by the `.wasm` file stem (which
    /// matches the Soroban contract crate name). `BTreeMap` so the
    /// on-disk JSON is deterministically ordered and diffs cleanly in
    /// code review.
    pub contracts: BTreeMap<String, BaselineEntry>,
}

fn default_version() -> u32 {
    BASELINE_VERSION
}

/// Per-contract size + hash + capture-date snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineEntry {
    /// Compiled `.wasm` size in bytes, recorded when the baseline was
    /// last refreshed.
    pub size_bytes: u64,

    /// Optional SHA-256 of the `.wasm` artifact, recorded in the
    /// `sha256:<hex>` form. Used by the checker to surface
    /// unintentionally stale baseline entries (same size, different
    /// bytes) — usually a sign the baseline was copy-pasted or the
    /// artifact was rebuilt from a different source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,

    /// ISO date (`YYYY-MM-DD`) on which this specific entry was last
    /// refreshed. Distinct from the top-level `captured_at` so that an
    /// entry added on one date and refreshed later retains its
    /// provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
}

impl Baseline {
    /// Read a baseline from disk. Treats an empty file as an empty
    /// baseline so re-running with `--update-baseline` after touching
    /// the file by hand doesn't trip the parser.
    ///
    /// # Errors
    /// Returns [`Error::BaselineRead`] on IO failure,
    /// [`Error::BaselineParse`] if the contents cannot be deserialized.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).map_err(|source| Error::BaselineRead {
            path: path.to_path_buf(),
            source,
        })?;
        if bytes.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(Self::default());
        }
        serde_json::from_slice(&bytes).map_err(|source| Error::BaselineParse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Write the baseline to disk as pretty JSON so the file diffs
    /// cleanly under code review.
    ///
    /// # Errors
    /// Returns [`Error::BaselineSerialize`] if serialization fails
    /// (shouldn't happen for valid data) or [`Error::BaselineWrite`] on
    /// IO failure.
    pub fn write(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(|source| {
            Error::BaselineSerialize {
                path: path.to_path_buf(),
                source,
            }
        })?;
        fs::write(path, format!("{json}\n")).map_err(|source| Error::BaselineWrite {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Return `true` when the baseline carries no contract entries.
    /// Used by the CLI to print a friendly hint about bootstrapping.
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }
}
