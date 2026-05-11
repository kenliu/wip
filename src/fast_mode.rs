// Fast mode: fzf-powered session picker for keyboard-optimized selection.
// Requires fzf to be installed (`brew install fzf`).

use crate::index::{index_path, Index};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn format_age(file_modified_at: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let secs = (now - file_modified_at).max(0);
    if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        // Count by chars not bytes to avoid splitting multi-byte UTF-8 characters
        s.chars().take(max.saturating_sub(3)).collect::<String>() + "..."
    }
}

fn project_name(cwd: &str) -> String {
    std::path::Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    if Command::new("fzf").arg("--version").output().is_err() {
        return Err("fzf is not installed. Install it with: brew install fzf".into());
    }

    let index = Index::load(&index_path())?;
    let sessions: Vec<_> = index
        .in_progress_sessions()
        .into_iter()
        .filter(|s| !s.session_id.starts_with("agent-"))
        .collect();

    if sessions.is_empty() {
        eprintln!("No in-progress sessions found. Run 'wip scan' first.");
        return Ok(());
    }

    // Each line sent to fzf is tab-delimited: session_id TAB cwd TAB display_line
    // fzf only shows field 3 (--with-nth=3); fields 1 and 2 are invisible to the
    // user but returned in the selected output so we can extract them after selection
    let lines: Vec<String> = sessions
        .iter()
        .map(|s| {
            let age = format_age(s.file_modified_at);
            let cwd = s.cwd.as_deref().unwrap_or("");
            let project = truncate(&project_name(cwd), 20);
            let summary = truncate(&s.summary, 55);
            let left_off = truncate(&s.left_off, 50);
            format!(
                "{}\t{}\t{:<22} {:<57} {:>8}  ↩ {}",
                s.session_id, cwd, project, summary, age, left_off
            )
        })
        .collect();

    let input = lines.join("\n");

    let mut fzf = Command::new("fzf")
        .args([
            "--delimiter=\t",
            "--with-nth=3", // only display field 3; fields 1+2 are hidden metadata
            "--no-sort",    // preserve recency order from the index
            "--reverse",    // most recent at the top
            "--prompt=wip > ",
            "--height=100%",
            "--info=hidden",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    fzf.stdin.take().unwrap().write_all(input.as_bytes())?;

    let output = fzf.wait_with_output()?;

    // fzf exits with non-zero status when the user cancels (Escape or ctrl-c)
    if !output.status.success() {
        return Ok(());
    }

    let selected = String::from_utf8(output.stdout)?;
    let mut fields = selected.splitn(3, '\t');
    let session_id = fields.next().unwrap_or("").trim().to_string();
    let cwd = fields.next().unwrap_or("").trim().to_string();

    if session_id.is_empty() {
        return Err("Could not parse selected session".into());
    }

    // Clear the terminal before handing off so claude starts with a clean screen
    print!("\x1B[2J\x1B[1;1H");
    std::io::stdout().flush()?;

    // exec() replaces this process with claude entirely — no wip process remains.
    // current_dir() sets the working directory for claude, not the parent shell
    // (see GitHub issue #2 for shell integration to solve the cwd-after-exit problem)
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("claude");
    cmd.arg("--resume").arg(&session_id);
    if !cwd.is_empty() {
        cmd.current_dir(&cwd);
    }
    let err = cmd.exec();

    Err(format!("Failed to launch claude: {}", err).into())
}
