// Scan mode: discovers Claude Code session files, assesses new/modified ones
// with an LLM, updates the index, and prunes stale entries. Designed to run
// unattended (e.g. via cron) and exit silently.

pub mod jsonl_parser;
pub mod lm_assessment;

use crate::index::{index_path, Index, SessionEntry};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

fn session_glob() -> String {
    let home = dirs::home_dir().expect("Could not find home directory");
    home.join(".claude/projects/**/*.jsonl")
        .to_string_lossy()
        .to_string()
}

fn log_path() -> std::path::PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".wip")
        .join("scan.log.jsonl")
}

fn mtime(path: &std::path::Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64)
        .unwrap_or(0)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn timestamp_str() -> String {
    // Manual date formatting to avoid adding a chrono dependency just for logging.
    // This is approximate (ignores leap years/days) but good enough for a log file.
    let now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let s = now % 60;
    let m = (now / 60) % 60;
    let h = (now / 3600) % 24;
    let days = now / 86400;
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", year, month, day, h, m, s)
}

fn append_log(msg: &str) {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Errors here are silently ignored — log failures shouldn't abort a scan
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", msg);
    }
}

// These will move to config.json once configuration is implemented
const MAX_AGE_DAYS: i64 = 30;
const MIN_AGE_SECS: i64 = 30;

pub async fn run(force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY environment variable not set")?;

    let path = index_path();
    let mut index = Index::load(&path)?;
    let now = now();

    let files: Vec<_> = glob::glob(&session_glob())
        .expect("Invalid glob pattern")
        .filter_map(|r| r.ok())
        .collect();

    let mut assessments_run = 0u32;
    let mut input_tokens_total = 0u32;
    let mut output_tokens_total = 0u32;

    for file in files {
        let path_str = file.to_string_lossy().to_string();
        let file_mtime = mtime(&file);

        // Claude Code creates agent-* files for subagent sessions spawned during
        // a conversation. These are internal and not useful to show to the user.
        let stem = file.file_stem().unwrap_or_default().to_string_lossy();
        if stem.starts_with("agent-") {
            continue;
        }

        // Don't assess very old sessions — they're unlikely to be returned to
        if now - file_mtime > MAX_AGE_DAYS * 86400 {
            continue;
        }

        // Skip files modified very recently — they may still be actively written to
        if now - file_mtime < MIN_AGE_SECS {
            continue;
        }

        // Skip files that haven't changed since last scan to avoid redundant API calls
        if !force {
            if let Some(existing) = index.sessions.iter().find(|s| s.path == path_str) {
                if existing.file_modified_at >= file_mtime {
                    continue;
                }
            }
        }

        let session_id = file.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        eprintln!("Assessing: {}", session_id);

        let context = match jsonl_parser::parse_and_extract(&file) {
            Ok(c) => c,
            Err(e) => { eprintln!("  Parse error: {}", e); continue; }
        };

        let assessment = match lm_assessment::assess(&context, &api_key).await {
            Ok(a) => a,
            Err(e) => { eprintln!("  Assessment error: {}", e); continue; }
        };

        input_tokens_total += assessment.input_tokens;
        output_tokens_total += assessment.output_tokens;
        assessments_run += 1;

        index.upsert(SessionEntry {
            path: path_str,
            session_id,
            provider: "claude-code".to_string(),
            status: assessment.status,
            file_modified_at: file_mtime,
            last_scanned_at: now,
            summary: assessment.summary,
            left_off: assessment.left_off,
            cwd: context.cwd,
        });
    }

    // Prune index entries that are no longer valid. This keeps the index from
    // accumulating stale sessions over time.
    let before = index.sessions.len();
    index.sessions.retain(|s| {
        if s.session_id.starts_with("agent-") { return false; }
        if now - s.file_modified_at > MAX_AGE_DAYS * 86400 { return false; }
        // Remove sessions whose files have been deleted
        if !std::path::Path::new(&s.path).exists() { return false; }
        true
    });
    let pruned = before - index.sessions.len();

    let in_progress = index.sessions.iter().filter(|s| s.status == "in-progress").count();
    let done = index.sessions.iter().filter(|s| s.status == "done").count();

    index.save(&index_path())?;

    let total_tokens = input_tokens_total + output_tokens_total;
    let log_entry = serde_json::json!({
        "timestamp": timestamp_str(),
        "assessments_run": assessments_run,
        "in_progress": in_progress,
        "done": done,
        "pruned": pruned,
        "tokens": {
            "total": total_tokens,
            "input": input_tokens_total,
            "output": output_tokens_total,
        }
    });
    append_log(&log_entry.to_string());
    eprintln!("{}", log_entry);

    Ok(())
}
