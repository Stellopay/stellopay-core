use crate::Config;
use anyhow::Result;
use std::path::Path;
use tokio::fs;

pub async fn load_config(config_path: &Path) -> Result<Config> {
    // Expand tilde in path
    let expanded_path = if config_path.starts_with("~") {
        let home_dir =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        let path_str = config_path.to_string_lossy();
        let without_tilde = &path_str[1..]; // Remove the ~
        home_dir.join(without_tilde.trim_start_matches('/'))
    } else {
        config_path.to_path_buf()
    };

    if !expanded_path.exists() {
        // Create default config if it doesn't exist
        let mut default_config = Config::default();
        create_config_file(&expanded_path, &default_config).await?;
        apply_env_overrides(&mut default_config);
        return Ok(default_config);
    }

    let config_content = fs::read_to_string(&expanded_path).await?;
    let mut config: Config = toml::from_str(&config_content)?;

    // Precedence: CLI flag > environment variable > TOML file > built-in
    // default. CLI flags are resolved per-command; here we layer environment
    // variables on top of the TOML file so an env override always wins over
    // the file (see `apply_env_overrides`).
    apply_env_overrides(&mut config);

    Ok(config)
}

pub async fn create_config_file(path: &Path, config: &Config) -> Result<()> {
    // Create parent directory if it doesn't exist
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let config_content = toml::to_string_pretty(config)?;
    fs::write(path, config_content).await?;

    println!("Created default config file at: {}", path.display());

    Ok(())
}

pub fn get_secret_key(config: &Config) -> Result<String> {
    // Check environment variable first
    if let Ok(key) = std::env::var("STELLOPAY_SECRET_KEY") {
        return Ok(key);
    }

    // Check config file
    if let Some(secret_key) = &config.auth.secret_key {
        return Ok(secret_key.clone());
    }

    Err(anyhow::anyhow!("No secret key found. Set STELLOPAY_SECRET_KEY environment variable or add it to config file"))
}

/// Layer environment variables on top of a config already loaded from TOML.
///
/// Precedence (highest first): CLI flag > environment variable > TOML file >
/// built-in default. This function implements the "environment variable"
/// layer: any `STELLOPAY_*` variable that is set overrides the corresponding
/// key loaded from the TOML file. Missing variables leave the TOML/default
/// value untouched, so a project-local `stellopay.toml` still works untouched
/// when no env var is present.
pub fn apply_env_overrides(config: &mut Config) {
    if let Ok(v) = std::env::var("STELLOPAY_RPC_URL") {
        config.network.rpc_url = v;
    }
    if let Ok(v) = std::env::var("STELLOPAY_NETWORK_PASSPHRASE") {
        config.network.network_passphrase = v;
    }
    if let Ok(v) = std::env::var("STELLOPAY_CONTRACT_ID") {
        config.contract.default_contract_id = Some(v);
    }
    if let Ok(v) = std::env::var("STELLOPAY_SECRET_KEY") {
        config.auth.secret_key = Some(v);
    }
    if let Ok(v) = std::env::var("STELLOPAY_DEFAULT_TOKEN") {
        config.defaults.token = Some(v);
    }
    if let Ok(v) = std::env::var("STELLOPAY_DEFAULT_FREQUENCY") {
        config.defaults.frequency = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn write_temp_toml(dir: &std::path::Path, content: &str) -> std::path::PathBuf {
        let p = dir.join("stellopay-test.toml");
        std::fs::write(&p, content).unwrap();
        p
    }

    const SAMPLE_TOML: &str = "rpc_url = \"from-toml\"\nnetwork_passphrase = \"P\"\n[contract]\ndefault_contract_id = \"C123\"\n[auth]\nsecret_key = \"S\"\n[defaults]\ntoken = \"T\"\nfrequency = \"weekly\"\n";

    #[tokio::test]
    async fn toml_only_resolves_without_env() {
        let tmp = tempfile::tempdir().unwrap();
        let p = write_temp_toml(tmp.path(), SAMPLE_TOML);
        env::remove_var("STELLOPAY_RPC_URL");
        let cfg = load_config(&p).await.unwrap();
        assert_eq!(cfg.network.rpc_url, "from-toml");
    }

    #[tokio::test]
    async fn env_var_overrides_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let p = write_temp_toml(tmp.path(), SAMPLE_TOML);
        env::set_var("STELLOPAY_RPC_URL", "from-env");
        let cfg = load_config(&p).await.unwrap();
        assert_eq!(cfg.network.rpc_url, "from-env");
        env::remove_var("STELLOPAY_RPC_URL");
    }

    #[tokio::test]
    async fn malformed_toml_returns_error_not_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let p = write_temp_toml(tmp.path(), "this = is = not = valid toml ==\n");
        let res = load_config(&p).await;
        assert!(
            res.is_err(),
            "malformed TOML should surface as Err, not panic"
        );
    }
}

