use fd_lock::RwLock;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    InProgress,
    Done,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStatus::InProgress => write!(f, "in-progress"),
            SessionStatus::Done => write!(f, "done"),
        }
    }
}

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
    pub status: SessionStatus,
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
    // Total user + assistant messages with extractable text
    #[serde(default)]
    pub message_count: u32,
    // Duration from first to last timestamped record, in seconds
    #[serde(default)]
    pub duration_secs: Option<i64>,
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
            .filter(|s| s.status == SessionStatus::InProgress && !s.manually_done)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(id: &str, status: SessionStatus, mtime: i64) -> SessionEntry {
        SessionEntry {
            path: format!("/tmp/{}.jsonl", id),
            session_id: id.to_string(),
            provider: "claude-code".to_string(),
            status,
            file_modified_at: mtime,
            last_scanned_at: mtime,
            summary: "test summary".to_string(),
            left_off: "test left off".to_string(),
            cwd: Some("/home/user/project".to_string()),
            continuation: false,
            last_prompt: None,
            manually_done: false,
            flagged: false,
            custom_title: None,
            file_size_bytes: 1000,
            turn_count: 5,
            message_count: 10,
            duration_secs: Some(300),
        }
    }

    // ── SessionStatus serde ─────────────────────────────────────────────

    #[test]
    fn status_serializes_to_kebab_case() {
        assert_eq!(serde_json::to_string(&SessionStatus::InProgress).unwrap(), "\"in-progress\"");
        assert_eq!(serde_json::to_string(&SessionStatus::Done).unwrap(), "\"done\"");
    }

    #[test]
    fn status_deserializes_from_kebab_case() {
        let ip: SessionStatus = serde_json::from_str("\"in-progress\"").unwrap();
        assert_eq!(ip, SessionStatus::InProgress);
        let done: SessionStatus = serde_json::from_str("\"done\"").unwrap();
        assert_eq!(done, SessionStatus::Done);
    }

    #[test]
    fn status_display() {
        assert_eq!(SessionStatus::InProgress.to_string(), "in-progress");
        assert_eq!(SessionStatus::Done.to_string(), "done");
    }

    #[test]
    fn status_invalid_deserialize_errors() {
        let result = serde_json::from_str::<SessionStatus>("\"maybe\"");
        assert!(result.is_err());
    }

    // ── Index save/load round-trip ──────────────────────────────────────

    #[test]
    fn save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");

        let mut index = Index::default();
        index.sessions.push(make_session("abc", SessionStatus::InProgress, 1000));
        index.sessions.push(make_session("def", SessionStatus::Done, 2000));
        index.save(&path).unwrap();

        let loaded = Index::load(&path).unwrap();
        assert_eq!(loaded.sessions.len(), 2);
        assert_eq!(loaded.sessions[0].session_id, "abc");
        assert_eq!(loaded.sessions[0].status, SessionStatus::InProgress);
        assert_eq!(loaded.sessions[1].session_id, "def");
        assert_eq!(loaded.sessions[1].status, SessionStatus::Done);
    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let path = PathBuf::from("/tmp/nonexistent_wip_test_index.json");
        let index = Index::load(&path).unwrap();
        assert!(index.sessions.is_empty());
    }

    // ── upsert ──────────────────────────────────────────────────────────

    #[test]
    fn upsert_inserts_new() {
        let mut index = Index::default();
        index.upsert(make_session("abc", SessionStatus::InProgress, 1000));
        assert_eq!(index.sessions.len(), 1);
        assert_eq!(index.sessions[0].session_id, "abc");
    }

    #[test]
    fn upsert_updates_existing_by_path() {
        let mut index = Index::default();
        index.upsert(make_session("abc", SessionStatus::InProgress, 1000));

        let mut updated = make_session("abc", SessionStatus::Done, 2000);
        updated.summary = "updated summary".to_string();
        index.upsert(updated);

        assert_eq!(index.sessions.len(), 1);
        assert_eq!(index.sessions[0].status, SessionStatus::Done);
        assert_eq!(index.sessions[0].summary, "updated summary");
    }

    #[test]
    fn upsert_preserves_flagged_and_manually_done() {
        let mut index = Index::default();
        let mut s = make_session("abc", SessionStatus::InProgress, 1000);
        s.flagged = true;
        s.manually_done = true;
        index.sessions.push(s);

        let updated = make_session("abc", SessionStatus::InProgress, 2000);
        index.upsert(updated);

        assert!(index.sessions[0].flagged);
        assert!(index.sessions[0].manually_done);
    }

    // ── mark_manually_done ──────────────────────────────────────────────

    #[test]
    fn mark_manually_done_sets_flag() {
        let mut index = Index::default();
        index.sessions.push(make_session("abc", SessionStatus::InProgress, 1000));
        index.mark_manually_done("abc");
        assert!(index.sessions[0].manually_done);
    }

    #[test]
    fn mark_manually_done_no_match_is_noop() {
        let mut index = Index::default();
        index.sessions.push(make_session("abc", SessionStatus::InProgress, 1000));
        index.mark_manually_done("xyz");
        assert!(!index.sessions[0].manually_done);
    }

    // ── toggle_flagged ──────────────────────────────────────────────────

    #[test]
    fn toggle_flagged_on_off() {
        let mut index = Index::default();
        index.sessions.push(make_session("abc", SessionStatus::InProgress, 1000));

        index.toggle_flagged("abc");
        assert!(index.sessions[0].flagged);

        index.toggle_flagged("abc");
        assert!(!index.sessions[0].flagged);
    }

    // ── all_sessions ────────────────────────────────────────────────────

    #[test]
    fn all_sessions_sorted_by_recency() {
        let mut index = Index::default();
        index.sessions.push(make_session("old", SessionStatus::InProgress, 1000));
        index.sessions.push(make_session("new", SessionStatus::InProgress, 3000));
        index.sessions.push(make_session("mid", SessionStatus::InProgress, 2000));

        let all = index.all_sessions();
        assert_eq!(all[0].session_id, "new");
        assert_eq!(all[1].session_id, "mid");
        assert_eq!(all[2].session_id, "old");
    }

    #[test]
    fn all_sessions_includes_done() {
        let mut index = Index::default();
        index.sessions.push(make_session("ip", SessionStatus::InProgress, 1000));
        index.sessions.push(make_session("done", SessionStatus::Done, 2000));

        let all = index.all_sessions();
        assert_eq!(all.len(), 2);
    }

    // ── in_progress_sessions ────────────────────────────────────────────

    #[test]
    fn in_progress_filters_done() {
        let mut index = Index::default();
        index.sessions.push(make_session("ip", SessionStatus::InProgress, 1000));
        index.sessions.push(make_session("done", SessionStatus::Done, 2000));

        let result = index.in_progress_sessions();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].session_id, "ip");
    }

    #[test]
    fn in_progress_filters_manually_done() {
        let mut index = Index::default();
        let mut s = make_session("ip", SessionStatus::InProgress, 1000);
        s.manually_done = true;
        index.sessions.push(s);

        let result = index.in_progress_sessions();
        assert!(result.is_empty());
    }

    #[test]
    fn in_progress_sorted_by_recency() {
        let mut index = Index::default();
        index.sessions.push(make_session("old", SessionStatus::InProgress, 1000));
        index.sessions.push(make_session("new", SessionStatus::InProgress, 3000));

        let result = index.in_progress_sessions();
        assert_eq!(result[0].session_id, "new");
        assert_eq!(result[1].session_id, "old");
    }

    #[test]
    fn in_progress_deduplicates_continuations_by_cwd() {
        let mut index = Index::default();

        let mut newer = make_session("newer", SessionStatus::InProgress, 2000);
        newer.continuation = true;
        newer.cwd = Some("/project".to_string());
        index.sessions.push(newer);

        let mut older = make_session("older", SessionStatus::InProgress, 1000);
        older.continuation = true;
        older.cwd = Some("/project".to_string());
        index.sessions.push(older);

        let result = index.in_progress_sessions();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].session_id, "newer");
    }

    #[test]
    fn in_progress_non_continuations_always_shown() {
        let mut index = Index::default();

        let s1 = make_session("s1", SessionStatus::InProgress, 2000);
        index.sessions.push(s1);

        let s2 = make_session("s2", SessionStatus::InProgress, 1000);
        index.sessions.push(s2);

        let result = index.in_progress_sessions();
        assert_eq!(result.len(), 2);
    }
}
