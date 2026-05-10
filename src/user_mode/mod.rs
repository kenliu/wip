// User mode: loads the pre-computed index and presents an interactive TUI for
// session browsing and resumption. The index is always shown instantly from disk;
// no scanning or network access happens here.

mod tui;

use crate::index::{index_path, Index};

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let index = Index::load(&index_path())?;
    let sessions: Vec<_> = index
        .in_progress_sessions()
        .into_iter()
        // Guard against agent sessions that may have slipped into older index files
        .filter(|s| !s.session_id.starts_with("agent-"))
        .collect();

    if sessions.is_empty() {
        eprintln!("No in-progress sessions found. Run 'wip scan' first.");
        return Ok(());
    }

    tui::run(&sessions)
}
