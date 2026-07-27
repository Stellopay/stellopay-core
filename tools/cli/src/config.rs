//! Configuration loading and resolution for the Stellopay CLI.
//!
//! # Precedence order (highest → lowest)
//!
//! 1. **CLI flag** — the `--config` path passed on the command line, which
//!    points to the user's global config file (e.g. `~/.stellopay/config.toml`).
//! 2. **Environment variables** — `STELLOPAY_RPC_URL`, `STELLOPAY_NETWORK_PASSPHRASE`,
//!    `STELLOPAY_CONTRACT_ID`, `STELLOPAY_SECRET_KEY`, `STELLOPAY_TOKEN`,
//!    `STELLOPAY_FREQUENCY`.
//! 3. **Project-local TOML file** — `stellopay.toml` in the current working
//!    directory (optional).  Silently ignored when absent; returns a clear
//!    error when present but malformed.
//! 4. **Built-in defaults** — hard-coded fallback values compiled into the
//!    binary (testnet RPC, monthly frequency, etc.).
//!
//! [`resolve_config`] is the single entry-point that applies this stack and
//! returns a fully-populated [`Config`].

use crate::{AuthConfig, Config, ContractConfig, DefaultsConfig, NetworkConfig};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

// ── Project-local TOML file name ─────────────────────────────────────────────

/// Name of the optional project-local config file searched in the CWD.
pub const PROJECT_CONFIG_FILE: &str = "stellopay.toml";

// ── Sparse intermediate structs (all fields optional) ────────────────────────

/// Sparse version of [`NetworkConfig`] used when parsing the project-local
/// TOML file.  Every field is `Option` so that absent keys do not overwrite
/// higher-precedence values.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct FileNetworkConfig {
    pub rpc_url: Option<String>,
    pub network_passphrase: Option<String>,
}

/// Sparse version of [`ContractConfig`] for the project-local TOML file.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct FileContractConfig {
    pub default_contract_id: Option<String>,
}

/// Sparse version of [`AuthConfig`] for the project-local TOML file.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct FileAuthConfig {
    pub secret_key: Option<String>,
}

/// Sparse version of [`DefaultsConfig`] for the project-local TOML file.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct FileDefaultsConfig {
    pub token: Option<String>,
    pub frequency: Option<String>,
}

/// Represents the full contents of a `stellopay.toml` project-local config
/// file.  All sections and fields are optional so a partial file is valid.
///
/// # Example `stellopay.toml`
///
/// ```toml
/// [network]
/// rpc_url = "https://soroban-testnet.stellar.org:443"
///
/// [contract]
/// default_contract_id = "CABC..."
///
/// [defaults]
/// frequency = "weekly"
/// ```
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct FileConfig {
    pub network: Option<FileNetworkConfig>,
    pub contract: Option<FileContractConfig>,
    pub auth: Option<FileAuthConfig>,
    pub defaults: Option<FileDefaultsConfig>,
}

// ── Project-local TOML loader ─────────────────────────────────────────────────

/// Attempt to load and parse `stellopay.toml` from the current working
/// directory.
///
/// * Returns `Ok(None)` when the file does not exist (not an error).
/// * Returns `Err` with a descriptive message when the file exists but cannot
///   be parsed as valid TOML — this satisfies the requirement to "fail with a
///   clear error if the TOML file exists but is malformed".
pub fn load_project_config() -> Result<Option<FileConfig>> {
    load_project_config_from(PROJECT_CONFIG_FILE)
}

/// Testable variant: load from an explicit path instead of the CWD sentinel.
pub fn load_project_config_from(path: &str) -> Result<Option<FileConfig>> {
    let p = Path::new(path);
    if !p.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(p)
        .with_context(|| format!("Failed to read config file '{}'", path))?;

    let cfg: FileConfig = toml::from_str(&content).with_context(|| {
        format!(
            "Malformed TOML in '{}': file exists but could not be parsed",
            path
        )
    })?;

    Ok(Some(cfg))
}

// ── Environment-variable layer ────────────────────────────────────────────────

