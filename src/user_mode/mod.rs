// User mode: loads the pre-computed index and presents an interactive TUI for
// session browsing and resumption. The index is always shown instantly from disk;
// no scanning or network access happens here.

mod tui;

use crate::index::{index_path, Index};
use crate::scan_mode;
use std::io::Write;
use tui::{TuiAction, UiState};

fn ui_state_path() -> std::path::PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".wip")
        .join("ui_state.json")
}

fn load_ui_state() -> UiState {
    let path = ui_state_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_ui_state(state: &UiState) {
    let path = ui_state_path();
    if let Ok(json) = serde_json::to_string(state) {
        let _ = std::fs::write(path, json);
    }
}

pub async fn run(background_scan: bool) -> Result<(), Box<dyn std::error::Error>> {
    if background_scan {
        tokio::spawn(async {
            // Errors are silently dropped — stderr would corrupt the TUI display,
            // and per-session errors are already written to ~/.wip/scan.log.jsonl
            let _ = scan_mode::run(false, true).await;
        });
    }

    let path = index_path();
    let index = Index::load(&path)?;

    let sessions: Vec<_> = index
        .all_sessions()
        .into_iter()
        .filter(|s| !s.session_id.starts_with("agent-"))
        .cloned()
        .collect();

    if sessions.is_empty() {
        eprintln!("No sessions found. Run 'wip scan' first.");
        return Ok(());
    }

    let ui_state = load_ui_state();
    let (action, ui_state) = tui::run(sessions, 0, &path, ui_state)?;
    save_ui_state(&ui_state);

    match action {
        TuiAction::Quit => Ok(()),
        TuiAction::Resume { session_id, cwd } => {
            print!("\x1B[2J\x1B[1;1H");
            std::io::stdout().flush()?;

            // exec() replaces this process entirely — no wip process remains in the process table
            use std::os::unix::process::CommandExt;
            let mut cmd = std::process::Command::new("claude");
            cmd.arg("--resume").arg(&session_id);
            if let Some(ref cwd) = cwd {
                if !cwd.is_empty() {
                    cmd.current_dir(cwd);
                }
            }
            Err(format!("Failed to launch claude: {}", cmd.exec()).into())
        }
    }
}
