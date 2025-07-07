use anyhow::Result;
use log::{info, warn, error};
use std::path::PathBuf;
use stellopay_cli::Config;

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
