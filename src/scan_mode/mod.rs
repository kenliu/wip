// Scan mode: discovers Claude Code session files, summarizes new/modified ones
// with an LLM, updates the index, and prunes stale entries. Designed to run
// unattended (e.g. via cron) and exit silently.

pub mod jsonl_parser;
pub mod lm_summarizer;

use crate::config::{config_path, Config, KeychainEntry, ScanConfig, SummaryBackend};
use crate::index::{acquire_lock, index_path, Index, SessionEntry, SessionStatus};
use lm_summarizer::SummarizerConfig;
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

/// Prompts for an Anthropic API key and stores it in the system keychain,
/// creating or updating ~/.wip/config.json to point at the keychain entry.
pub async fn run_auth() -> Result<(), Box<dyn std::error::Error>> {
    let key_input = prompt("Anthropic API key (sk-ant-...): ")?;
    let api_key = key_input.trim().to_string();
    if api_key.is_empty() {
        return Err("API key is required.".into());
    }

    crate::keychain::set_password("anthropic_api_key", &api_key).await
        .map_err(|e| format!("Failed to store key in system keychain: {}", e))?;
    eprintln!("Stored in system keychain.");

    let path = config_path();
    let mut config = if path.exists() {
        Config::load().map_err(|e| format!("Failed to load {}: {}", path.display(), e))?
    } else {
        Config {
            scan: ScanConfig {
                summary_backend: SummaryBackend::Anthropic,
                summary_model: "claude-sonnet-4-6".to_string(),
                ..Default::default()
            },
            providers: std::collections::HashMap::new(),
            storage_dir: "~/.wip".to_string(),
            index_refresh_threshold: 3600,
        }
    };

    config.scan.summary_api_key = Some(KeychainEntry { keychain_key: "anthropic_api_key".to_string() });
    config.save(&path)?;
    eprintln!("Updated {}.", path.display());

    Ok(())
}

pub async fn run(force: bool, silent: bool) -> Result<(), Box<dyn std::error::Error>> {
    let summarizer_config = build_summarizer_config().await?;

    let _lock = acquire_lock()?;
    let path = index_path();
    let mut index = Index::load(&path)?;
    let now = now();

    let files: Vec<_> = glob::glob(&session_glob())
        .expect("Invalid glob pattern")
        .filter_map(|r| r.ok())
        .collect();

    let mut summaries_run = 0u32;
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

        // Don't summarize very old sessions — they're unlikely to be returned to
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

        let continuation = jsonl_parser::is_continuation_session(&file);

        let context = match jsonl_parser::parse_and_extract(&file) {
            Ok(c) => c,
            // "No messages found" means the session was created but never used — skip silently
            Err(e) if e.to_string() == "No messages found" => continue,
            Err(e) => {
                if !silent { eprintln!("  Parse error {}: {}", session_id, e); }
                continue;
            }
        };

        if !silent { eprintln!("Summarizing: {}", session_id); }

        let result = match lm_summarizer::summarize(&context, &summarizer_config).await {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                // 4xx errors indicate a configuration problem (bad key, wrong model,
                // wrong project) that will fail identically for every session — abort
                // rather than spamming the same error for every remaining file.
                if is_fatal_api_error(&msg) {
                    return Err(format!("Scan aborted: {}", msg).into());
                }
                if !silent { eprintln!("  Summary error: {}", e); }
                continue;
            }
        };

        input_tokens_total += result.input_tokens;
        output_tokens_total += result.output_tokens;
        summaries_run += 1;

        let file_size_bytes = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);

        index.upsert(SessionEntry {
            path: path_str,
            session_id,
            provider: "claude-code".to_string(),
            status: result.status,
            file_modified_at: file_mtime,
            last_scanned_at: now,
            summary: result.summary,
            left_off: result.left_off,
            cwd: context.cwd,
            continuation,
            last_prompt: context.last_prompt,
            manually_done: false,
            flagged: false,
            custom_title: context.custom_title,
            file_size_bytes,
            turn_count: context.turn_count,
            message_count: context.message_count,
            duration_secs: context.duration_secs,
        });
    }

    // Prune index entries that are no longer valid. This keeps the index from
    // accumulating stale sessions over time.
    let before = index.sessions.len();
    index.sessions.retain(|s| {
        if s.session_id.starts_with("agent-") { return false; }
        // Files that no longer exist can't be resumed regardless of flag state
        if !std::path::Path::new(&s.path).exists() { return false; }
        // Flagged sessions are kept indefinitely — the user explicitly marked them
        if s.flagged { return true; }
        if now - s.file_modified_at > MAX_AGE_DAYS * 86400 { return false; }
        true
    });
    let pruned = before - index.sessions.len();

    let in_progress = index.sessions.iter().filter(|s| s.status == SessionStatus::InProgress).count();
    let done = index.sessions.iter().filter(|s| s.status == SessionStatus::Done).count();

    index.save(&index_path())?;

    let total_tokens = input_tokens_total + output_tokens_total;
    let log_entry = serde_json::json!({
        "unix_ts": now,
        "summaries_run": summaries_run,
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
    if !silent { eprintln!("{}", log_entry); }

    Ok(())
}

/// Builds the summarizer config, running an interactive setup wizard to create
/// ~/.wip/config.json if it doesn't exist and stdin is a terminal.
async fn build_summarizer_config() -> Result<SummarizerConfig, Box<dyn std::error::Error>> {
    let path = config_path();

    if !path.exists() {
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            run_setup_wizard(&path).await?;
        } else {
            // Non-interactive (cron, pipe): fall back to env var or show setup guide
            return match std::env::var("ANTHROPIC_API_KEY") {
                Ok(api_key) if !api_key.is_empty() => Ok(SummarizerConfig::Anthropic {
                    api_key,
                    model: "claude-sonnet-4-6".to_string(),
                }),
                _ => Err(setup_guide().into()),
            };
        }
    }

    let config = Config::load()
        .map_err(|e| format!("Failed to load {}: {}", path.display(), e))?;

    let model = config.scan.summary_model.clone();
    match config.scan.summary_backend {
        SummaryBackend::Vertex => {
            let project_id = config
                .scan
                .vertex_project_id
                .ok_or("vertex backend requires vertex_project_id in config")?;
            let region = config
                .scan
                .vertex_region
                .unwrap_or_else(|| "us-east5".to_string());
            Ok(SummarizerConfig::Vertex { project_id, region, model })
        }
        SummaryBackend::Anthropic => {
            // Try the keychain first (if configured), then fall back to env var
            let api_key = resolve_anthropic_api_key(config.scan.summary_api_key.as_ref()).await;
            if api_key.is_empty() {
                return Err("wip: Anthropic API key not found.\n\n\
                            Run `wip scan` in a terminal to configure it, or set:\n\
                            \n  export ANTHROPIC_API_KEY=sk-ant-...".into());
            }
            Ok(SummarizerConfig::Anthropic { api_key, model })
        }
    }
}

