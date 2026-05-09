use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Index {
    pub sessions: Vec<SessionEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionEntry {
    pub path: String,
    pub session_id: String,
    pub provider: String,
    pub status: String, // "in-progress" or "done"
    pub file_modified_at: i64,
    pub last_scanned_at: i64,
    pub summary: String,
    pub left_off: String,
    pub cwd: Option<String>,
}

pub fn index_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".wip")
        .join("index.json")
}

impl Index {
    pub fn load(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(Index::default());
        }
        let content = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn in_progress_sessions(&self) -> Vec<&SessionEntry> {
        let mut sessions: Vec<&SessionEntry> = self.sessions
            .iter()
            .filter(|s| s.status == "in-progress")
            .collect();
        sessions.sort_by(|a, b| b.file_modified_at.cmp(&a.file_modified_at));
        sessions
    }

    pub fn upsert(&mut self, session: SessionEntry) {
        if let Some(pos) = self.sessions.iter().position(|s| s.path == session.path) {
            self.sessions[pos] = session;
        } else {
            self.sessions.push(session);
        }
    }
}
