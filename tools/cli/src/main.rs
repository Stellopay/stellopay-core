use clap::Parser;
use std::process;

use anyhow::anyhow;
use stellopay_cli::commands::*;
use stellopay_cli::config::*;
use stellopay_cli::{classify_error, Cli, Commands, Config, Error, ExitCode, WebhookCommands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Set up logging
    if cli.verbose {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }

    // Load configuration from the --config path (CLI flag, layer 1).
    let cli_config = match load_config(&cli.config).await {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            process::exit(classify_error(&e).as_u8());
        }
    };

    // Merge with env vars and the optional project-local stellopay.toml
    // (layers 2–4).  Precedence: CLI flag > env var > stellopay.toml > default.
    let config = match resolve_config_with_project_file(Some(&cli_config), PROJECT_CONFIG_FILE) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error resolving config: {}", e);
            process::exit(1);
        }
    };

    // Execute command
    let result = match cli.command {
        Commands::Deploy {
            network,
            owner,
            wasm,
        } => deploy_command(network, owner, wasm, &config).await,
        Commands::Info { contract_id } => info_command(contract_id, &config).await,
        Commands::Status => status_command(&config).await,
        Commands::Webhook { command } => webhook_command(command, &config).await,
        Commands::EmergencyWithdraw {
            contract_id,
            token,
            recipient,
            amount,
        } => {
            // //Loading config
            let dummy_env = "cli-context";
            // let config=load_config(&cli.config)?;
            // //resolving contract ID from arg or config
            let contract_id_str = contract_id
                .as_deref()
                .ok_or_else(|| anyhow!("Missing contract ID"))?;

            //calling logic function
            emergency_withdraw(
                &config,
                &dummy_env,
                contract_id_str,
                &token,
                &recipient,
                amount,
                cli.verbose,
            )
            .await?;
            Ok(())
        }
    };

    match result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(classify_error(&e).as_u8());
        }
    }
    Ok(())
}

/// Supported Stellar network identifiers accepted by `--network`.
pub(crate) const SUPPORTED_NETWORKS: &[&str] = &["testnet", "mainnet"];

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

pub(crate) fn get_rpc_url_for_network(network: &str) -> anyhow::Result<String> {
    match network {
        "testnet" => Ok("https://soroban-testnet.stellar.org:443".to_string()),
        "mainnet" => Ok("https://soroban-mainnet.stellar.org:443".to_string()),
        other => Err(anyhow!(
            "unsupported network '{}'. Supported networks: {}",
            other,
            SUPPORTED_NETWORKS.join(", ")
        )),
    }
}

pub(crate) fn get_network_passphrase(network: &str) -> anyhow::Result<String> {
    match network {
        "testnet" => Ok("Test SDF Network ; September 2015".to_string()),
        "mainnet" => Ok("Public Global Stellar Network ; September 2015".to_string()),
        other => Err(anyhow!(
            "unsupported network '{}'. Supported networks: {}",
            other,
            SUPPORTED_NETWORKS.join(", ")
        )),
    }
}
