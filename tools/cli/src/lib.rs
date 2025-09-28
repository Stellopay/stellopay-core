use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
//use soroban_sdk::Env;
//use thiserror::Error;

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
    EmergencyWithdraw{
        #[arg(long)]
        contract_id:Option<String>,
        #[arg(long)]
        token:String,
        #[arg(long)]
        recipient:String,
        #[arg(long)]
        amount:i128,
    },
    /// Webhook management commands
    Webhook {
        #[command(subcommand)]
        command: WebhookCommands,
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
#[derive(Debug,thiserror::Error)]
pub enum Error{
    #[error("Zero amount is not allowed")]
    ZeroAmount,
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
        }
    }
}
//admin and pause checks
pub fn require_admin(_context:&str)->Result<(),Error>{
    //dummy implementation
    Ok(())
}
pub fn require_not_paused(_context:&str)->Result<(),Error>{
    //dummy implementation
    Ok(())
}
//token client
pub struct TokenClient;
impl TokenClient{
    pub fn new(_rpc_url:&str,_token_address:&str)->Self{
        TokenClient
    }
    pub fn transfer(&self,_to:&str,_amount:i128)->Result<(),Error>{
        //dummy implementation
        Ok(())
    }

}