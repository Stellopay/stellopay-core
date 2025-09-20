use anyhow::Result;
use log::{info, warn, error};
use std::path::PathBuf;
use stellopay_cli::Config;
// use crate::Config;
use stellopay_cli::{require_admin,require_not_paused,TokenClient,Error, WebhookCommands};
// use crate::token;
//use soroban_sdk::contractclient::Client as SorobanHttpClient;
//use anyhow::{Result,anyhow};
//use soroban_client::rpc::Client as SorobonClient;
use crate::utils::SorobanHttpClient;

pub async fn deploy_command(
    network: String,
    owner: String,
    wasm: Option<PathBuf>,
    config: &Config,
) -> Result<()> {
    info!("Deploying contract to network: {}", network);
    
    // Determine WASM file path
    let wasm_path = wasm.unwrap_or_else(|| {
        PathBuf::from("../../onchain/target/wasm32v1-none/release/stello_pay_contract.wasm")
    });
    
    if !wasm_path.exists() {
        error!("WASM file not found: {:?}", wasm_path);
        return Err(anyhow::anyhow!("WASM file not found. Please build the contract first."));
    }
    
    // Check if soroban CLI is available
    let soroban_check = std::process::Command::new("soroban")
        .arg("--version")
        .output();
    
    if soroban_check.is_err() {
        error!("Soroban CLI not found. Please install it first:");
        error!("cargo install --locked soroban-cli");
        return Err(anyhow::anyhow!("Soroban CLI not found"));
    }
    
    println!("Deploying contract with the following parameters:");
    println!("  Network: {}", network);
    println!("  Owner: {}", owner);
    println!("  WASM file: {:?}", wasm_path);
    println!("  RPC URL: {}", config.network.rpc_url);
    println!();
    
    // Build the deployment command
    let mut cmd = std::process::Command::new("soroban");
    cmd.args([
        "contract", "deploy",
        "--wasm", wasm_path.to_str().unwrap(),
        "--rpc-url", &config.network.rpc_url,
        "--network", &network,
    ]);
    
    println!("Running deployment command...");
    println!("Command: {:?}", cmd);
    
    let output = cmd.output()?;
    
    if !output.status.success() {
        error!("Contract deployment failed:");
        error!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        error!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        return Err(anyhow::anyhow!("Contract deployment failed"));
    }
    
    let contract_id = String::from_utf8(output.stdout)?.trim().to_string();
    info!("Contract deployed successfully: {}", contract_id);
    
    println!("✅ Contract deployed successfully!");
    println!("Contract ID: {}", contract_id);
    
    // Initialize contract
    let init_output = std::process::Command::new("soroban")
        .args([
            "contract", "invoke",
            "--id", &contract_id,
            "--rpc-url", &config.network.rpc_url,
            "--network", &network,
            "--", "initialize",
            "--owner", &owner,
        ])
        .output()?;
    
    if !init_output.status.success() {
        error!("Contract initialization failed: {}", String::from_utf8_lossy(&init_output.stderr));
        return Err(anyhow::anyhow!("Contract initialization failed"));
    }
    
    info!("Contract initialized successfully");
    println!("✅ Contract initialized with owner: {}", owner);
    
    Ok(())
}

