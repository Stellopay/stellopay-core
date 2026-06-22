use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;
use tempfile::TempDir;

use std::path::PathBuf;
use tokio::fs;

use stellopay_cli::config::{load_config, get_secret_key};
use stellopay_cli::commands::emergency_withdraw;
use stellopay_cli::{Config, Error, NetworkConfig, ContractConfig, AuthConfig, DefaultsConfig};

const VALID_CONTRACT:  &str = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";
const VALID_TOKEN:     &str = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCN3";
const VALID_RECIPIENT: &str = "GBZXN7PIRZGNMHGA7MUUUF4GWPY5AYPGK4YVMQKN74ILIXB4UGOT7ZN";
const VALID_AMOUNT:   i128  = 1_000;
const SECRET_KEY:      &str = "SCZANGBA5AKIA7MXODKVS4EKDRNKJHXIXLJHM6H3RDNL3VRI7RJGMQE";

fn make_config(secret_key: Option<&str>) -> Config {
    Config {
        network: NetworkConfig {
            rpc_url: "https://soroban-testnet.stellar.org:443".to_string(),
            network_passphrase: "Test SDF Network ; September 2015".to_string(),
        },
        contract: ContractConfig {
            default_contract_id: Some("CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4".to_string()),
        },
        auth: AuthConfig {
            secret_key: secret_key.map(str::to_string),
        },
        defaults: DefaultsConfig {
            token: None,
            frequency: "monthly".to_string(),
        },
    }
}


#[test]
fn test_cli_help() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("CLI tool for StellopayCore contract management"));
}

#[test]
fn test_cli_status() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("status");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("StellopayCore CLI Status"));
}

#[test]
fn test_cli_info_without_contract_id() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("info");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("No contract ID provided"));
}

#[test]
fn test_cli_info_with_contract_id() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("info")
        .arg("--contract-id")
        .arg("CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE");
    
    // This might fail depending on network connectivity, but should at least attempt to connect
    let output = cmd.output().unwrap();
    assert!(output.status.success() || String::from_utf8_lossy(&output.stderr).contains("Failed to get contract details"));
}

#[test] 
fn test_deploy_without_owner() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("deploy");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_config_file_creation() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("test_config.toml");
    
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("--config")
        .arg(config_path.to_str().unwrap())
        .arg("status");
    
    cmd.assert().success();
    
    // Check that config file was created
    assert!(config_path.exists());
    
    // Check config file content
    let config_content = std::fs::read_to_string(&config_path).unwrap();
    assert!(config_content.contains("rpc_url"));
    assert!(config_content.contains("network_passphrase"));
}

#[test]
fn test_deploy_with_invalid_owner() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("deploy")
        .arg("--owner")
        .arg("invalid_address");
    
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Contract deployment failed"));
}

#[test]
fn test_deploy_with_missing_wasm() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("deploy")
        .arg("--owner")
        .arg("GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
    
    // Should fail due to missing WASM file or other deployment issues
    let output = cmd.output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_invalid_command() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("invalid_command");
    
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn test_version_flag() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("--version");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("stellopay-cli"));
}

#[test]
fn test_config_with_custom_network() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("custom_config.toml");
    
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("--config")
        .arg(config_path.to_str().unwrap())
        .arg("deploy")
        .arg("--network")
        .arg("futurenet")
        .arg("--owner")
        .arg("GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
    
    // Should fail due to missing WASM or other issues, but config should be created
    let _output = cmd.output().unwrap();
    assert!(config_path.exists());
}

#[test]
fn test_info_with_invalid_contract_id() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("info")
        .arg("--contract-id")
        .arg("invalid_contract_id");
    
    let output = cmd.output().unwrap();
    // The CLI currently doesn't validate contract ID format but logs warnings
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Failed to get contract details"));
}

#[test]
fn test_concurrent_config_access() {
    use std::thread;
    use std::sync::Arc;
    
    let temp_dir = Arc::new(TempDir::new().unwrap());
    let config_path = temp_dir.path().join("concurrent_config.toml");
    
    let handles: Vec<_> = (0..3).map(|i| {
        let config_path = config_path.clone();
        thread::spawn(move || {
            let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
            cmd.arg("--config")
                .arg(config_path.to_str().unwrap())
                .arg("status");
            
            let output = cmd.output().unwrap();
            assert!(output.status.success(), "Thread {} failed", i);
        })
    }).collect();
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    // Config should exist after all threads complete
    assert!(config_path.exists());
}

#[test]
fn test_cli_with_verbose_flag() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("--verbose")
        .arg("status");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("StellopayCore CLI Status"));
}

#[test]
fn test_cli_with_short_verbose_flag() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("-v")
        .arg("status");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("StellopayCore CLI Status"));
}

#[tokio::test]
async fn test_zero_amount_returns_zero_amount_error() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(&config, "cli-context", VALID_CONTRACT,VALID_TOKEN, VALID_RECIPIENT, 0, false,).await;

    assert!(
        matches!(result, Err(Error::ZeroAmount)),
        "Expected ZeroAmount, got: {result:?}"
    );
}

#[tokio::test]
async fn test_negative_amount_returns_zero_amount_error() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(&config, "cli-context", VALID_CONTRACT, VALID_TOKEN, VALID_RECIPIENT, -1, false,).await;

    assert!(
        matches!(result, Err(Error::ZeroAmount)),
        "Expected ZeroAmount for negative input, got: {result:?}"
    );
}

#[tokio::test]
async fn test_amount_exceeding_maximum_returns_error() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(&config, "cli-context", VALID_CONTRACT,VALID_TOKEN, VALID_RECIPIENT, 100_000_001, false,).await;

    assert!(
        matches!(result, Err(Error::MaximumAmount)),
        "Expected MaximumAmount, got: {result:?}"
    );
}

#[tokio::test]
async fn test_amount_at_exact_maximum_passes_amount_guard() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(&config, "cli-context", VALID_CONTRACT, VALID_TOKEN, VALID_RECIPIENT, 100_000_000, false,).await;

    assert!(
        !matches!(result, Err(Error::MaximumAmount) | Err(Error::ZeroAmount)),
        "Boundary value must pass amount guards, got: {result:?}"
    );
}

#[tokio::test]
async fn test_invalid_recipient_returns_invalid_address_error() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(&config, "cli-context", VALID_CONTRACT, VALID_TOKEN, "invalid_address", VALID_AMOUNT, false,).await;

    assert!(
        matches!(result, Err(Error::InvalidAddress)),
        "Expected InvalidAddress, got: {result:?}"
    );
}

#[tokio::test]
async fn test_over_limit_checked_before_address_validation() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(&config, "cli-context", VALID_CONTRACT,VALID_TOKEN, "bad-address", 200_000_000, false).await;

    assert!(
        matches!(result, Err(Error::MaximumAmount)),
        "MaximumAmount must fire before InvalidAddress, got: {result:?}"
    );
}