use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, Serialize, Deserialize)]
pub struct Employee {
    pub address: String,
    pub name: String,
    pub email: String,
    pub department: String,
    pub salary: i128,
    pub currency: String,
    pub frequency: String,
    pub start_date: DateTime<Utc>,
}

pub fn format_stellar_amount(amount: i128) -> String {
    // Stellar uses 7 decimal places
    let decimal_amount = amount as f64 / 10_000_000.0;
    format!("{:.7}", decimal_amount)
}

pub fn parse_stellar_amount(amount_str: &str) -> Result<i128> {
    let amount: f64 = amount_str.parse()?;
    Ok((amount * 10_000_000.0) as i128)
}

pub fn validate_stellar_address(address: &str) -> bool {
    // Basic validation - Stellar addresses start with 'G' and are 56 characters long
    address.starts_with('G') && address.len() == 56
}

pub fn validate_contract_address(address: &str) -> bool {
    // Basic validation - Contract addresses start with 'C' and are 56 characters long
    address.starts_with('C') && address.len() == 56
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActivityMetrics {
    pub transactions: u64,
    pub volume: HashMap<String, i128>,
    pub unique_users: u64,
    pub errors: u64,
}

pub fn format_amount(amount: i128, decimals: u32) -> String {
    let divisor = 10_i128.pow(decimals);
    let whole = amount / divisor;
    let fractional = amount % divisor;

    if fractional == 0 {
        whole.to_string()
    } else {
        // Format with full precision, then remove trailing zeros
        let formatted = format!(
            "{}.{:0width$}",
            whole,
            fractional,
            width = decimals as usize
        );
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

pub fn parse_amount(amount_str: &str, decimals: u32) -> Result<i128> {
    let parts: Vec<&str> = amount_str.split('.').collect();

    match parts.len() {
        1 => {
            // No decimal point, treat as whole number
            let whole: i128 = parts[0].parse()?;
            Ok(whole * 10_i128.pow(decimals))
        }
        2 => {
            // Has decimal point
            let whole: i128 = parts[0].parse()?;
            let fractional_str = parts[1];

            if fractional_str.len() > decimals as usize {
                return Err(anyhow::anyhow!("Too many decimal places"));
            }

            let fractional: i128 = fractional_str.parse()?;
            let fractional_scaled =
                fractional * 10_i128.pow(decimals - fractional_str.len() as u32);

            Ok(whole * 10_i128.pow(decimals) + fractional_scaled)
        }
        _ => Err(anyhow::anyhow!("Invalid amount format")),
    }
}

pub fn format_duration(seconds: u64) -> String {
    let days = seconds / (24 * 60 * 60);
    let hours = (seconds % (24 * 60 * 60)) / (60 * 60);
    let minutes = (seconds % (60 * 60)) / 60;
    let secs = seconds % 60;

    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, minutes, secs)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

pub fn parse_duration(duration_str: &str) -> Result<u64> {
    let duration_str = duration_str.to_lowercase();

    if duration_str.ends_with("s") {
        let num_str = &duration_str[..duration_str.len() - 1];
        let seconds: u64 = num_str.parse()?;
        Ok(seconds)
    } else if duration_str.ends_with("m") {
        let num_str = &duration_str[..duration_str.len() - 1];
        let minutes: u64 = num_str.parse()?;
        Ok(minutes * 60)
    } else if duration_str.ends_with("h") {
        let num_str = &duration_str[..duration_str.len() - 1];
        let hours: u64 = num_str.parse()?;
        Ok(hours * 60 * 60)
    } else if duration_str.ends_with("d") {
        let num_str = &duration_str[..duration_str.len() - 1];
        let days: u64 = num_str.parse()?;
        Ok(days * 24 * 60 * 60)
    } else {
        // Try to parse as seconds
        let seconds: u64 = duration_str.parse()?;
        Ok(seconds)
    }
}

pub fn validate_address(address: &str) -> Result<()> {
    if address.is_empty() {
        return Err(anyhow::anyhow!("Address cannot be empty"));
    }

    if address.len() != 56 {
        return Err(anyhow::anyhow!("Address must be 56 characters long"));
    }

    if !address
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err(anyhow::anyhow!(
            "Address must contain only uppercase letters and digits"
        ));
    }

    Ok(())
}

pub fn truncate_address(address: &str, chars: usize) -> String {
    if address.len() <= chars * 2 {
        return address.to_string();
    }

    format!(
        "{}...{}",
        &address[..chars],
        &address[address.len() - chars..]
    )
}

pub fn format_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "No data to display".to_string();
    }

    // Calculate column widths
    let mut widths = headers.iter().map(|h| h.len()).collect::<Vec<_>>();

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    let mut result = String::new();

    // Header
    result.push_str("┌");
    for (i, width) in widths.iter().enumerate() {
        result.push_str(&"─".repeat(width + 2));
        if i < widths.len() - 1 {
            result.push_str("┬");
        }
    }
    result.push_str("┐\n");

    // Header row
    result.push_str("│");
    for (i, (header, width)) in headers.iter().zip(widths.iter()).enumerate() {
        result.push_str(&format!(" {:<width$} ", header, width = width));
        if i < widths.len() - 1 {
            result.push_str("│");
        }
    }
    result.push_str("│\n");

    // Header separator
    result.push_str("├");
    for (i, width) in widths.iter().enumerate() {
        result.push_str(&"─".repeat(width + 2));
        if i < widths.len() - 1 {
            result.push_str("┼");
        }
    }
    result.push_str("┤\n");

    // Data rows
    for row in rows {
        result.push_str("│");
        for (i, (cell, width)) in row.iter().zip(widths.iter()).enumerate() {
            result.push_str(&format!(" {:<width$} ", cell, width = width));
            if i < widths.len() - 1 {
                result.push_str("│");
            }
        }
        result.push_str("│\n");
    }

    // Bottom border
    result.push_str("└");
    for (i, width) in widths.iter().enumerate() {
        result.push_str(&"─".repeat(width + 2));
        if i < widths.len() - 1 {
            result.push_str("┴");
        }
    }
    result.push_str("┘");

    result
}