/// All environment variables recognised by the CLI.  Using typed constants
/// avoids typos across the codebase.
pub mod env_keys {
    pub const RPC_URL: &str = "STELLOPAY_RPC_URL";
    pub const NETWORK_PASSPHRASE: &str = "STELLOPAY_NETWORK_PASSPHRASE";
    pub const CONTRACT_ID: &str = "STELLOPAY_CONTRACT_ID";
    pub const SECRET_KEY: &str = "STELLOPAY_SECRET_KEY";
    pub const TOKEN: &str = "STELLOPAY_TOKEN";
    pub const FREQUENCY: &str = "STELLOPAY_FREQUENCY";
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

// ── Config resolution ─────────────────────────────────────────────────────────

/// Build a fully-resolved [`Config`] by layering all configuration sources.
///
/// # Precedence (highest → lowest)
///
/// 1. **CLI flag** — the caller passes the already-loaded `cli_config`
///    (parsed from the `--config` path).  Any `Some` value there wins.
/// 2. **Environment variables** — checked via [`std::env::var`] for every
///    recognised key.
/// 3. **Project-local TOML** — `stellopay.toml` in the CWD, loaded via
///    [`load_project_config`].  Absent → skipped; malformed → `Err`.
/// 4. **Built-in defaults** — values from [`Config::default()`].
///
/// The merge is explicit: each field is resolved independently so the
/// precedence is clear and auditable at the call site.
pub fn resolve_config(cli_config: Option<&Config>) -> Result<Config> {
    resolve_config_with_project_file(cli_config, PROJECT_CONFIG_FILE)
}

/// Testable variant: accepts an explicit project-file path so tests can use
/// temporary files without touching the real CWD.
pub fn resolve_config_with_project_file(
    cli_config: Option<&Config>,
    project_file: &str,
) -> Result<Config> {
    // Layer 3 — project-local TOML (may be absent).
    let file = load_project_config_from(project_file)?;

    // Layer 4 — built-in defaults.
    let defaults = Config::default();

    // ── network.rpc_url ───────────────────────────────────────────────────
    // Precedence: CLI flag > env var > TOML file > default
    let rpc_url = cli_config
        .map(|c| c.network.rpc_url.clone())
        .filter(|v| !v.is_empty())
        .or_else(|| env_opt(env_keys::RPC_URL))
        .or_else(|| file.as_ref()?.network.as_ref()?.rpc_url.clone())
        .unwrap_or(defaults.network.rpc_url);

    // ── network.network_passphrase ────────────────────────────────────────
    let network_passphrase = cli_config
        .map(|c| c.network.network_passphrase.clone())
        .filter(|v| !v.is_empty())
        .or_else(|| env_opt(env_keys::NETWORK_PASSPHRASE))
        .or_else(|| {
            file.as_ref()?
                .network
                .as_ref()?
                .network_passphrase
                .clone()
        })
        .unwrap_or(defaults.network.network_passphrase);

    // ── contract.default_contract_id ─────────────────────────────────────
    let default_contract_id = cli_config
        .and_then(|c| c.contract.default_contract_id.clone())
        .or_else(|| env_opt(env_keys::CONTRACT_ID))
        .or_else(|| {
            file.as_ref()?
                .contract
                .as_ref()?
                .default_contract_id
                .clone()
        })
        .or(defaults.contract.default_contract_id);

    // ── auth.secret_key ───────────────────────────────────────────────────
    let secret_key = cli_config
        .and_then(|c| c.auth.secret_key.clone())
        .or_else(|| env_opt(env_keys::SECRET_KEY))
        .or_else(|| file.as_ref()?.auth.as_ref()?.secret_key.clone())
        .or(defaults.auth.secret_key);

    // ── defaults.token ────────────────────────────────────────────────────
    let token = cli_config
        .and_then(|c| c.defaults.token.clone())
        .or_else(|| env_opt(env_keys::TOKEN))
        .or_else(|| file.as_ref()?.defaults.as_ref()?.token.clone())
        .or(defaults.defaults.token);

    // ── defaults.frequency ────────────────────────────────────────────────
    let frequency = cli_config
        .map(|c| c.defaults.frequency.clone())
        .filter(|v| !v.is_empty())
        .or_else(|| env_opt(env_keys::FREQUENCY))
        .or_else(|| file.as_ref()?.defaults.as_ref()?.frequency.clone())
        .unwrap_or(defaults.defaults.frequency);

    Ok(Config {
        network: NetworkConfig {
            rpc_url,
            network_passphrase,
        },
        contract: ContractConfig {
            default_contract_id,
        },
        auth: AuthConfig { secret_key },
        defaults: DefaultsConfig { token, frequency },
    })
}

// ── Legacy async helpers (kept for backward compatibility) ────────────────────

/// Load a `Config` from an explicit file path.
///
/// If the file does not exist a default config is written there and returned.
/// This is the original behaviour used by `main.rs` for the `--config` flag.
pub async fn load_config(config_path: &Path) -> Result<Config> {
    // Expand tilde in path
    let expanded_path = if config_path.starts_with("~") {
        let home_dir =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        let path_str = config_path.to_string_lossy();
        let without_tilde = &path_str[1..];
        home_dir.join(without_tilde.trim_start_matches('/'))
    } else {
        config_path.to_path_buf()
    };

    if !expanded_path.exists() {
        let default_config = Config::default();
        create_config_file(&expanded_path, &default_config).await?;
        return Ok(default_config);
    }

    let config_content = fs::read_to_string(&expanded_path).await?;
    let config: Config = toml::from_str(&config_content)
        .with_context(|| format!("Malformed TOML in '{}'", expanded_path.display()))?;

    Ok(config)
}

/// Write a [`Config`] to `path` as pretty-printed TOML, creating parent
/// directories as needed.
pub async fn create_config_file(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let config_content = toml::to_string_pretty(config)?;
    fs::write(path, config_content).await?;

    println!("Created default config file at: {}", path.display());

    Ok(())
}

/// Return the secret key, checking environment first then the config struct.
pub fn get_secret_key(config: &Config) -> Result<String> {
    if let Ok(key) = std::env::var(env_keys::SECRET_KEY) {
        return Ok(key);
    }

    if let Some(secret_key) = &config.auth.secret_key {
        return Ok(secret_key.clone());
    }

    Err(anyhow::anyhow!(
        "No secret key found. Set {} environment variable or add it to config file",
        env_keys::SECRET_KEY
    ))
}


// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    /// Process-wide lock that serialises every test that touches environment
    /// variables — either setting them *or* reading them via `resolve_config`.
    /// Without this, two parallel tests racing on the same env key produce
    /// flaky results.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// Acquire the env lock.  Holding the returned guard for the test's entire
    /// scope ensures exclusive access to the process environment.
    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Convenience: clear every known STELLOPAY_ env var so a test that does
    /// not explicitly set them starts from a clean slate.
    fn clear_all_env() {
        std::env::remove_var(env_keys::RPC_URL);
        std::env::remove_var(env_keys::NETWORK_PASSPHRASE);
        std::env::remove_var(env_keys::CONTRACT_ID);
        std::env::remove_var(env_keys::SECRET_KEY);
        std::env::remove_var(env_keys::TOKEN);
        std::env::remove_var(env_keys::FREQUENCY);
    }

