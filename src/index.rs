use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Index {
    pub sessions: Vec<SessionEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionEntry {
    pub path: String,
    // UUID filename stem, passed to `claude --resume` to resume the session
    pub session_id: String,
    pub provider: String,
    pub status: String, // "in-progress" or "done"
    // Unix timestamps — i64 to match filesystem mtime values
    pub file_modified_at: i64,
    pub last_scanned_at: i64,
    // LLM-generated fields from summarization
    pub summary: String,
    pub left_off: String,
    // Working directory at the time the session was active; used to cd before resuming
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
        // Atomic write: write to a temp file then rename to avoid corrupting the index
        // if the process crashes mid-write
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
        // Most recently modified first — these are the sessions the user is most
        // likely to want to return to
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