pub fn colorize_status(status: &str) -> String {
    match status.to_lowercase().as_str() {
        "active" | "success" | "paid" | "healthy" => {
            format!("\x1b[32m{}\x1b[0m", status) // Green
        }
        "inactive" | "failed" | "error" | "unhealthy" => {
            format!("\x1b[31m{}\x1b[0m", status) // Red
        }
        "pending" | "processing" | "warning" => {
            format!("\x1b[33m{}\x1b[0m", status) // Yellow
        }
        "paused" | "disabled" => {
            format!("\x1b[90m{}\x1b[0m", status) // Gray
        }
        _ => status.to_string(),
    }
}

/// Default relative path to the release WASM artifact produced by a contract build.
pub const DEFAULT_WASM_PATH: &str =
    "../../onchain/target/wasm32v1-none/release/stello_pay_contract.wasm";

/// Outcome of comparing a local source WASM hash against a deployed hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Byte-for-byte SHA-256 hashes match.
    Match { hash: String },
    /// Hashes differ — source has drifted from the on-chain WASM.
    Mismatch {
        local_hash: String,
        deployed_hash: String,
    },
}

/// Compute the SHA-256 hex digest of a WASM file's raw bytes.
///
/// Verification must hash the actual WASM payload (not a version string) so
/// silent source/deployment drift is caught.
pub fn compute_wasm_hash(wasm_path: &Path) -> Result<String> {
    let bytes = std::fs::read(wasm_path).map_err(|e| {
        anyhow::anyhow!(
            "failed to read WASM file '{}': {}",
            wasm_path.display(),
            e
        )
    })?;
    if bytes.is_empty() {
        return Err(anyhow::anyhow!(
            "WASM file '{}' is empty",
            wasm_path.display()
        ));
    }
    Ok(sha256_hex(&bytes))
}

fn sha256_hex(data: &[u8]) -> String {
    hex_encode(&sha256(data))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Minimal SHA-256 (FIPS 180-4) so the CLI stays free of extra crypto crates.
fn sha256(message: &[u8]) -> [u8; 32] {
    // Initial hash values
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (message.len() as u64).saturating_mul(8);
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let j = i * 4;
            w[i] = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
    }
    out
}

