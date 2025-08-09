use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use reqwest::Client;
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
pub struct Employee {
    pub address: String,
    pub name: String,
    pub email: String,
    pub department: String,
    pub salary: i128,
    pub currency: String,
    pub frequency: String,
    pub start_date: DateTime<Utc>,
}

pub fn format_stellar_amount(amount: i128) -> String {
    // Stellar uses 7 decimal places
    let decimal_amount = amount as f64 / 10_000_000.0;
    format!("{:.7}", decimal_amount)
}

pub fn parse_stellar_amount(amount_str: &str) -> Result<i128> {
    let amount: f64 = amount_str.parse()?;
    Ok((amount * 10_000_000.0) as i128)
}

pub fn validate_stellar_address(address: &str) -> bool {
    // Basic validation - Stellar addresses start with 'G' and are 56 characters long
    address.starts_with('G') && address.len() == 56
}

pub fn validate_contract_address(address: &str) -> bool {
    // Basic validation - Contract addresses start with 'C' and are 56 characters long
    address.starts_with('C') && address.len() == 56
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActivityMetrics {
    pub transactions: u64,
    pub volume: HashMap<String, i128>,
    pub unique_users: u64,
    pub errors: u64,
}

pub fn format_amount(amount: i128, decimals: u32) -> String {
    let divisor = 10_i128.pow(decimals);
    let whole = amount / divisor;
    let fractional = amount % divisor;

    if fractional == 0 {
        whole.to_string()
    } else {
        // Format with full precision, then remove trailing zeros
        let formatted = format!(
            "{}.{:0width$}",
            whole,
            fractional,
            width = decimals as usize
        );
        formatted.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub fn parse_amount(amount_str: &str, decimals: u32) -> Result<i128> {
    let parts: Vec<&str> = amount_str.split('.').collect();

    match parts.len() {
        1 => {
            // No decimal point, treat as whole number
            let whole: i128 = parts[0].parse()?;
            Ok(whole * 10_i128.pow(decimals))
        }
        2 => {
            // Has decimal point
            let whole: i128 = parts[0].parse()?;
            let fractional_str = parts[1];

            if fractional_str.len() > decimals as usize {
                return Err(anyhow::anyhow!("Too many decimal places"));
            }

            let fractional: i128 = fractional_str.parse()?;
            let fractional_scaled =
                fractional * 10_i128.pow(decimals - fractional_str.len() as u32);

            Ok(whole * 10_i128.pow(decimals) + fractional_scaled)
        }
        _ => Err(anyhow::anyhow!("Invalid amount format")),
    }
}

pub fn format_duration(seconds: u64) -> String {
    let days = seconds / (24 * 60 * 60);
    let hours = (seconds % (24 * 60 * 60)) / (60 * 60);
    let minutes = (seconds % (60 * 60)) / 60;
    let secs = seconds % 60;

    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, minutes, secs)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

pub fn parse_duration(duration_str: &str) -> Result<u64> {
    let duration_str = duration_str.to_lowercase();

    if duration_str.ends_with("s") {
        let num_str = &duration_str[..duration_str.len() - 1];
        let seconds: u64 = num_str.parse()?;
        Ok(seconds)
    } else if duration_str.ends_with("m") {
        let num_str = &duration_str[..duration_str.len() - 1];
        let minutes: u64 = num_str.parse()?;
        Ok(minutes * 60)
    } else if duration_str.ends_with("h") {
        let num_str = &duration_str[..duration_str.len() - 1];
        let hours: u64 = num_str.parse()?;
        Ok(hours * 60 * 60)
    } else if duration_str.ends_with("d") {
        let num_str = &duration_str[..duration_str.len() - 1];
        let days: u64 = num_str.parse()?;
        Ok(days * 24 * 60 * 60)
    } else {
        // Try to parse as seconds
        let seconds: u64 = duration_str.parse()?;
        Ok(seconds)
    }
}

pub fn validate_address(address: &str) -> Result<()> {
    if address.is_empty() {
        return Err(anyhow::anyhow!("Address cannot be empty"));
    }

    if address.len() != 56 {
        return Err(anyhow::anyhow!("Address must be 56 characters long"));
    }

    if !address
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err(anyhow::anyhow!(
            "Address must contain only uppercase letters and digits"
        ));
    }

    Ok(())
}

pub fn truncate_address(address: &str, chars: usize) -> String {
    if address.len() <= chars * 2 {
        return address.to_string();
    }

    format!(
        "{}...{}",
        &address[..chars],
        &address[address.len() - chars..]
    )
}

pub fn format_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "No data to display".to_string();
    }

    // Calculate column widths
    let mut widths = headers.iter().map(|h| h.len()).collect::<Vec<_>>();

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    let mut result = String::new();

    // Header
    result.push_str("┌");
    for (i, width) in widths.iter().enumerate() {
        result.push_str(&"─".repeat(width + 2));
        if i < widths.len() - 1 {
            result.push_str("┬");
        }
    }
    result.push_str("┐\n");

    // Header row
    result.push_str("│");
    for (i, (header, width)) in headers.iter().zip(widths.iter()).enumerate() {
        result.push_str(&format!(" {:<width$} ", header, width = width));
        if i < widths.len() - 1 {
            result.push_str("│");
        }
    }
    result.push_str("│\n");

    // Header separator
    result.push_str("├");
    for (i, width) in widths.iter().enumerate() {
        result.push_str(&"─".repeat(width + 2));
        if i < widths.len() - 1 {
            result.push_str("┼");
        }
    }
    result.push_str("┤\n");

    // Data rows
    for row in rows {
        result.push_str("│");
        for (i, (cell, width)) in row.iter().zip(widths.iter()).enumerate() {
            result.push_str(&format!(" {:<width$} ", cell, width = width));
            if i < widths.len() - 1 {
                result.push_str("│");
            }
        }
        result.push_str("│\n");
    }

    // Bottom border
    result.push_str("└");
    for (i, width) in widths.iter().enumerate() {
        result.push_str(&"─".repeat(width + 2));
        if i < widths.len() - 1 {
            result.push_str("┴");
        }
    }
    result.push_str("┘");

    result
}

