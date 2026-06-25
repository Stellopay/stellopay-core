use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;
use tempfile::TempDir;

use std::path::PathBuf;
use tokio::fs;

use stellopay_cli::commands::emergency_withdraw;
use stellopay_cli::config::{get_secret_key, load_config};
use stellopay_cli::utils::SorobanHttpClient;
use stellopay_cli::{AuthConfig, Config, ContractConfig, DefaultsConfig, Error, NetworkConfig};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const VALID_CONTRACT: &str = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";
const VALID_TOKEN: &str = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCN3";
const VALID_RECIPIENT: &str = "GBZXN7PIRZGNMHGA7MUUUF4GWPY5AYPGK4YVMQKN74ILIXB4UGOT7ZN";
const VALID_AMOUNT: i128 = 1_000;
const SECRET_KEY: &str = "SCZANGBA5AKIA7MXODKVS4EKDRNKJHXIXLJHM6H3RDNL3VRI7RJGMQE";

fn make_config(secret_key: Option<&str>) -> Config {
    Config {
        network: NetworkConfig {
            rpc_url: "https://soroban-testnet.stellar.org:443".to_string(),
            network_passphrase: "Test SDF Network ; September 2015".to_string(),
        },
        contract: ContractConfig {
            default_contract_id: Some(
                "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4".to_string(),
            ),
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
    cmd.assert().success().stdout(predicate::str::contains(
        "CLI tool for StellopayCore contract management",
    ));
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
    assert!(
        output.status.success()
            || String::from_utf8_lossy(&output.stderr).contains("Failed to get contract details")
    );
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
    cmd.arg("deploy").arg("--owner").arg("invalid_address");

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
    use std::sync::Arc;
    use std::thread;

    let temp_dir = Arc::new(TempDir::new().unwrap());
    let config_path = temp_dir.path().join("concurrent_config.toml");

    let handles: Vec<_> = (0..3)
        .map(|i| {
            let config_path = config_path.clone();
            thread::spawn(move || {
                let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
                cmd.arg("--config")
                    .arg(config_path.to_str().unwrap())
                    .arg("status");

                let output = cmd.output().unwrap();
                assert!(output.status.success(), "Thread {} failed", i);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // Config should exist after all threads complete
    assert!(config_path.exists());
}

#[test]
fn test_cli_with_verbose_flag() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("--verbose").arg("status");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("StellopayCore CLI Status"));
}

#[test]
fn test_cli_with_short_verbose_flag() {
    let mut cmd = Command::cargo_bin("stellopay-cli").unwrap();
    cmd.arg("-v").arg("status");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("StellopayCore CLI Status"));
}

#[tokio::test]
async fn test_zero_amount_returns_zero_amount_error() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(
        &config,
        "cli-context",
        VALID_CONTRACT,
        VALID_TOKEN,
        VALID_RECIPIENT,
        0,
        false,
    )
    .await;

    assert!(
        matches!(result, Err(Error::ZeroAmount)),
        "Expected ZeroAmount, got: {result:?}"
    );
}

#[tokio::test]
async fn test_negative_amount_returns_zero_amount_error() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(
        &config,
        "cli-context",
        VALID_CONTRACT,
        VALID_TOKEN,
        VALID_RECIPIENT,
        -1,
        false,
    )
    .await;

    assert!(
        matches!(result, Err(Error::ZeroAmount)),
        "Expected ZeroAmount for negative input, got: {result:?}"
    );
}

#[tokio::test]
async fn test_amount_exceeding_maximum_returns_error() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(
        &config,
        "cli-context",
        VALID_CONTRACT,
        VALID_TOKEN,
        VALID_RECIPIENT,
        100_000_001,
        false,
    )
    .await;

    assert!(
        matches!(result, Err(Error::MaximumAmount)),
        "Expected MaximumAmount, got: {result:?}"
    );
}

#[tokio::test]
async fn test_amount_at_exact_maximum_passes_amount_guard() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(
        &config,
        "cli-context",
        VALID_CONTRACT,
        VALID_TOKEN,
        VALID_RECIPIENT,
        100_000_000,
        false,
    )
    .await;

    assert!(
        !matches!(result, Err(Error::MaximumAmount) | Err(Error::ZeroAmount)),
        "Boundary value must pass amount guards, got: {result:?}"
    );
}

#[tokio::test]
async fn test_invalid_recipient_returns_invalid_address_error() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(
        &config,
        "cli-context",
        VALID_CONTRACT,
        VALID_TOKEN,
        "invalid_address",
        VALID_AMOUNT,
        false,
    )
    .await;

    assert!(
        matches!(result, Err(Error::InvalidAddress)),
        "Expected InvalidAddress, got: {result:?}"
    );
}

#[tokio::test]
async fn test_over_limit_checked_before_address_validation() {
    let config = make_config(Some(SECRET_KEY));

    let result = emergency_withdraw(
        &config,
        "cli-context",
        VALID_CONTRACT,
        VALID_TOKEN,
        "bad-address",
        200_000_000,
        false,
    )
    .await;

    assert!(
        matches!(result, Err(Error::MaximumAmount)),
        "MaximumAmount must fire before InvalidAddress, got: {result:?}"
    );
}

