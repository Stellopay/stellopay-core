use clap::Parser;
use std::process;

mod commands;
mod config;
mod utils;

use commands::*;
use config::*;
use stellopay_cli::{Cli, Commands};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    
    // Set up logging
    if cli.verbose {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }
    
    // Load configuration
    let config = match load_config(&cli.config).await {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            process::exit(1);
        }
    };
    
    // Execute command
    let result = match cli.command {
        Commands::Deploy { network, owner, wasm } => {
            deploy_command(network, owner, wasm, &config).await
        }
        Commands::Info { contract_id } => {
            info_command(contract_id, &config).await
        }
        Commands::Status => {
            status_command(&config).await
        }
    };
    
    match result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

pub struct DeployArgs {
    pub network: String,
    pub owner: String,
    pub wasm: Option<std::path::PathBuf>,
}

pub struct DepositArgs {
    pub amount: i128,
    pub token: String,
    pub employer: Option<String>,
}

pub struct PayArgs {
    pub employee: String,
    pub employer: Option<String>,
}

pub struct BulkPayArgs {
    pub employees: std::path::PathBuf,
    pub limit: usize,
}

pub struct InfoArgs {
    pub detailed: bool,
}

pub struct StreamArgs {
    pub events: Vec<String>,
    pub format: String,
}

fn get_rpc_url_for_network(network: &str) -> String {
    match network {
        "testnet" => "https://soroban-testnet.stellar.org:443".to_string(),
        "mainnet" => "https://soroban-mainnet.stellar.org:443".to_string(),
        _ => panic!("Unknown network: {}", network),
    }
}

fn get_network_passphrase(network: &str) -> String {
    match network {
        "testnet" => "Test SDF Network ; September 2015".to_string(),
        "mainnet" => "Public Global Stellar Network ; September 2015".to_string(),
        _ => panic!("Unknown network: {}", network),
    }
}