pub fn colorize_status(status: &str) -> String {
    match status.to_lowercase().as_str() {
        "active" | "success" | "paid" | "healthy" => {
            format!("\x1b[32m{}\x1b[0m", status) // Green
        }
        "inactive" | "failed" | "error" | "unhealthy" => {
            format!("\x1b[31m{}\x1b[0m", status) // Red
        }
        "pending" | "processing" | "warning" => {
            format!("\x1b[33m{}\x1b[0m", status) // Yellow
        }
        "paused" | "disabled" => {
            format!("\x1b[90m{}\x1b[0m", status) // Gray
        }
        _ => status.to_string(),
    }
}

pub fn format_percentage(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

pub fn format_gas(gas: u64) -> String {
    if gas >= 1_000_000 {
        format!("{:.1}M", gas as f64 / 1_000_000.0)
    } else if gas >= 1_000 {
        format!("{:.1}K", gas as f64 / 1_000.0)
    } else {
        gas.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_amount() {
        assert_eq!(format_amount(1000000000, 7), "100");
        assert_eq!(format_amount(1500000000, 7), "150");
        assert_eq!(format_amount(1234567890, 7), "123.456789");
    }

    #[test]
    fn test_parse_amount() {
        assert_eq!(parse_amount("100", 7).unwrap(), 1000000000);
        assert_eq!(parse_amount("150.5", 7).unwrap(), 1505000000);
        assert_eq!(parse_amount("123.456789", 7).unwrap(), 1234567890);
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), 30);
        assert_eq!(parse_duration("5m").unwrap(), 300);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        assert_eq!(parse_duration("1d").unwrap(), 86400);
    }

    #[test]
    fn test_validate_address() {
        assert!(
            validate_address("GCKFBEIYTKP6RCZEKMGL2QAPLGKUBGE5UAHRQJRXGCQHKPQM6CHCM4K4").is_ok()
        );
        assert!(validate_address("invalid").is_err());
        assert!(validate_address("").is_err());
    }

    #[test]
    fn test_truncate_address() {
        let addr = "GCKFBEIYTKP6RCZEKMGL2QAPLGKUBGE5UAHRQJRXGCQHKPQM6CHCM4K4";
        assert_eq!(truncate_address(addr, 4), "GCKF...M4K4");
        assert_eq!(truncate_address("SHORT", 4), "SHORT");
    }
}
pub struct SorobanHttpClient{
    base_url:String,
    client:reqwest::Client,
}
impl SorobanHttpClient{
    pub fn new(base_url: &str)->Self{
        Self{
            base_url:base_url.to_string(),
            client:reqwest::Client::new(),
        }
    }
    pub async fn get_ledger_info(&self)->Result<String>{
        let url=format!("{}/ledger",self.base_url);
        let res=self.client.get(&url).send().await?;
        let body =res.text().await?;
        Ok(body)
    }
    pub async fn invoke(
        &self,
        contract_id:&str,
        method:&str,
        args:Vec<(&str,&str)>,
        signer:&str,
    ) -> Result<String> {
        let url=format!("{}/invoke",self.base_url.trim_end_matches('/'));
        println!("Invoking Soroban at: {}",url);
        let payload=json!({
            "contract_id":contract_id,
            "method":method,
            "args":args.iter().map(|(k,v)| json!({(*k):v})).collect::<Vec<_>>(),
            "signer":signer,
        });
        let response=self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await?;
        let body=response.text().await?;
        Ok(body)
    }
}