/// Normalize a hex hash string for comparison (lowercase, strip optional `0x`).
pub fn normalize_hash(hash: &str) -> String {
    let trimmed = hash.trim();
    let without_prefix = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    without_prefix.to_ascii_lowercase()
}

/// Compare local and deployed WASM hashes byte-for-byte (via normalized hex).
pub fn compare_wasm_hashes(local_hash: &str, deployed_hash: &str) -> VerifyOutcome {
    let local = normalize_hash(local_hash);
    let deployed = normalize_hash(deployed_hash);
    if local == deployed {
        VerifyOutcome::Match { hash: local }
    } else {
        VerifyOutcome::Mismatch {
            local_hash: local,
            deployed_hash: deployed,
        }
    }
}

/// Human-readable pass/fail message for a verification outcome.
pub fn format_verify_message(outcome: &VerifyOutcome) -> String {
    match outcome {
        VerifyOutcome::Match { hash } => {
            format!(
                "✅ Verification passed: local and deployed WASM hashes match ({})",
                hash
            )
        }
        VerifyOutcome::Mismatch {
            local_hash,
            deployed_hash,
        } => {
            format!(
                "❌ Verification failed: deployed WASM hash mismatch (drift detected)\n  local:    {}\n  deployed: {}",
                local_hash, deployed_hash
            )
        }
    }
}

/// Build the StellopayCore contract via the Soroban CLI and return the WASM path.
pub fn build_contract_wasm() -> Result<PathBuf> {
    info_or_print_build("Building contract from source...");

    let soroban_check = std::process::Command::new("soroban")
        .arg("--version")
        .output();
    if soroban_check.is_err() {
        return Err(anyhow::anyhow!(
            "Soroban CLI not found. Install with: cargo install --locked soroban-cli"
        ));
    }

    let contract_dir = PathBuf::from("../../onchain/contracts/stello_pay_contract");
    let output = if contract_dir.exists() {
        std::process::Command::new("soroban")
            .args(["contract", "build"])
            .current_dir(&contract_dir)
            .output()?
    } else {
        // Fall back to building from the current directory.
        std::process::Command::new("soroban")
            .args(["contract", "build"])
            .output()?
    };

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Contract build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let wasm_path = PathBuf::from(DEFAULT_WASM_PATH);
    if !wasm_path.exists() {
        return Err(anyhow::anyhow!(
            "Build succeeded but WASM not found at {}",
            wasm_path.display()
        ));
    }
    Ok(wasm_path)
}

fn info_or_print_build(msg: &str) {
    println!("{}", msg);
}

