// User mode: loads the pre-computed index and presents an interactive TUI for
// session browsing and resumption. The index is always shown instantly from disk;
// no scanning or network access happens here.

mod tui;

use crate::index::{index_path, Index};
use crate::scan_mode;
use std::io::Write;
use tui::TuiAction;

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
        .in_progress_sessions()
        .into_iter()
        .filter(|s| !s.session_id.starts_with("agent-"))
        .cloned()
        .collect();

    if sessions.is_empty() {
        eprintln!("No in-progress sessions found. Run 'wip scan' first.");
        return Ok(());
    }

    match tui::run(sessions, 0, &path)? {
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
