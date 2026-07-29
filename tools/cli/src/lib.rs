pub mod commands;
pub mod config;
pub mod utils;

pub use config::{
    create_config_file, get_secret_key, load_config, load_project_config,
    resolve_config, resolve_config_with_project_file, FileConfig, PROJECT_CONFIG_FILE,
};

use crate::utils::RetryPolicy;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "stellopay-cli")]
#[command(about = "CLI tool for StellopayCore contract management")]
#[command(version = "0.1.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Configuration file path
    #[arg(short, long, default_value = "~/.stellopay/config.toml")]
    pub config: PathBuf,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    #[arg(long, short = 'y', global = true)]
    pub yes: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Deploy a new contract
    Deploy {
        /// Network to deploy to
        #[arg(long, default_value = "testnet")]
        network: String,

        /// Owner address
        #[arg(long)]
        owner: String,

        /// WASM file path
        #[arg(long)]
        wasm: Option<PathBuf>,
    },

    /// Get contract information
    Info {
        /// Contract ID to inspect
        #[arg(long)]
        contract_id: Option<String>,
    },
    /// Show CLI status
    Status,
    /// Emergency Command
    EmergencyWithdraw {
        #[arg(long)]
        contract_id: Option<String>,
        #[arg(long)]
        token: String,
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        amount: i128,
    },
    /// Webhook management commands
    Webhook {
        #[command(subcommand)]
        command: WebhookCommands,
    },

    /// Verify that a deployed contract's on-chain WASM hash matches a fresh
    /// build from the current source tree (byte-for-byte SHA-256 comparison).
    Verify {
        /// Contract ID to verify against (falls back to config default)
        #[arg(long)]
        contract_id: Option<String>,

        /// Network to query (testnet, mainnet)
        #[arg(long, default_value = "testnet")]
        network: String,

        /// Local WASM file path (skips build when provided)
        #[arg(long)]
        wasm: Option<PathBuf>,

        /// Skip rebuilding the contract from source
        #[arg(long)]
        skip_build: bool,

        /// Optional deployed WASM hash override (hex). When set, skips the
        /// on-chain fetch — useful for offline/CI checks and tests.
        #[arg(long)]
        deployed_hash: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum WebhookCommands {
    /// Register a new webhook
    Register {
        /// Webhook name
        #[arg(long)]
        name: String,
        /// Webhook description
        #[arg(long)]
        description: String,
        /// Webhook URL
        #[arg(long)]
        url: String,
        /// Events to subscribe to (comma-separated)
        #[arg(long)]
        events: String,
        /// Webhook secret
        #[arg(long)]
        secret: String,
        /// Contract ID
        #[arg(long)]
        contract_id: Option<String>,
    },
    /// Update an existing webhook
    Update {
        /// Webhook ID
        #[arg(long)]
        webhook_id: u64,
        /// New webhook name
        #[arg(long)]
        name: Option<String>,
        /// New webhook description
        #[arg(long)]
        description: Option<String>,
        /// New webhook URL
        #[arg(long)]
        url: Option<String>,
        /// New events to subscribe to (comma-separated)
        #[arg(long)]
        events: Option<String>,
        /// Activate/deactivate webhook
        #[arg(long)]
        active: Option<bool>,
        /// Contract ID
        #[arg(long)]
        contract_id: Option<String>,
    },
    /// Delete a webhook
    Delete {
        /// Webhook ID
        #[arg(long)]
        webhook_id: u64,
        /// Contract ID
        #[arg(long)]
        contract_id: Option<String>,
    },
    /// List webhooks for an owner
    List {
        /// Owner address
        #[arg(long)]
        owner: String,
        /// Contract ID
        #[arg(long)]
        contract_id: Option<String>,
    },
    /// Get webhook information
    Get {
        /// Webhook ID
        #[arg(long)]
        webhook_id: u64,
        /// Contract ID
        #[arg(long)]
        contract_id: Option<String>,
    },
    /// Get webhook statistics
    Stats {
        /// Contract ID
        #[arg(long)]
        contract_id: Option<String>,
    },
    /// Test webhook delivery
    Test {
        /// Webhook ID
        #[arg(long)]
        webhook_id: u64,
        /// Event type to test
        #[arg(long)]
        event_type: String,
        /// Contract ID
        #[arg(long)]
        contract_id: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub network: NetworkConfig,
    pub contract: ContractConfig,
    pub auth: AuthConfig,
    pub defaults: DefaultsConfig,
    /// Retry policy for read-only (`query`) RPC calls. Defaults are applied
    /// automatically when the key is absent from a TOML config, so existing
    /// config files keep loading.
    #[serde(default)]
    pub retry: RetryPolicy,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub rpc_url: String,
    pub network_passphrase: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContractConfig {
    pub default_contract_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthConfig {
    pub secret_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DefaultsConfig {
    pub token: Option<String>,
    pub frequency: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PayrollInfo {
    pub employee: String,
    pub employer: String,
    pub token: String,
    pub amount: i128,
    pub frequency: u64,
    pub next_payment: u64,
    pub last_payment: u64,
    pub active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaymentHistory {
    pub employee: String,
    pub employer: String,
    pub token: String,
    pub amount: i128,
    pub timestamp: u64,
    pub transaction_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContractStatus {
    pub contract_id: String,
    pub owner: Option<String>,
    pub is_paused: bool,
    pub supported_tokens: Vec<String>,
    pub active_payrolls: u32,
    pub total_locked_value: HashMap<String, i128>,
    pub last_activity: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthMetrics {
    pub is_healthy: bool,
    pub response_time: u64,
    pub error_rate: f64,
    pub success_rate: f64,
    pub last_check: u64,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub average_response_time: u64,
    pub p95_response_time: u64,
    pub p99_response_time: u64,
    pub throughput: f64,
    pub error_rate: f64,
    pub gas_usage: GasMetrics,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GasMetrics {
    pub average: u64,
    pub median: u64,
    pub p95: u64,
    pub p99: u64,
    pub total: u64,
}
//error enum
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Zero amount is not allowed")]
    ZeroAmount,
    #[error("Maximum Amount Surpassed")]
    MaximumAmount,
    #[error("Invalid Address")]
    InvalidAddress,
    #[error("Missing secret key")]
    MissingSecretKey,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
// Helper functions for frequency conversion
pub fn frequency_to_seconds(frequency: &str) -> Result<u64, String> {
    match frequency.to_lowercase().as_str() {
        "weekly" => Ok(7 * 24 * 60 * 60),
        "biweekly" => Ok(14 * 24 * 60 * 60),
        "monthly" => Ok(30 * 24 * 60 * 60),
        "quarterly" => Ok(90 * 24 * 60 * 60),
        "annually" => Ok(365 * 24 * 60 * 60),
        _ => Err(format!("Invalid frequency: {}", frequency)),
    }
}

pub fn seconds_to_frequency(seconds: u64) -> String {
    match seconds {
        604800 => "weekly".to_string(),
        1209600 => "biweekly".to_string(),
        2592000 => "monthly".to_string(),
        7776000 => "quarterly".to_string(),
        31536000 => "annually".to_string(),
        _ => format!("{} seconds", seconds),
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: NetworkConfig {
                rpc_url: "https://soroban-testnet.stellar.org:443".to_string(),
                network_passphrase: "Test SDF Network ; September 2015".to_string(),
            },
            contract: ContractConfig {
                default_contract_id: None,
            },
            auth: AuthConfig { secret_key: None },
            defaults: DefaultsConfig {
                token: None,
                frequency: "monthly".to_string(),
            },
            retry: RetryPolicy::default(),
        }
    }
}
//admin and pause checks
pub fn require_admin(_context: &str) -> Result<(), Error> {
    //dummy implementation
    Ok(())
}
pub fn require_not_paused(_context: &str) -> Result<(), Error> {
    //dummy implementation
    Ok(())
}
//token client
pub struct TokenClient;
impl TokenClient {
    pub fn new(_rpc_url: &str, _token_address: &str) -> Self {
        TokenClient
    }
    pub fn transfer(&self, _to: &str, _amount: i128) -> Result<(), Error> {
        //dummy implementation
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CLI exit codes
// ---------------------------------------------------------------------------

/// Process exit codes emitted by the CLI, grouped by failure category.
///
/// Scripts and CI wrappers can branch on these to tell a usage mistake apart
/// from a transient network failure. See `tools/cli/EXIT_CODES.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Successful execution.
    Success = 0,
    /// Unspecified / generic failure (default catch-all).
    Generic = 1,
    /// Command-line usage error (bad flags, unknown subcommand).
    Usage = 2,
    /// Configuration error (missing / unreadable / invalid config).
    Config = 3,
    /// Network / RPC failure talking to a Soroban endpoint.
    Network = 4,
    /// Verification failure (e.g. deployed WASM hash mismatch).
    Verification = 5,
}

impl ExitCode {
    /// Numeric value to hand to [`std::process::exit`].
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Map an error to a stable [`ExitCode`] category.
///
/// CLI errors are surfaced as `anyhow::Error`; we classify heuristically by
/// walking the cause chain and matching known keywords so callers/scripts can
/// react differently per failure kind. Clap itself emits `Usage` (2) for
/// argument-parse errors before this handler is ever reached.
pub fn classify_error(e: &anyhow::Error) -> ExitCode {
    let mut msg = e.to_string();
    for cause in e.chain().skip(1) {
        msg.push(' ');
        msg.push_str(&cause.to_string());
    }
    let m = msg.to_lowercase();
    if m.contains("verif") || m.contains("hash mismatch") || m.contains("drift") {
        ExitCode::Verification
    } else if m.contains("config") || m.contains("toml") || m.contains("config file") {
        ExitCode::Config
    } else if m.contains("network")
        || m.contains("rpc")
        || m.contains("soroban")
        || m.contains("connection")
        || m.contains("timeout")
        || m.contains("http")
    {
        ExitCode::Network
    } else {
        ExitCode::Generic
    }
}

#[cfg(test)]
mod exit_code_tests {
    use super::*;

    #[test]
    fn exit_code_values_are_stable() {
        assert_eq!(ExitCode::Success.as_u8(), 0);
        assert_eq!(ExitCode::Generic.as_u8(), 1);
        assert_eq!(ExitCode::Usage.as_u8(), 2);
        assert_eq!(ExitCode::Config.as_u8(), 3);
        assert_eq!(ExitCode::Network.as_u8(), 4);
        assert_eq!(ExitCode::Verification.as_u8(), 5);
    }

    #[test]
    fn classifies_config_errors() {
        let e = anyhow::anyhow!("could not read config file: invalid toml");
        assert_eq!(classify_error(&e), ExitCode::Config);
    }

    #[test]
    fn classifies_network_errors() {
        let e = anyhow::anyhow!("Soroban RPC error: connection refused");
        assert_eq!(classify_error(&e), ExitCode::Network);
    }

    #[test]
    fn classifies_verification_errors() {
        let e = anyhow::anyhow!("deployed WASM hash mismatch (drift detected)");
        assert_eq!(classify_error(&e), ExitCode::Verification);
    }

    #[test]
    fn falls_back_to_generic() {
        let e = anyhow::anyhow!("something unexpected happened");
        assert_eq!(classify_error(&e), ExitCode::Generic);
    }
}