    fn tmp_toml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("tmp file");
        f.write_all(content.as_bytes()).expect("write");
        f
    }

    fn tmp_path(f: &NamedTempFile) -> &str {
        f.path().to_str().expect("utf8 path")
    }

    // ── load_project_config_from ──────────────────────────────────────────

    #[test]
    fn absent_file_returns_none() {
        let _env = lock_env();
        let result = load_project_config_from("/tmp/stellopay_does_not_exist_xyz.toml");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn empty_file_returns_all_none_fields() {
        let _env = lock_env();
        let f = tmp_toml("");
        let cfg = load_project_config_from(tmp_path(&f))
            .expect("ok")
            .expect("some");
        assert!(cfg.network.is_none());
        assert!(cfg.contract.is_none());
        assert!(cfg.auth.is_none());
        assert!(cfg.defaults.is_none());
    }

    #[test]
    fn malformed_toml_returns_clear_error() {
        let _env = lock_env();
        let f = tmp_toml("this is not [ valid toml !!!");
        let err = load_project_config_from(tmp_path(&f)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Malformed TOML"),
            "expected 'Malformed TOML' in error, got: {msg}"
        );
    }

    #[test]
    fn partial_network_section_parses() {
        let _env = lock_env();
        let f = tmp_toml(
            r#"
[network]
rpc_url = "https://rpc.example.com"
"#,
        );
        let cfg = load_project_config_from(tmp_path(&f))
            .expect("ok")
            .expect("some");
        let net = cfg.network.expect("network section");
        assert_eq!(net.rpc_url.as_deref(), Some("https://rpc.example.com"));
        assert!(net.network_passphrase.is_none());
    }

    #[test]
    fn full_file_config_parses_all_sections() {
        let _env = lock_env();
        let f = tmp_toml(
            r#"
[network]
rpc_url = "https://rpc.example.com"
network_passphrase = "My Network ; 2024"

[contract]
default_contract_id = "CABC123"

[auth]
secret_key = "SXXX"

[defaults]
token = "USDC"
frequency = "weekly"
"#,
        );
        let cfg = load_project_config_from(tmp_path(&f))
            .expect("ok")
            .expect("some");
        assert_eq!(
            cfg.network.as_ref().unwrap().rpc_url.as_deref(),
            Some("https://rpc.example.com")
        );
        assert_eq!(
            cfg.network.unwrap().network_passphrase.as_deref(),
            Some("My Network ; 2024")
        );
        assert_eq!(
            cfg.contract.unwrap().default_contract_id.as_deref(),
            Some("CABC123")
        );
        assert_eq!(cfg.auth.unwrap().secret_key.as_deref(), Some("SXXX"));
        let d = cfg.defaults.unwrap();
        assert_eq!(d.token.as_deref(), Some("USDC"));
        assert_eq!(d.frequency.as_deref(), Some("weekly"));
    }

    // ── TOML-only (AC #1 from the issue) ─────────────────────────────────

    #[test]
    fn toml_only_rpc_url_resolved() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[network]
rpc_url = "https://toml-rpc.example.com""#);
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.network.rpc_url, "https://toml-rpc.example.com");
    }

    #[test]
    fn toml_only_contract_id_resolved() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[contract]
