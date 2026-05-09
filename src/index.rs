use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Index {
    pub sessions: Vec<SessionEntry>,
    pub last_full_scan: Option<String>,
    pub token_usage_stats: TokenUsageStats,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionEntry {
    pub path: String,
    pub provider: String,
    pub display_name: String,
    pub status: String, // "in-progress" or "done"
    pub file_modified_at: i64,
    pub last_scanned_at: i64,
    pub summary: String,
    pub left_off: String,
    pub cli_launcher: String,
    pub assessment: AssessmentResult,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssessmentResult {
    pub tokens_used: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TokenUsageStats {
    pub total_tokens_used: u32,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub assessments_run: u32,
    pub assessments_skipped: u32,
    pub estimated_cost: f64,
}

impl Index {
    pub fn load(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(Index {
                sessions: Vec::new(),
                last_full_scan: None,
                token_usage_stats: TokenUsageStats::default(),
            });
        }

        let content = std::fs::read_to_string(path)?;
        let index = serde_json::from_str(&content)?;
        Ok(index)
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn get_in_progress_sessions(&self) -> Vec<SessionEntry> {
        self.sessions
            .iter()
            .filter(|s| s.status == "in-progress")
            .cloned()
            .collect()
    }

    pub fn update_or_add_session(&mut self, session: SessionEntry) {
        if let Some(pos) = self.sessions.iter().position(|s| s.path == session.path) {
            self.sessions[pos] = session;
        } else {
            self.sessions.push(session);
        }
    }
}
