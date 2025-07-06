use stellopay_cli::Config;
use anyhow::Result;
use std::path::Path;
use tokio::fs;

pub async fn load_config(config_path: &Path) -> Result<Config> {
    // Expand tilde in path
    let expanded_path = if config_path.starts_with("~") {
        let home_dir = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        let path_str = config_path.to_string_lossy();
        let without_tilde = &path_str[1..]; // Remove the ~
        home_dir.join(without_tilde.trim_start_matches('/'))
    } else {
        config_path.to_path_buf()
    };
    
    if !expanded_path.exists() {
        // Create default config if it doesn't exist
        let default_config = Config::default();
        create_config_file(&expanded_path, &default_config).await?;
        return Ok(default_config);
    }
    
    let config_content = fs::read_to_string(&expanded_path).await?;
    let config: Config = toml::from_str(&config_content)?;
    
    Ok(config)
}

async fn create_config_file(path: &Path, config: &Config) -> Result<()> {
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