/// Fetch the deployed contract WASM via Soroban CLI and return its SHA-256 hash.
pub fn fetch_deployed_wasm_hash(
    contract_id: &str,
    rpc_url: &str,
    network: &str,
) -> Result<String> {
    let temp_dir = std::env::temp_dir();
    let out_file = temp_dir.join(format!("stellopay-verify-{}.wasm", contract_id));

    let output = std::process::Command::new("soroban")
        .args([
            "contract",
            "fetch",
            "--id",
            contract_id,
            "--rpc-url",
            rpc_url,
            "--network",
            network,
            "--out-file",
            out_file.to_str().unwrap_or("stellopay-verify.wasm"),
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run soroban contract fetch: {}", e))?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "failed to fetch deployed contract WASM: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let hash = compute_wasm_hash(&out_file)?;
    let _ = std::fs::remove_file(&out_file);
    Ok(hash)
}

#[cfg(test)]
mod verify_hash_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn sha256_matches_known_test_vector() {
        // FIPS 180-4 / NIST example: SHA-256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn compute_wasm_hash_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.wasm");
        std::fs::write(&path, b"\0asm\x01\x00\x00\x00fake-wasm").unwrap();
        let a = compute_wasm_hash(&path).unwrap();
        let b = compute_wasm_hash(&path).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn compare_matching_hashes() {
        let outcome = compare_wasm_hashes(
            "AaBbCcDdEeFf00112233445566778899aabbccddeeff00112233445566778899",
            "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899",
        );
        assert!(matches!(outcome, VerifyOutcome::Match { .. }));
    }

    #[test]
    fn compare_mismatching_hashes_reports_both() {
        let local = "aa".repeat(32);
        let deployed = "bb".repeat(32);
        let outcome = compare_wasm_hashes(&local, &deployed);
        match outcome {
            VerifyOutcome::Mismatch {
                local_hash,
                deployed_hash,
            } => {
                assert_eq!(local_hash, local);
                assert_eq!(deployed_hash, deployed);
            }
            other => panic!("expected mismatch, got {other:?}"),
        }
    }

    #[test]
    fn format_verify_message_includes_both_hashes_on_fail() {
        let outcome = VerifyOutcome::Mismatch {
            local_hash: "localhash".into(),
            deployed_hash: "deployedhash".into(),
        };
        let msg = format_verify_message(&outcome);
        assert!(msg.contains("localhash"));
        assert!(msg.contains("deployedhash"));
        assert!(msg.contains("mismatch") || msg.contains("failed"));
    }

    #[test]
    fn different_bytes_produce_different_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.wasm");
        let b = dir.path().join("b.wasm");
        let mut fa = std::fs::File::create(&a).unwrap();
        fa.write_all(b"wasm-content-a").unwrap();
        let mut fb = std::fs::File::create(&b).unwrap();
        fb.write_all(b"wasm-content-b").unwrap();
        assert_ne!(
            compute_wasm_hash(&a).unwrap(),
            compute_wasm_hash(&b).unwrap()
        );
    }
}

pub fn format_percentage(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

pub fn format_gas(gas: u64) -> String {
    if gas >= 1_000_000 {
        format!("{:.1}M", gas as f64 / 1_000_000.0)
    } else if gas >= 1_000 {
        format!("{:.1}K", gas as f64 / 1_000.0)
    } else {
        gas.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_amount() {
        assert_eq!(format_amount(1000000000, 7), "100");
        assert_eq!(format_amount(1500000000, 7), "150");
        assert_eq!(format_amount(1234567890, 7), "123.456789");
    }

    #[test]
    fn test_parse_amount() {
        assert_eq!(parse_amount("100", 7).unwrap(), 1000000000);
        assert_eq!(parse_amount("150.5", 7).unwrap(), 1505000000);
        assert_eq!(parse_amount("123.456789", 7).unwrap(), 1234567890);
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), 30);
        assert_eq!(parse_duration("5m").unwrap(), 300);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        assert_eq!(parse_duration("1d").unwrap(), 86400);
    }

    #[test]
    fn test_validate_address() {
        assert!(
            validate_address("GCKFBEIYTKP6RCZEKMGL2QAPLGKUBGE5UAHRQJRXGCQHKPQM6CHCM4K4").is_ok()
        );
        assert!(validate_address("invalid").is_err());
        assert!(validate_address("").is_err());
    }

    #[test]
    fn test_truncate_address() {
        let addr = "GCKFBEIYTKP6RCZEKMGL2QAPLGKUBGE5UAHRQJRXGCQHKPQM6CHCM4K4";
        assert_eq!(truncate_address(addr, 4), "GCKF...M4K4");
        assert_eq!(truncate_address("SHORT", 4), "SHORT");
    }
}
/// Retry policy for read-only RPC (`query`) calls made by the CLI.
///
/// This is intentionally distinct from the per-webhook [`RetryConfig`] the
/// contract returns: it governs client-side retries of transient network
/// errors on *idempotent reads only*. State-changing `invoke` calls must
/// never be retried (see `commands.rs`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of attempts (including the first). `0` disables retries.
    #[serde(default)]
    pub max_attempts: u32,
    /// Base backoff between attempts, in milliseconds. Doubled each attempt
    /// (exponential) up to `max_delay_ms`.
    #[serde(default)]
    pub base_delay_ms: u64,
    /// Upper bound on a single backoff interval, in milliseconds.
    #[serde(default)]
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 200,
            max_delay_ms: 5_000,
        }
    }
}