/// Returns the API key from the keychain entry if configured, falling back to the env var.
async fn resolve_anthropic_api_key(entry: Option<&KeychainEntry>) -> String {
    if let Some(e) = entry {
        match crate::keychain::get_password(&e.keychain_key).await {
            Ok(key) if !key.is_empty() => return key,
            Ok(_) => eprintln!("wip: keychain entry '{}' is empty, falling back to env var", e.keychain_key),
            Err(err) => eprintln!("wip: keychain lookup failed ({}), falling back to env var", err),
        }
    }
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_default()
}

/// Prompts the user for configuration, writes ~/.wip/config.json, and prints
/// next-step instructions. Returns an error only if the wizard itself fails.
async fn run_setup_wizard(config_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("wip is not configured. Let's set up your Claude API connection.\n");
    eprintln!("Which backend would you like to use?");
    eprintln!("  1) Anthropic API key");
    eprintln!("  2) Google Cloud Vertex AI");

    let choice = prompt("Choice [1]: ")?;
    let use_vertex = choice.trim() == "2";

    let scan = if use_vertex {
        let project_input = prompt("GCP project ID: ")?;
        let project_id = project_input.trim().to_string();
        if project_id.is_empty() {
            return Err("GCP project ID is required for the Vertex backend.".into());
        }
        let region_input = prompt("Region [us-east5]: ")?;
        let region = if region_input.trim().is_empty() {
            "us-east5".to_string()
        } else {
            region_input.trim().to_string()
        };
        ScanConfig {
            summary_backend: SummaryBackend::Vertex,
            // Explicitly set the model — Default::default() for String is "",
            // not the serde default, so we must populate it here.
            summary_model: "claude-sonnet-4-6".to_string(),
            vertex_project_id: Some(project_id),
            vertex_region: Some(region),
            ..Default::default()
        }
    } else {
        let key_input = prompt("Anthropic API key (sk-ant-...): ")?;
        let api_key = key_input.trim().to_string();
        if api_key.is_empty() {
            return Err("API key is required for the Anthropic backend.".into());
        }

        let store_input = prompt("Store in system keychain? [Y/n]: ")?;
        let store_in_keychain = !matches!(store_input.trim().to_lowercase().as_str(), "n" | "no");

        let summary_api_key = if store_in_keychain {
            match crate::keychain::set_password("anthropic_api_key", &api_key).await {
                Ok(()) => {
                    eprintln!("Stored in system keychain.");
                    Some(KeychainEntry { keychain_key: "anthropic_api_key".to_string() })
                }
                Err(e) => {
                    eprintln!("Warning: could not store in keychain: {}.", e);
                    eprintln!("Set ANTHROPIC_API_KEY in your shell profile instead.");
                    None
                }
            }
        } else {
            eprintln!("Set ANTHROPIC_API_KEY in your shell profile to authenticate.");
            None
        };

        ScanConfig {
            summary_backend: SummaryBackend::Anthropic,
            summary_model: "claude-sonnet-4-6".to_string(),
            summary_api_key,
            ..Default::default()
        }
    };

    let config = Config { scan, resume_command: None };
    config.save(&config_path.to_path_buf())?;

    eprintln!("\nCreated {}.", config_path.display());

    if use_vertex {
        eprintln!("Using model: claude-sonnet-4-6 (edit summary_model in config to change)");
        eprintln!("If you haven't already, authenticate with GCP:");
        eprintln!("  gcloud auth application-default login");
    }
    eprintln!();

    Ok(())
}

fn prompt(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::{BufRead, Write};
    print!("{}", text);
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

fn setup_guide() -> &'static str {
    "wip is not configured. Set up a Claude API connection to get started:\n\
     \n  Option 1 — Anthropic API key (simplest):\n\
     \n    export ANTHROPIC_API_KEY=sk-ant-...\n\
     \n    Add to ~/.zshrc or ~/.bashrc to make it permanent.\n\
     \n  Option 2 — Google Cloud Vertex AI:\n\
     \n    Run `wip scan` in a terminal to be guided through setup.\n\
     \nThen run: wip scan"
}

/// Returns true for API errors that will affect every session and should abort
/// the scan rather than be logged per-file. 4xx = client/config error,
/// 5xx = server unavailable — both are systemic, not file-specific.
fn is_fatal_api_error(msg: &str) -> bool {
    msg.contains("API error 4") || msg.contains("API error 5")
}
