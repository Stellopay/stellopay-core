use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;
use tempfile::TempDir;

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
