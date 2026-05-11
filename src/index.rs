use fd_lock::RwLock;
use serde::{Deserialize, Serialize};
use std::fs::File;
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
    // True for sessions that were started via queue-operation (automated continuations).
    // These are chained sessions where only the latest per cwd is worth showing.
    #[serde(default)]
    pub continuation: bool,
    // The last thing the user typed, from the `last-prompt` record in the JSONL file
    #[serde(default)]
    pub last_prompt: Option<String>,
    // Set by the user pressing 'x' in the TUI — suppresses this session from all views
    #[serde(default)]
    pub manually_done: bool,
    // Set by the user pressing 'f' in the TUI — shows a flag indicator next to the session
    #[serde(default)]
    pub flagged: bool,
    // User-assigned name from the `/rename` command (`custom-title` JSONL record)
    #[serde(default)]
    pub custom_title: Option<String>,
    // Size of the JSONL session file in bytes at last scan
    #[serde(default)]
    pub file_size_bytes: u64,
    // Number of user turns in the session
    #[serde(default)]
    pub turn_count: u32,
}

pub fn index_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".wip")
        .join("index.json")
}

fn lock_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".wip")
        .join("index.lock")
}

/// Acquires an exclusive advisory lock on the index lock file.
/// Returns the lock guard — drop it to release.
pub fn acquire_lock() -> Result<fd_lock::RwLockWriteGuard<'static, File>, Box<dyn std::error::Error>> {
    let path = lock_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(&path)?;
    // SAFETY: we box the RwLock and leak it so the guard's lifetime is 'static.
    // The lock file lives for the duration of the process anyway.
    let lock = Box::new(RwLock::new(file));
    let lock_ref: &'static mut RwLock<File> = Box::leak(lock);
    Ok(lock_ref.write()?)
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

    pub fn mark_manually_done(&mut self, session_id: &str) {
        if let Some(entry) = self.sessions.iter_mut().find(|s| s.session_id == session_id) {
            entry.manually_done = true;
        }
    }

    pub fn toggle_flagged(&mut self, session_id: &str) {
        if let Some(entry) = self.sessions.iter_mut().find(|s| s.session_id == session_id) {
            entry.flagged = !entry.flagged;
        }
    }

    pub fn all_sessions(&self) -> Vec<&SessionEntry> {
        let mut sessions: Vec<&SessionEntry> = self.sessions.iter().collect();
        sessions.sort_by(|a, b| b.file_modified_at.cmp(&a.file_modified_at));
        sessions
    }

    pub fn in_progress_sessions(&self) -> Vec<&SessionEntry> {
        let mut sessions: Vec<&SessionEntry> = self.sessions
            .iter()
            .filter(|s| s.status == "in-progress" && !s.manually_done)
            .collect();
        // Most recently modified first — these are the sessions the user is most
        // likely to want to return to
        sessions.sort_by(|a, b| b.file_modified_at.cmp(&a.file_modified_at));

        // For continuation chains, only show the most recent session per cwd.
        // Since we sorted by recency, the first continuation we see for a given
        // cwd is the latest — subsequent ones are older steps in the same chain.
        let mut seen_continuation_cwds = std::collections::HashSet::new();
        sessions.retain(|s| {
            if s.continuation {
                seen_continuation_cwds.insert(s.cwd.as_deref().unwrap_or("").to_string())
            } else {
                true
            }
        });
        sessions
    }

    pub fn upsert(&mut self, session: SessionEntry) {
        if let Some(pos) = self.sessions.iter().position(|s| s.path == session.path) {
            let existing = &self.sessions[pos];
            // Preserve fields that the user sets interactively — the scanner never owns these
            let flagged = existing.flagged;
            let manually_done = existing.manually_done;
            self.sessions[pos] = SessionEntry { flagged, manually_done, ..session };
        } else {
            self.sessions.push(session);
        }
    }
}
