pub mod jsonl_parser;
pub mod lm_assessment;

use crate::index::{index_path, Index, SessionEntry};
use std::time::{SystemTime, UNIX_EPOCH};

fn session_glob() -> String {
    let home = dirs::home_dir().expect("Could not find home directory");
    home.join(".claude/projects/**/*.jsonl")
        .to_string_lossy()
        .to_string()
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


// Hardcoded limits — will be configurable settings in future
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

    eprintln!("Found {} session files", files.len());

    for file in files {
        let path_str = file.to_string_lossy().to_string();
        let file_mtime = mtime(&file);

        // Skip agent subagent sessions
        let stem = file.file_stem().unwrap_or_default().to_string_lossy();
        if stem.starts_with("agent-") {
            continue;
        }

        // Skip files older than max age (will be configurable)
        if now - file_mtime > MAX_AGE_DAYS * 86400 {
            continue;
        }

        // Skip files modified less than 5 minutes ago (may still be written)
        if now - file_mtime < MIN_AGE_SECS {
            continue;
        }

        // Skip unchanged files unless forced
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

    // Prune stale entries from index
    let before = index.sessions.len();
    index.sessions.retain(|s| {
        // Remove agent sessions
        if s.session_id.starts_with("agent-") {
            return false;
        }
        // Remove sessions older than max age
        if now - s.file_modified_at > MAX_AGE_DAYS * 86400 {
            return false;
        }
        // Remove sessions whose file no longer exists
        if !std::path::Path::new(&s.path).exists() {
            return false;
        }
        true
    });
    let pruned = before - index.sessions.len();
    if pruned > 0 {
        eprintln!("Pruned {} stale entries from index.", pruned);
    }

    index.save(&index_path())?;
    eprintln!("Index saved.");
    Ok(())
}