pub async fn info_command(contract_id: Option<String>, config: &Config) -> Result<()> {
    let contract_id = contract_id
        .or_else(|| config.contract.default_contract_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No contract ID provided"))?;
    
    info!("Getting contract information for: {}", contract_id);
    
    println!("Contract Information:");
    println!("  Contract ID: {}", contract_id);
    println!("  Network RPC: {}", config.network.rpc_url);
    println!("  Network Passphrase: {}", config.network.network_passphrase);
    
    // Try to get contract info using soroban CLI
    let output = std::process::Command::new("soroban")
        .args([
            "contract", "inspect",
            "--id", &contract_id,
            "--rpc-url", &config.network.rpc_url,
        ])
        .output();
    
    match output {
        Ok(output) if output.status.success() => {
            println!("\nContract Details:");
            println!("{}", String::from_utf8_lossy(&output.stdout));
        }
        Ok(output) => {
            warn!("Failed to get contract details:");
            warn!("{}", String::from_utf8_lossy(&output.stderr));
        }
        Err(e) => {
            warn!("Could not run soroban CLI: {}", e);
        }
    }
    
    Ok(())
}

pub async fn status_command(config: &Config) -> Result<()> {
    println!("StellopayCore CLI Status");
    println!("========================");
    println!();
    
    // Check configuration
    println!("Configuration:");
    println!("  Network RPC: {}", config.network.rpc_url);
    println!("  Network Passphrase: {}", config.network.network_passphrase);
    println!("  Default Contract ID: {}", 
        config.contract.default_contract_id.as_deref().unwrap_or("Not set"));
    println!();
    
    // Check if soroban CLI is available
    print!("Soroban CLI: ");
    match std::process::Command::new("soroban").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            println!("✅ Available ({})", version.trim());
        }
        Ok(_) => {
            println!("❌ Not working properly");
        }
        Err(_) => {
            println!("❌ Not found");
            println!("   Install with: cargo install --locked soroban-cli");
        }
    }
    
    // Check if contract WASM exists
    let wasm_path = PathBuf::from("../../onchain/target/wasm32v1-none/release/stello_pay_contract.wasm");
    print!("Contract WASM: ");
    if wasm_path.exists() {
        println!("✅ Built");
    } else {
        println!("❌ Not found");
        println!("   Build with: cd onchain/contracts/stello_pay_contract && soroban contract build");
    }
    
    println!();
    println!("Ready to use StellopayCore CLI!");
    
    Ok(())
}

pub async fn emergency_withdraw(
    config:&Config,
    dummy_context:&str,
    contract_id: &str,
    token: &str,
    recipient: &str,
    amount: i128,
    verbose: bool,
) -> Result<(), Error>{
    //verbose output
    if verbose{
        println!("Withdrawing {} of token {} to {}",amount,token,recipient);
    }
   
    //get secret key from config
    let signer=config.auth.secret_key
    .clone()
    .ok_or_else( || anyhow::anyhow!("Missing secret key"))?;
    //ensuring caller is admin
    require_admin(&dummy_context)?;

    //Ensuring contract is not paused
    require_not_paused(&dummy_context)?;

    //validating amount is non-zero
    if amount<=0{
        return Err(Error::ZeroAmount)
    }
     //preparing soroban contract call
    let contract_client=SorobanHttpClient::new(&config.network.rpc_url);
    //performing token transfer
    let token_client=TokenClient::new(&config.network.rpc_url,token);
    token_client.transfer(
        &recipient,
        amount,
    );
    //emitting event for transparency
    // env.events().publish(
    //     (symbol_short!("emergency_withdraw"),recipient.clone()),
    //     amount,
    // );
    //calling the contract function
    contract_client.invoke(
        contract_id,
        "emergency_withdraw",
        vec![
            ("token",token),
            ("recipient",recipient),
            ("amount",&amount.to_string()),
        ],
        &signer,
    ).await?;
    Ok(())
}

pub async fn webhook_command(
    command: WebhookCommands,
    config: &Config,
) -> Result<()> {
    match command {
        WebhookCommands::Register { name, description, url, events, secret, contract_id } => {
            webhook_register_command(name, description, url, events, secret, contract_id, config).await
        }
        WebhookCommands::Update { webhook_id, name, description, url, events, active, contract_id } => {
            webhook_update_command(webhook_id, name, description, url, events, active, contract_id, config).await
        }
        WebhookCommands::Delete { webhook_id, contract_id } => {
            webhook_delete_command(webhook_id, contract_id, config).await
        }
        WebhookCommands::List { owner, contract_id } => {
            webhook_list_command(owner, contract_id, config).await
        }
        WebhookCommands::Get { webhook_id, contract_id } => {
            webhook_get_command(webhook_id, contract_id, config).await
        }
        WebhookCommands::Stats { contract_id } => {
            webhook_stats_command(contract_id, config).await
        }
        WebhookCommands::Test { webhook_id, event_type, contract_id } => {
            webhook_test_command(webhook_id, event_type, contract_id, config).await
        }
    }
}