default_contract_id = "CTOML123""#);
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.contract.default_contract_id.as_deref(), Some("CTOML123"));
    }

    #[test]
    fn toml_only_secret_key_resolved() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[auth]
secret_key = "STOMLKEY""#);
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.auth.secret_key.as_deref(), Some("STOMLKEY"));
    }

    #[test]
    fn toml_only_frequency_resolved() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[defaults]
frequency = "weekly""#);
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.defaults.frequency, "weekly");
    }

    #[test]
    fn toml_only_token_resolved() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[defaults]
token = "USDC""#);
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.defaults.token.as_deref(), Some("USDC"));
    }

    // ── Env-only (AC #2 from the issue) ──────────────────────────────────

    #[test]
    fn env_only_rpc_url_resolved() {
        let _env = lock_env();
        clear_all_env();
        std::env::set_var(env_keys::RPC_URL, "https://env-rpc.example.com");
        let f = tmp_toml("");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.network.rpc_url, "https://env-rpc.example.com");
    }

    #[test]
    fn env_only_contract_id_resolved() {
        let _env = lock_env();
        clear_all_env();
        std::env::set_var(env_keys::CONTRACT_ID, "CENV999");
        let f = tmp_toml("");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.contract.default_contract_id.as_deref(), Some("CENV999"));
    }

    #[test]
    fn env_only_secret_key_resolved() {
        let _env = lock_env();
        clear_all_env();
        std::env::set_var(env_keys::SECRET_KEY, "SENVKEY");
        let f = tmp_toml("");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.auth.secret_key.as_deref(), Some("SENVKEY"));
    }

    #[test]
    fn env_only_frequency_resolved() {
        let _env = lock_env();
        clear_all_env();
        std::env::set_var(env_keys::FREQUENCY, "quarterly");
        let f = tmp_toml("");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.defaults.frequency, "quarterly");
    }

    // ── Env overrides TOML (AC #2 — env var wins over same key in TOML) ──

    #[test]
    fn env_overrides_toml_rpc_url() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[network]
rpc_url = "https://toml-rpc.example.com""#);
        std::env::set_var(env_keys::RPC_URL, "https://env-rpc.example.com");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(
            cfg.network.rpc_url, "https://env-rpc.example.com",
            "env var must win over TOML"
        );
    }

    #[test]
    fn env_overrides_toml_contract_id() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[contract]
default_contract_id = "CTOML""#);
        std::env::set_var(env_keys::CONTRACT_ID, "CENV");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.contract.default_contract_id.as_deref(), Some("CENV"));
    }

    #[test]
    fn env_overrides_toml_secret_key() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[auth]
secret_key = "STOML""#);
        std::env::set_var(env_keys::SECRET_KEY, "SENV");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.auth.secret_key.as_deref(), Some("SENV"));
    }

    #[test]
    fn env_overrides_toml_frequency() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[defaults]
frequency = "monthly""#);
        std::env::set_var(env_keys::FREQUENCY, "annually");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.defaults.frequency, "annually");
    }

    // ── CLI flag overrides env and TOML ───────────────────────────────────

    #[test]
    fn cli_overrides_env_and_toml_rpc_url() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[network]