// --- SorobanHttpClient::query tests ---
//
// These tests exercise the read-only `query` path against a local mock RPC
// server (wiremock), so they do not depend on network access or a live
// Soroban node.

#[tokio::test]
async fn test_query_success_returns_result_field() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": { "webhook_id": 1, "active": true }
        })))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let result = client
        .query(VALID_CONTRACT, "get_webhook", vec![("webhook_id", "1")])
        .await
        .expect("query should succeed");

    assert_eq!(result["webhook_id"], 1);
    assert_eq!(result["active"], true);
}

#[tokio::test]
async fn test_query_empty_result() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": []
        })))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let result = client
        .query(VALID_CONTRACT, "list_owner_webhooks", vec![("owner", "G...")])
        .await
        .expect("query should succeed even with an empty result");

    assert_eq!(result, serde_json::json!([]));
}

#[tokio::test]
async fn test_query_returns_whole_body_when_no_result_field() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_webhooks": 3,
            "active_webhooks": 2
        })))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let result = client
        .query(VALID_CONTRACT, "get_webhook_stats", vec![])
        .await
        .expect("query should succeed");

    assert_eq!(result["total_webhooks"], 3);
    assert_eq!(result["active_webhooks"], 2);
}

#[tokio::test]
async fn test_query_rpc_error_surfaces_as_err() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "contract not found"
        })))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let err = client
        .query("unknown_contract", "get_webhook", vec![])
        .await
        .expect_err("an RPC-level error field should surface as Err");

    assert!(
        err.to_string().contains("contract not found"),
        "expected error message to include RPC error, got: {err}"
    );
}

#[tokio::test]
async fn test_query_http_error_status_surfaces_as_err() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let err = client
        .query(VALID_CONTRACT, "get_webhook", vec![])
        .await
        .expect_err("a non-2xx HTTP status should surface as Err");

    assert!(
        err.to_string().contains("500"),
        "expected error message to include status code, got: {err}"
    );
}

#[tokio::test]
async fn test_query_malformed_response_surfaces_as_err() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let err = client
        .query(VALID_CONTRACT, "get_webhook", vec![])
        .await
        .expect_err("a non-JSON body should surface as Err");

    assert!(
        err.to_string().contains("Malformed"),
        "expected error message to flag malformed response, got: {err}"
    );
}

#[tokio::test]
async fn test_query_request_never_includes_a_signer() {
    // Security property: the read-only query path must not carry a signer or
    // secret key, unlike `invoke`. We assert this both by the method's
    // signature (it takes no signer argument) and by inspecting the actual
    // request body sent over the wire.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .and(body_partial_json(serde_json::json!({ "read_only": true })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": "ok"
        })))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let result = client
        .query(VALID_CONTRACT, "get_webhook_stats", vec![])
        .await
        .expect("query should succeed");

    assert_eq!(result, "ok");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let sent_body: serde_json::Value = requests[0].body_json().unwrap();
    assert!(
        sent_body.get("signer").is_none(),
        "query request body must never include a signer field, got: {sent_body}"
    );
    assert_eq!(sent_body["read_only"], true);
}

#[tokio::test]
async fn test_query_as_deserializes_into_typed_struct() {
    use stellopay_cli::utils::WebhookInfo;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {
                "id": 7,
                "name": "payroll-events",
                "description": "notifies on payroll runs",
                "url": "https://example.com/hook",
                "events": ["payment.sent"],
                "is_active": true
            }
        })))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let webhook: WebhookInfo = client
        .query_as(VALID_CONTRACT, "get_webhook", vec![("webhook_id", "7")])
        .await
        .expect("typed query should succeed");

    assert_eq!(webhook.id, Some(7));
    assert_eq!(webhook.name, Some("payroll-events".to_string()));
    assert_eq!(webhook.is_active, Some(true));
    assert_eq!(webhook.events, Some(vec!["payment.sent".to_string()]));
}

#[tokio::test]
async fn test_query_as_tolerates_missing_optional_fields() {
    use stellopay_cli::utils::WebhookInfo;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": { "id": 9 }
        })))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let webhook: WebhookInfo = client
        .query_as(VALID_CONTRACT, "get_webhook", vec![("webhook_id", "9")])
        .await
        .expect("typed query should tolerate missing optional fields");

    assert_eq!(webhook.id, Some(9));
    assert_eq!(webhook.name, None);
    assert_eq!(webhook.retry_config, None);
}

#[tokio::test]
async fn test_query_as_returns_err_on_shape_mismatch() {
    use stellopay_cli::utils::WebhookStats;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": { "total_webhooks": "not-a-number" }
        })))
        .mount(&server)
        .await;

    let client = SorobanHttpClient::new(&server.uri());
    let err = client
        .query_as::<WebhookStats>(VALID_CONTRACT, "get_webhook_stats", vec![])
        .await
        .expect_err("a type mismatch in the result shape should surface as Err");

    assert!(
        err.to_string().contains("did not match expected shape"),
        "expected shape-mismatch error, got: {err}"
    );
}