pub async fn webhook_register_command(
    name: String,
    description: String,
    url: String,
    events: String,
    secret: String,
    contract_id: Option<String>,
    config: &Config,
) -> Result<()> {
    let contract_id = contract_id
        .or_else(|| config.contract.default_contract_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No contract ID provided"))?;
    
    info!("Registering webhook: {}", name);
    
    println!("Registering Webhook:");
    println!("  Name: {}", name);
    println!("  Description: {}", description);
    println!("  URL: {}", url);
    println!("  Events: {}", events);
    println!("  Contract ID: {}", contract_id);
    
    // Parse events
    let event_list: Vec<&str> = events.split(',').map(|s| s.trim()).collect();
    
    // Create webhook registration data structure
    let registration_data = serde_json::json!({
        "name": name,
        "description": description,
        "url": url,
        "events": event_list,
        "secret": secret,
        "retry_config": {
            "max_retries": 3,
            "retry_delay": 60,
            "exponential_backoff": true,
            "max_delay": 3600
        },
        "security_config": {
            "signature_method": "HmacSha256",
            "rate_limit_per_minute": 60,
            "require_tls": true
        }
    });
    
    // Call contract to register webhook
    let contract_client = SorobanHttpClient::new(&config.network.rpc_url);
    let signer = config.auth.secret_key
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Missing secret key"))?;
    
    let result = contract_client.invoke(
        &contract_id,
        "register_webhook",
        vec![
            ("registration", &registration_data.to_string()),
        ],
        &signer,
    ).await?;
    
    println!("✅ Webhook registered successfully!");
    println!("Webhook ID: {}", result);
    
    Ok(())
}

pub async fn webhook_update_command(
    webhook_id: u64,
    name: Option<String>,
    description: Option<String>,
    url: Option<String>,
    events: Option<String>,
    active: Option<bool>,
    contract_id: Option<String>,
    config: &Config,
) -> Result<()> {
    let contract_id = contract_id
        .or_else(|| config.contract.default_contract_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No contract ID provided"))?;
    
    info!("Updating webhook: {}", webhook_id);
    
    println!("Updating Webhook {}:", webhook_id);
    
    // Create update data structure
    let mut update_data = serde_json::Map::new();
    
    if let Some(name) = name {
        update_data.insert("name".to_string(), serde_json::Value::String(name));
        println!("  Name: {}", name);
    }
    if let Some(description) = description {
        update_data.insert("description".to_string(), serde_json::Value::String(description));
        println!("  Description: {}", description);
    }
    if let Some(url) = url {
        update_data.insert("url".to_string(), serde_json::Value::String(url));
        println!("  URL: {}", url);
    }
    if let Some(events) = events {
        let event_list: Vec<&str> = events.split(',').map(|s| s.trim()).collect();
        update_data.insert("events".to_string(), serde_json::Value::Array(
            event_list.iter().map(|e| serde_json::Value::String(e.to_string())).collect()
        ));
        println!("  Events: {}", events);
    }
    if let Some(active) = active {
        update_data.insert("is_active".to_string(), serde_json::Value::Bool(active));
        println!("  Active: {}", active);
    }
    
    // Call contract to update webhook
    let contract_client = SorobanHttpClient::new(&config.network.rpc_url);
    let signer = config.auth.secret_key
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Missing secret key"))?;
    
    contract_client.invoke(
        &contract_id,
        "update_webhook",
        vec![
            ("webhook_id", &webhook_id.to_string()),
            ("update", &serde_json::Value::Object(update_data).to_string()),
        ],
        &signer,
    ).await?;
    
    println!("✅ Webhook updated successfully!");
    
    Ok(())
}

pub async fn webhook_delete_command(
    webhook_id: u64,
    contract_id: Option<String>,
    config: &Config,
) -> Result<()> {
    let contract_id = contract_id
        .or_else(|| config.contract.default_contract_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No contract ID provided"))?;
    
    info!("Deleting webhook: {}", webhook_id);
    
    println!("Deleting Webhook {}:", webhook_id);
    
    // Call contract to delete webhook
    let contract_client = SorobanHttpClient::new(&config.network.rpc_url);
    let signer = config.auth.secret_key
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Missing secret key"))?;
    
    contract_client.invoke(
        &contract_id,
        "delete_webhook",
        vec![
            ("webhook_id", &webhook_id.to_string()),
        ],
        &signer,
    ).await?;
    
    println!("✅ Webhook deleted successfully!");
    
    Ok(())
}

pub async fn webhook_list_command(
    owner: String,
    contract_id: Option<String>,
    config: &Config,
) -> Result<()> {
    let contract_id = contract_id
        .or_else(|| config.contract.default_contract_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No contract ID provided"))?;
    
    info!("Listing webhooks for owner: {}", owner);
    
    println!("Webhooks for Owner: {}", owner);
    
    // Call contract to list webhooks
    let contract_client = SorobanHttpClient::new(&config.network.rpc_url);
    
    let result = contract_client.query(
        &contract_id,
        "list_owner_webhooks",
        vec![
            ("owner", &owner),
        ],
    ).await?;
    
    println!("Webhook IDs: {}", result);
    
    Ok(())
}

pub async fn webhook_get_command(
    webhook_id: u64,
    contract_id: Option<String>,
    config: &Config,
) -> Result<()> {
    let contract_id = contract_id
        .or_else(|| config.contract.default_contract_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No contract ID provided"))?;
    
    info!("Getting webhook: {}", webhook_id);
    
    println!("Webhook Information:");
    println!("  Webhook ID: {}", webhook_id);
    
    // Call contract to get webhook
    let contract_client = SorobanHttpClient::new(&config.network.rpc_url);
    
    let result = contract_client.query(
        &contract_id,
        "get_webhook",
        vec![
            ("webhook_id", &webhook_id.to_string()),
        ],
    ).await?;
    
    println!("Webhook Details: {}", result);
    
    Ok(())
}

pub async fn webhook_stats_command(
    contract_id: Option<String>,
    config: &Config,
) -> Result<()> {
    let contract_id = contract_id
        .or_else(|| config.contract.default_contract_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No contract ID provided"))?;
    
    info!("Getting webhook statistics");
    
    println!("Webhook Statistics:");
    
    // Call contract to get webhook stats
    let contract_client = SorobanHttpClient::new(&config.network.rpc_url);
    
    let result = contract_client.query(
        &contract_id,
        "get_webhook_stats",
        vec![],
    ).await?;
    
    println!("Statistics: {}", result);
    
    Ok(())
}

pub async fn webhook_test_command(
    webhook_id: u64,
    event_type: String,
    contract_id: Option<String>,
    config: &Config,
) -> Result<()> {
    let contract_id = contract_id
        .or_else(|| config.contract.default_contract_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No contract ID provided"))?;
    
    info!("Testing webhook: {} with event: {}", webhook_id, event_type);
    
    println!("Testing Webhook:");
    println!("  Webhook ID: {}", webhook_id);
    println!("  Event Type: {}", event_type);
    
    // Call contract to test webhook
    let contract_client = SorobanHttpClient::new(&config.network.rpc_url);
    let signer = config.auth.secret_key
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Missing secret key"))?;
    
    let result = contract_client.invoke(
        &contract_id,
        "test_webhook",
        vec![
            ("webhook_id", &webhook_id.to_string()),
            ("event_type", &event_type),
        ],
        &signer,
    ).await?;
    
    println!("✅ Webhook test completed!");
    println!("Result: {}", result);
    
    Ok(())
}