rpc_url = "https://toml-rpc.example.com""#);
        std::env::set_var(env_keys::RPC_URL, "https://env-rpc.example.com");
        let mut cli = Config::default();
        cli.network.rpc_url = "https://cli-rpc.example.com".to_string();
        let cfg = resolve_config_with_project_file(Some(&cli), tmp_path(&f)).expect("resolve");
        assert_eq!(
            cfg.network.rpc_url, "https://cli-rpc.example.com",
            "CLI flag must win over both env and TOML"
        );
    }

    #[test]
    fn cli_overrides_env_and_toml_secret_key() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml(r#"[auth]
secret_key = "STOML""#);
        std::env::set_var(env_keys::SECRET_KEY, "SENV");
        let mut cli = Config::default();
        cli.auth.secret_key = Some("SCLI".to_string());
        let cfg = resolve_config_with_project_file(Some(&cli), tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.auth.secret_key.as_deref(), Some("SCLI"));
    }

    // ── Malformed TOML (AC #3 from the issue) ────────────────────────────

    #[test]
    fn malformed_toml_resolve_returns_error_not_panic() {
        let _env = lock_env();
        let f = tmp_toml("[network\nbad syntax !!!");
        let result = resolve_config_with_project_file(None, tmp_path(&f));
        assert!(result.is_err(), "expected Err for malformed TOML");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Malformed TOML"),
            "error message must mention 'Malformed TOML', got: {msg}"
        );
    }

    #[test]
    fn wrong_type_in_toml_returns_error() {
        let _env = lock_env();
        // frequency must be a string; an integer is a type mismatch.
        let f = tmp_toml(r#"[defaults]
frequency = 999"#);
        let result = resolve_config_with_project_file(None, tmp_path(&f));
        assert!(result.is_err(), "type mismatch must produce Err");
    }

    // ── Built-in default fallback ─────────────────────────────────────────

    #[test]
    fn absent_file_and_no_env_uses_defaults() {
        let _env = lock_env();
        clear_all_env();
        let cfg = resolve_config_with_project_file(
            None,
            "/tmp/stellopay_no_file_xyz_abc.toml",
        )
        .expect("resolve");
        let defaults = Config::default();
        assert_eq!(cfg.network.rpc_url, defaults.network.rpc_url);
        assert_eq!(cfg.network.network_passphrase, defaults.network.network_passphrase);
        assert_eq!(cfg.defaults.frequency, defaults.defaults.frequency);
    }

    #[test]
    fn empty_toml_and_no_env_uses_defaults() {
        let _env = lock_env();
        clear_all_env();
        let f = tmp_toml("");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        let defaults = Config::default();
        assert_eq!(cfg.network.rpc_url, defaults.network.rpc_url);
        assert_eq!(cfg.defaults.frequency, defaults.defaults.frequency);
    }

    // ── Mixed sources ─────────────────────────────────────────────────────

    #[test]
    fn toml_fills_gaps_not_covered_by_env() {
        let _env = lock_env();
        clear_all_env();
        // env provides rpc_url; TOML provides contract_id — both appear.
        let f = tmp_toml(r#"[contract]
default_contract_id = "CGAPFILL""#);
        std::env::set_var(env_keys::RPC_URL, "https://env-rpc.example.com");
        let cfg = resolve_config_with_project_file(None, tmp_path(&f)).expect("resolve");
        assert_eq!(cfg.network.rpc_url, "https://env-rpc.example.com");
        assert_eq!(cfg.contract.default_contract_id.as_deref(), Some("CGAPFILL"));
    }

    // ── get_secret_key helper ─────────────────────────────────────────────

    #[test]
    fn get_secret_key_prefers_env_over_config_struct() {
        let _env = lock_env();
        clear_all_env();
        let mut cfg = Config::default();
        cfg.auth.secret_key = Some("SCFG".to_string());
        std::env::set_var(env_keys::SECRET_KEY, "SENV_KEY");
        let key = get_secret_key(&cfg).expect("key");
        assert_eq!(key, "SENV_KEY");
    }

    #[test]
    fn get_secret_key_falls_back_to_config_struct() {
        let _env = lock_env();
        clear_all_env();
        let mut cfg = Config::default();
        cfg.auth.secret_key = Some("SCFG_ONLY".to_string());
        let key = get_secret_key(&cfg).expect("key");
        assert_eq!(key, "SCFG_ONLY");
    }

    #[test]
    fn get_secret_key_returns_err_when_nowhere() {
        let _env = lock_env();
        clear_all_env();
        let cfg = Config::default(); // secret_key is None
        let result = get_secret_key(&cfg);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No secret key"));
    }
}
