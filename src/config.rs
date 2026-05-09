use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub providers: HashMap<String, ProviderConfig>,
    pub scan: ScanConfig,
    #[serde(default = "default_storage_dir")]
    pub storage_dir: String,
    #[serde(default = "default_refresh_threshold")]
    pub index_refresh_threshold: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    pub session_patterns: Vec<String>,
    pub cli_launcher: CliLauncher,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CliLauncher {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScanConfig {
    pub assessment_model: String,
    pub assessment_api_key: KeychainEntry,
    pub assessment_prompt: String,
    #[serde(default)]
    pub pricing: Option<Pricing>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KeychainEntry {
    pub keychain_key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Pricing {
    pub input_tokens_per_million: f64,
    pub output_tokens_per_million: f64,
}

fn default_storage_dir() -> String {
    "~/.wip".to_string()
}

fn default_refresh_threshold() -> u64 {
    3600
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = PathBuf::from("~/.wip/config.json").canonicalize()?;
        let content = std::fs::read_to_string(config_path)?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
