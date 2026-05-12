use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub scan: ScanConfig,
    #[serde(default)]
    pub resume_command: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SummaryBackend {
    #[default]
    Anthropic,
    Vertex,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ScanConfig {
    #[serde(default = "default_model")]
    pub summary_model: String,
    // Not needed for the Vertex backend (credentials come from ADC)
    #[serde(default)]
    pub summary_api_key: Option<KeychainEntry>,
    #[serde(default)]
    pub summary_backend: SummaryBackend,
    // Required when summary_backend is "vertex"
    #[serde(default)]
    pub vertex_project_id: Option<String>,
    // Vertex region, e.g. "us-east5". Defaults to "us-east5" if not set.
    #[serde(default)]
    pub vertex_region: Option<String>,
    // LLM prompt template; empty means use the built-in default
    #[serde(default)]
    pub summary_prompt: String,
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

fn default_model() -> String {
    "claude-sonnet-4-6".to_string()
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".wip/config.json")
}

impl Config {
    /// Returns the full argv for resuming a session, e.g.
    /// `["roachdev", "claude", "--", "--resume", "<id>"]`.
    pub fn resume_argv(&self, session_id: &str) -> Vec<String> {
        let raw = self.resume_command.as_deref().unwrap_or("claude");
        let mut argv: Vec<String> = raw.split_whitespace().map(String::from).collect();
        argv.push("--resume".to_string());
        argv.push(session_id.to_string());
        argv
    }

    /// Builds a Command for resuming a session.
    pub fn resume_cmd(&self, session_id: &str) -> std::process::Command {
        let argv = self.resume_argv(session_id);
        let mut cmd = std::process::Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd
    }

    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(config_path())?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_serde_round_trip() {
        let json = serde_json::to_string(&SummaryBackend::Anthropic).unwrap();
        assert_eq!(json, "\"anthropic\"");
        let back: SummaryBackend = serde_json::from_str(&json).unwrap();
        assert_eq!(back, SummaryBackend::Anthropic);

        let json = serde_json::to_string(&SummaryBackend::Vertex).unwrap();
        assert_eq!(json, "\"vertex\"");
        let back: SummaryBackend = serde_json::from_str(&json).unwrap();
        assert_eq!(back, SummaryBackend::Vertex);
    }

    #[test]
    fn config_save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        let config = Config {
            scan: ScanConfig {
                summary_backend: SummaryBackend::Vertex,
                summary_model: "claude-sonnet-4-6".to_string(),
                vertex_project_id: Some("my-project".to_string()),
                vertex_region: Some("us-east5".to_string()),
                ..Default::default()
            },
            resume_command: None,
        };
        config.save(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: Config = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.scan.summary_backend, SummaryBackend::Vertex);
        assert_eq!(loaded.scan.vertex_project_id.as_deref(), Some("my-project"));
    }

    #[test]
    fn config_defaults_applied() {
        let json = r#"{"scan": {}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.scan.summary_model, "claude-sonnet-4-6");
        assert_eq!(config.scan.summary_backend, SummaryBackend::Anthropic);
        assert!(config.scan.vertex_project_id.is_none());
        assert!(config.scan.pricing.is_none());
    }

    #[test]
    fn config_with_pricing() {
        let json = r#"{"scan": {"pricing": {"input_tokens_per_million": 3.0, "output_tokens_per_million": 15.0}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let pricing = config.scan.pricing.unwrap();
        assert!((pricing.input_tokens_per_million - 3.0).abs() < f64::EPSILON);
        assert!((pricing.output_tokens_per_million - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn config_backward_compat_with_old_fields() {
        // Old config files may have providers/storage_dir/index_refresh_threshold
        // — serde should ignore unknown fields gracefully
        let json = r#"{
            "scan": {},
            "providers": {},
            "storage_dir": "~/.wip",
            "index_refresh_threshold": 3600
        }"#;
        let result: Result<Config, _> = serde_json::from_str(json);
        // By default serde ignores unknown fields, so this should work
        assert!(result.is_ok());
    }
}
