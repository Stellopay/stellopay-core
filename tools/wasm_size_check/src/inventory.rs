//! WASM artifact inventory.
//!
//! Scans the directory produced by
//! `cargo build --release --target wasm32-unknown-unknown` and produces
//! one [`Measurement`] per `.wasm` file, keyed by file stem (no
//! extension). The result is a `BTreeMap` so reports are deterministically
//! ordered without needing to sort at display time.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};

/// One compiled `.wasm` artifact as observed on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// File stem (`"stello_pay_contract"` for `stello_pay_contract.wasm`).
    pub name: String,

    /// Compiled size in bytes.
    pub size_bytes: u64,

    /// SHA-256 of the artifact, formatted as `sha256:<lowercase hex>`.
    /// Matches the form stored in the baseline.
    pub sha256: String,
}

/// Walk `dir` and return a sorted, deduplicated map of contract name →
/// [`Measurement`].
///
/// The traversal is shallow-but-recursive: any `.wasm` file under
/// `dir` (including nested release subdirectories) is picked up. This
/// matches Cargo's layout — `target/wasm32-unknown-unknown/release/*.wasm`.
///
/// If a contract name appears more than once (duplicate `.wasm`
/// files), the first one encountered wins so the result is
/// deterministic regardless of filesystem ordering.
///
/// # Errors
/// Returns [`Error::WasmDirRead`] if the directory is missing, not
/// readable, or if any individual `.wasm` cannot be read.
pub fn scan(dir: &Path) -> Result<BTreeMap<String, Measurement>> {
    if !dir.exists() {
        return Err(Error::WasmDirRead {
            path: dir.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "directory does not exist"),
        });
    }

    let mut out: BTreeMap<String, Measurement> = BTreeMap::new();
    // `sort_by_file_name` gives us a deterministic, alphabetical
    // ordering across filesystems (std::fs::read_dir does not
    // guarantee any ordering). Without this, the "first hit wins"
    // tie-break below would be platform-dependent and could silently
    // pick a different artifact for the same contract name on Linux
    // vs. macOS, breaking size-regression reviews.
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(|e| Error::WasmDirRead {
            path: e.path().map(|p| p.to_path_buf()).unwrap_or_else(|| dir.to_path_buf()),
            source: std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("walk error: {e}"),
            ),
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("wasm") {
            continue;
        }
        let stem = match entry.path().file_stem().and_then(|s| s.to_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        if out.contains_key(&stem) {
            // First hit wins; skip subsequent bytes to keep behaviour
            // deterministic regardless of filesystem ordering.
            continue;
        }
        let bytes = fs::read(entry.path()).map_err(|source| Error::WasmDirRead {
            path: entry.path().to_path_buf(),
            source,
        })?;
        let size_bytes = bytes.len() as u64;
        let sha256 = sha256_hex(&bytes);
        out.insert(
            stem.clone(),
            Measurement {
                name: stem,
                size_bytes,
                sha256,
            },
        );
    }
    Ok(out)
}

/// Compute the `sha256:<hex>` fingerprint of a byte slice.
///
/// Exposed (rather than being `pub(crate)`) because it is a useful
/// primitive for unit tests and for downstream tooling that wants the
/// same format as the baseline file.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(7 + digest.len() * 2);
    out.push_str("sha256:");
    for byte in digest {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