/// Run an async operation with exponential backoff.
///
/// `op` is a factory that produces a *new* future on every attempt
/// (`FnMut`), so a future is never polled twice. On error the attempt is
/// retried until `policy.max_attempts` is reached; the wait between attempts
/// doubles (`base_delay_ms * 2^(attempt-1)`), capped at `max_delay_ms`.
///
/// Only use this for idempotent, read-only operations. Retrying a
/// state-changing call without idempotency protection could double-submit a
/// transaction.
pub async fn with_retry<F, Fut, T, E>(policy: RetryPolicy, mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    if policy.max_attempts == 0 {
        return op()
            .await
            .map_err(|e| anyhow::anyhow!("RPC call failed: {}", e));
    }

    let mut attempt: u32 = 0;
    let mut last_msg: String = String::new();
    loop {
        attempt += 1;
        match op().await {
            Ok(value) => return Ok(value),
            Err(e) => {
                last_msg = e.to_string();
                if attempt >= policy.max_attempts {
                    return Err(anyhow::anyhow!(
                        "RPC query failed after {} attempt(s): {}",
                        attempt,
                        last_msg
                    ));
                }
                let backoff = (policy.base_delay_ms as u64)
                    .saturating_mul(1u64 << (attempt.saturating_sub(1)))
                    .min(policy.max_delay_ms);
                sleep(Duration::from_millis(backoff)).await;
            }
        }
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::cell::Cell;

    #[tokio::test]
    async fn transient_failure_then_success() {
        let calls = Cell::new(0u32);
        let policy = RetryPolicy {
            max_attempts: 3,
            base_delay_ms: 0,
            max_delay_ms: 0,
        };
        let result = with_retry(policy, || {
            let n = calls.get();
            calls.set(n + 1);
            async move {
                if n < 2 {
                    Err::<u32, _>(anyhow::anyhow!("transient error {}", n))
                } else {
                    Ok(42u32)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.get(), 3, "should retry until success");
    }

    #[tokio::test]
    async fn exhausted_retries_surfaces_error() {
        let calls = Cell::new(0u32);
        let policy = RetryPolicy {
            max_attempts: 3,
            base_delay_ms: 0,
            max_delay_ms: 0,
        };
        let err = with_retry(policy, || {
            let n = calls.get();
            calls.set(n + 1);
            async move { Err::<(), _>(anyhow::anyhow!("always fails")) }
        })
        .await;
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("after 3 attempt"),
            "error should cite attempt count: {msg}"
        );
        assert_eq!(calls.get(), 3, "should attempt exactly max_attempts times");
    }

    #[tokio::test]
    async fn first_attempt_success_does_not_retry() {
        let calls = Cell::new(0u32);
        let policy = RetryPolicy {
            max_attempts: 5,
            base_delay_ms: 0,
            max_delay_ms: 0,
        };
        let value = with_retry(policy, || {
            let n = calls.get();
            calls.set(n + 1);
            async move { Ok::<u32, anyhow::Error>(7) }
        })
        .await
        .unwrap();
        assert_eq!(value, 7);
        assert_eq!(calls.get(), 1, "success on first try must not retry");
    }

    #[tokio::test]
    async fn zero_max_attempts_disables_retry() {
        let calls = Cell::new(0u32);
        let policy = RetryPolicy {
            max_attempts: 0,
            base_delay_ms: 100,
            max_delay_ms: 1000,
        };
        let err = with_retry(policy, || {
            let n = calls.get();
            calls.set(n + 1);
            async move { Err::<(), _>(anyhow::anyhow!("boom")) }
        })
        .await;
        assert!(err.is_err());
        assert_eq!(calls.get(), 1, "max_attempts=0 must still run once");
    }
}

/// Retry policy attached to a webhook, as returned by `register_webhook` / `get_webhook`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub retry_delay: u64,
    pub exponential_backoff: bool,
    pub max_delay: u64,
}

/// Delivery security policy attached to a webhook.
///
/// This deliberately excludes the webhook secret: read responses must never
/// echo back signing material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub signature_method: String,
    pub rate_limit_per_minute: u32,
    pub require_tls: bool,
}

/// Typed view of a single webhook, as returned by the contract's `get_webhook` query.
///
/// All fields are optional so that this type tolerates contract responses
/// that omit fields not yet populated (e.g. a webhook with no retry policy
/// configured), without failing deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookInfo {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub is_active: Option<bool>,
    pub retry_config: Option<RetryConfig>,
    pub security_config: Option<SecurityConfig>,
}

/// Typed view of aggregate webhook statistics, as returned by `get_webhook_stats`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookStats {
    pub total_webhooks: Option<u64>,
    pub active_webhooks: Option<u64>,
    pub total_deliveries: Option<u64>,
    pub failed_deliveries: Option<u64>,
}

pub struct SorobanHttpClient {
    base_url: String,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
}
impl SorobanHttpClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            client: reqwest::Client::new(),
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Override the retry policy used by read-only `query`/`query_as` calls.
    ///
    /// State-changing `invoke` calls intentionally ignore this policy.
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }
    pub async fn get_ledger_info(&self) -> Result<String> {
        let url = format!("{}/ledger", self.base_url);
        let res = self.client.get(&url).send().await?;
        let body = res.text().await?;
        Ok(body)
    }
    pub async fn invoke(
        &self,
        contract_id: &str,
        method: &str,
        args: Vec<(&str, &str)>,
        signer: &str,
    ) -> Result<String> {
        let url = format!("{}/invoke", self.base_url.trim_end_matches('/'));
        println!("Invoking Soroban at: {}", url);
        let payload = json!({
            "contract_id":contract_id,
            "method":method,
            "args":args.iter().map(|(k,v)| json!({(*k):v})).collect::<Vec<_>>(),
            "signer":signer,
        });
        let response = self.client.post(&url).json(&payload).send().await?;
        let body = response.text().await?;
        Ok(body)
    }

    /// Read-only contract call with retry/backoff on transient failure.
    ///
    /// Simulates a contract call without signing or submitting a transaction:
    /// it sends only the contract id, method, and arguments to the RPC query
    /// endpoint and never accepts a signer or secret key. The HTTP call is
    /// wrapped in [`with_retry`] using the client's [`RetryPolicy`]. Because
    /// `query` is idempotent it is always safe to retry; the state-changing
    /// [`SorobanHttpClient::invoke`] is intentionally *not* retried.
    pub async fn query(
        &self,
        contract_id: &str,
        method: &str,
        args: Vec<(&str, &str)>,
    ) -> Result<Value> {
        let policy = self.retry_policy;
        with_retry(policy, || {
            let a = args.clone();
            async move {
                let url = format!("{}/query", self.base_url);
                let payload = self.query_payload(contract_id, method, a);
                let response = self.client.post(&url).json(&payload).send().await?;
                let status = response.status();
                let body = response.text().await?;

                if !status.is_success() {
                    return Err(anyhow::anyhow!(
                        "Soroban query failed with status {}: {}",
                        status,
                        body
                    ));
                }

                let value: Value = serde_json::from_str(&body)
                    .map_err(|e| anyhow::anyhow!("Malformed Soroban query response: {}", e))?;

                if let Some(error) = value.get("error") {
                    return Err(anyhow::anyhow!("Soroban query RPC error: {}", error));
                }

                Ok(value.get("result").cloned().unwrap_or(value))
            }
        })
        .await
    }

    /// Same read-only query as [`SorobanHttpClient::query`], but deserialized
    /// into a typed result `T` instead of a raw [`Value`].
    ///
    /// Prefer this over `query` when the shape of the contract method's
    /// return value is known (e.g. [`WebhookInfo`], [`WebhookStats`]), so
    /// callers get compile-time field access instead of indexing into JSON.
    pub async fn query_as<T: serde::de::DeserializeOwned>(
        &self,
        contract_id: &str,
        method: &str,
        args: Vec<(&str, &str)>,
    ) -> Result<T> {
        let value = self.query(contract_id, method, args).await?;
        serde_json::from_value(value)
            .map_err(|e| anyhow::anyhow!("Soroban query result did not match expected shape: {}", e))
    }

    fn query_payload(&self, contract_id: &str, method: &str, args: Vec<(&str, &str)>) -> Value {
        json!({
            "contract_id": contract_id,
            "method": method,
            "args": args.iter().map(|(k, v)| json!({ (*k): v })).collect::<Vec<_>>(),
            "read_only": true,
        })
    }
}
