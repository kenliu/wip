use crate::config::Config;
use crate::index::{acquire_lock, Index, SessionEntry, SessionStatus};
use crate::util::{format_age, project_name};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UiState {
    pub show_preview: bool,
    pub show_all: bool,
    pub flagged_only: bool,
}

pub enum TuiAction {
    Resume { session_id: String, cwd: Option<String> },
    Quit,
}

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use serde_json::Value;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

fn false_positives_path() -> std::path::PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".wip")
        .join("false_positives.json")
}

// Appends a session to the false positives log for later prompt analysis.
fn record_false_positive(session: &SessionEntry) {
    let path = false_positives_path();
    let mut entries: Vec<Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    entries.push(serde_json::json!({
        "session_id": session.session_id,
        "path": session.path,
        "status": session.status,
        "summary": session.summary,
        "left_off": session.left_off,
        "flagged_at": now,
    }));

    if let Ok(json) = serde_json::to_string_pretty(&entries) {
        let _ = std::fs::write(&path, json);
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}M", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{}K", bytes / 1_024)
    } else {
        format!("{}B", bytes)
    }
}

// ── Chat preview helpers ──────────────────────────────────────────────────────

fn load_preview(path: &str) -> Vec<(String, String)> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return vec![] };

    // Session files can be many MB — only read the tail
    const TAIL_BYTES: u64 = 65_536;
    let file_len = file.seek(SeekFrom::End(0)).unwrap_or(0);
    let start = file_len.saturating_sub(TAIL_BYTES);
    let _ = file.seek(SeekFrom::Start(start));

    let mut buf = String::new();
    let _ = file.read_to_string(&mut buf);

    let mut messages = Vec::new();
    let mut lines = buf.lines();

    // If we seeked into the middle of the file, the first line is likely a
    // partial JSON record — skip it
    if start > 0 { lines.next(); }

    for line in lines {
        let line = line.trim();
        if line.is_empty() { continue; }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("user") => {
                if let Some(text) = extract_preview_user(&v) {
                    messages.push(("user".to_string(), text));
                }
            }
            Some("assistant") => {
                if let Some(text) = extract_preview_assistant(&v) {
                    messages.push(("assistant".to_string(), text));
                }
            }
            _ => {}
        }
    }
    messages
}

fn extract_preview_user(v: &Value) -> Option<String> {
    let content = v.get("message")?.get("content")?;
    match content {
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() { return None; }
            // Skip meta/system injections (local-command-caveat, system-reminder, etc.)
            if s.starts_with('<') { return None; }
            Some(s.to_string())
        }
        Value::Array(arr) => {
            let text: String = arr.iter()
                .filter_map(|item| {
                    if item.get("type")?.as_str()? == "text" {
                        let t = item.get("text")?.as_str()?.trim();
                        if t.is_empty() || t.starts_with('<') { return None; }
                        Some(t.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            if text.is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}

fn extract_preview_assistant(v: &Value) -> Option<String> {
    // Assistant content is an array; only text blocks are human-readable.
    // Thinking and tool_use blocks are skipped — too verbose for a preview.
    let arr = v.get("message")?.get("content")?.as_array()?;
    let text: String = arr.iter()
        .filter_map(|item| {
            if item.get("type")?.as_str()? == "text" {
                let t = item.get("text")?.as_str()?.trim();
                if t.is_empty() { return None; }
                Some(t.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.is_empty() { None } else { Some(text) }
}

// Greedy word-wrap: splits `text` into lines of at most `width` chars.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 { return vec![]; }
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            result.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if line.is_empty() {
                line = word.to_string();
            } else if line.len() + 1 + word.len() <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                result.push(line);
                line = word.to_string();
            }
        }
        if !line.is_empty() {
            result.push(line);
        }
    }
    result
}

// ── App ───────────────────────────────────────────────────────────────────────

struct App {
    sessions: Vec<SessionEntry>,
    index_path: std::path::PathBuf,
    selected: usize,
    filter: String,
    // When true, keyboard input goes to the filter string instead of navigation
    filter_mode: bool,
    // When true, only flagged sessions are shown
    flagged_only: bool,
    // When true, done sessions are included in the list
    show_all: bool,
    // When true, the chat preview pane is shown (toggled with →/←)
    show_preview: bool,
    // Keyed by session_id; populated lazily on first render of each session
    preview_cache: HashMap<String, Vec<(String, String)>>,
    // Transient error message shown in the footer, cleared on next keypress
    error_message: Option<String>,
}

impl App {
    fn new(sessions: Vec<SessionEntry>, index_path: std::path::PathBuf, initial_selected: usize, ui_state: UiState) -> Self {
        let selected = initial_selected.min(sessions.len().saturating_sub(1));
        Self {
            sessions,
            index_path,
            selected,
            filter: String::new(),
            filter_mode: false,
            flagged_only: ui_state.flagged_only,
            show_all: ui_state.show_all,
            show_preview: ui_state.show_preview,
            preview_cache: HashMap::new(),
            error_message: None,
        }
    }

    fn is_done(s: &SessionEntry) -> bool {
        s.status == SessionStatus::Done || s.manually_done
    }

    // Returns sessions matching the current filter and flagged_only/show_all modes.
    fn filtered(&self) -> Vec<&SessionEntry> {
        let q = self.filter.to_lowercase();
        self.sessions
            .iter()
            .filter(|s| {
                if !self.show_all && Self::is_done(s) { return false; }
                if self.flagged_only && !s.flagged { return false; }
                if !q.is_empty() {
                    let proj = project_name(s.cwd.as_deref().unwrap_or(""));
                    let title = s.custom_title.as_deref().unwrap_or("");
                    if !proj.to_lowercase().contains(&q) && !title.to_lowercase().contains(&q) { return false; }
                }
                true
            })
            .collect()
    }

    // Toggle flag on the selected session in-memory and persist to disk.
    fn handle_flag(&mut self, session_id: &str) {
        if let Some(s) = self.sessions.iter_mut().find(|s| s.session_id == session_id) {
            s.flagged = !s.flagged;
        }
        if let Ok(_lock) = acquire_lock() {
            if let Ok(mut index) = Index::load(&self.index_path) {
                index.toggle_flagged(session_id);
                let _ = index.save(&self.index_path);
            }
        }
        self.clamp_selected();
    }

    // Mark the selected session done in-memory and persist to disk.
    fn handle_mark_done(&mut self, session_id: &str) {
        self.sessions.retain(|s| s.session_id != session_id);
        if let Ok(_lock) = acquire_lock() {
            if let Ok(mut index) = Index::load(&self.index_path) {
                index.mark_manually_done(session_id);
                let _ = index.save(&self.index_path);
            }
        }
        self.clamp_selected();
    }

    fn filtered_count(&self) -> usize {
        self.filtered().len()
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.filtered_count() {
            self.selected += 1;
        }
    }

    // Keeps selected within the bounds of the current filtered list
    fn clamp_selected(&mut self) {
        let count = self.filtered_count();
        if count == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(count - 1);
        }
    }

    // Returns the index of the first session to render so the selected row stays visible.
    // Each session slot is 6 lines: header + summary + left_off + session_id + blank.
    fn scroll_offset(&self, list_height: u16) -> usize {
        let per_page = ((list_height as usize) / 6).max(1);
        self.selected.saturating_sub(per_page.saturating_sub(1))
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();

        // Header and footer span the full width; panes share the middle
        let [header_area, content_area, footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ]).areas(area);

        self.render_header(frame, header_area);

        if self.show_preview && content_area.width >= 80 {
            let [left_area, right_area] = Layout::horizontal([
                Constraint::Percentage(55),
                Constraint::Percentage(45),
            ]).areas(content_area);
            self.render_session_list(frame, left_area);
            self.render_preview(frame, right_area);
        } else {
            self.render_session_list(frame, content_area);
        }

        self.render_footer(frame, footer_area);
    }

    fn render_header(&self, frame: &mut ratatui::Frame, area: Rect) {
        let filtered = self.filtered();
        let count = filtered.len();
        let position = if count == 0 { 0 } else { self.selected + 1 };
        let header_text = if self.flagged_only {
            format!("  WIP: 🚩 FLAGGED  ({}/{})", position, count)
        } else if self.show_all && self.filter.is_empty() {
            format!("  WIP: ALL SESSIONS  ({}/{})", position, count)
        } else if self.show_all {
            format!("  WIP: ALL SESSIONS  ({}/{} of {})", position, count, self.sessions.len())
        } else if self.filter.is_empty() {
            format!("  WIP: IN-PROGRESS SESSIONS  ({}/{})", position, count)
        } else {
            format!(
                "  WIP: IN-PROGRESS SESSIONS  ({}/{} of {})",
                position,
                count,
                self.sessions.len()
            )
        };

        let bg = Color::Rgb(40, 44, 52);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                header_text,
                Style::default().fg(Color::Cyan).bg(bg).add_modifier(Modifier::BOLD),
            )))
            .style(Style::default().bg(bg)),
            area,
        );
    }

    fn render_session_list(&self, frame: &mut ratatui::Frame, area: Rect) {
        let filtered = self.filtered();

        // ── Session list ─────────────────────────────────────────────────────
        let list_height = area.height;
        let offset = self.scroll_offset(list_height);
        let max_width = area.width as usize;

        let mut lines: Vec<Line> = Vec::new();

        if filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  no sessions match \"{}\"", self.filter),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for (i, session) in filtered.iter().enumerate().skip(offset) {
                if lines.len() >= list_height as usize {
                    break;
                }

                let is_selected = i == self.selected;
                let is_done = Self::is_done(session);
                let (row_style, dim_style) = if is_selected {
                    (
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                        Style::default().fg(Color::Cyan),
                    )
                } else if is_done {
                    (
                        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
                        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
                    )
                } else {
                    (Style::default(), Style::default().fg(Color::DarkGray))
                };

                let cursor = if is_selected { "▶" } else { " " };
                let proj = project_name(session.cwd.as_deref().unwrap_or(""));
                let age = format_age(session.file_modified_at);
                let size = format_size(session.file_size_bytes);

                // Row 1: cursor + project name (strikethrough if done) + age + size + turns
                let proj_style = if is_done {
                    row_style.add_modifier(Modifier::CROSSED_OUT)
                } else {
                    row_style
                };
                let done_prefix = if is_done { "✓ " } else { "" };
                lines.push(Line::from(vec![
                    Span::styled(format!("{} {}", cursor, done_prefix), row_style),
                    Span::styled(format!("{:<21}", proj), proj_style),
                    Span::styled(format!("{:<10}", age), dim_style),
                    Span::styled(format!("{:<7}", size), dim_style),
                    Span::styled(format!("{}t", session.turn_count), dim_style),
                ]));

                // Row 2: flag + optional custom title badge + topic summary
                if lines.len() < list_height as usize {
                    let flag_str = if session.flagged { "🚩 " } else { "" };
                    let indent = format!("  {}", flag_str);

                    if let Some(title) = &session.custom_title {
                        let badge = format!(" {} ", title);
                        let badge_style = if is_done {
                            row_style.add_modifier(Modifier::REVERSED).add_modifier(Modifier::CROSSED_OUT)
                        } else {
                            row_style.add_modifier(Modifier::REVERSED)
                        };
                        let used = indent.chars().count() + badge.chars().count() + 1;
                        let summary: String = session.summary.chars()
                            .take(max_width.saturating_sub(used))
                            .collect();
                        lines.push(Line::from(vec![
                            Span::raw(indent),
                            Span::styled(badge, badge_style),
                            Span::styled(format!(" {}", summary), row_style),
                        ]));
                    } else {
                        let text = format!("{}{}", indent, session.summary);
                        let truncated: String =
                            text.chars().take(max_width.saturating_sub(2)).collect();
                        lines.push(Line::from(Span::styled(truncated, row_style)));
                    }
                }

                // Row 3: left_off — last action or next step
                if lines.len() < list_height as usize {
                    let text = format!("  ↩ {}", session.left_off);
                    let truncated: String =
                        text.chars().take(max_width.saturating_sub(2)).collect();
                    lines.push(Line::from(Span::styled(truncated, dim_style)));
                }

                // Row 4: last prompt the user typed (omitted if not recorded)
                if lines.len() < list_height as usize {
                    if let Some(prompt) = &session.last_prompt {
                        let text = format!("  ❯ {}", prompt);
                        let truncated: String =
                            text.chars().take(max_width.saturating_sub(2)).collect();
                        lines.push(Line::from(Span::styled(
                            truncated,
                            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
                        )));
                    }
                }

                // Blank separator between sessions
                if lines.len() < list_height as usize {
                    lines.push(Line::default());
                }
            }
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_footer(&self, frame: &mut ratatui::Frame, area: Rect) {
        // Dark blue-gray bar — complements the cyan selection highlight
        let bg = Color::Rgb(40, 44, 52);
        let footer_base = Style::default().fg(Color::Gray).bg(bg);
        let key_style = Style::default().fg(Color::Cyan).bg(bg).add_modifier(Modifier::BOLD);
        let label_style = Style::default().fg(Color::Rgb(100, 110, 120)).bg(bg);

        if let Some(ref msg) = self.error_message {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("  {}", msg),
                    Style::default().fg(Color::Red).bg(bg),
                )))
                .style(Style::default().bg(bg)),
                area,
            );
            return;
        }

        if self.filter_mode {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("  / {}▌", self.filter),
                    footer_base,
                )))
                .style(footer_base),
                area,
            );
        } else if !self.filter.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("↑↓", key_style),
                    Span::styled(" navigate   ", label_style),
                    Span::styled("enter", key_style),
                    Span::styled(" resume   ", label_style),
                    Span::styled("/", key_style),
                    Span::styled(" edit filter   ", label_style),
                    Span::styled("esc", key_style),
                    Span::styled(" clear", label_style),
                ]))
                .style(footer_base),
                area,
            );
        } else {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("↑↓", key_style),
                    Span::styled(" navigate   ", label_style),
                    Span::styled("enter", key_style),
                    Span::styled(" resume   ", label_style),
                    Span::styled("o", key_style),
                    Span::styled(" open in tab   ", label_style),
                    Span::styled("/", key_style),
                    Span::styled(" filter   ", label_style),
                    Span::styled("x", key_style),
                    Span::styled(" mark done   ", label_style),
                    Span::styled("f", key_style),
                    Span::styled(" flag   ", label_style),
                    Span::styled("a", key_style),
                    Span::styled(" all   ", label_style),
                    Span::styled("F", key_style),
                    Span::styled(" flagged only   ", label_style),
                    Span::styled("→/←", key_style),
                    Span::styled(" preview   ", label_style),
                    Span::styled("q", key_style),
                    Span::styled(" quit", label_style),
                ]))
                .style(footer_base),
                area,
            );
        }
    }

    fn render_preview(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        // Left border acts as a visual divider between the two panes
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Extract the key before dropping the filtered borrow so we can
        // mutably access preview_cache on the next line
        let session_key = {
            let filtered = self.filtered();
            filtered.get(self.selected).map(|s| (s.session_id.clone(), s.path.clone()))
        };
        let Some((session_id, session_path)) = session_key else { return };

        // Populate cache on first view of this session
        let messages = self.preview_cache
            .entry(session_id)
            .or_insert_with(|| load_preview(&session_path));

        let width = inner.width.saturating_sub(1) as usize; // 1-char right margin
        let height = inner.height as usize;

        // Build rendered lines for all messages
        let mut all_lines: Vec<Line> = Vec::new();
        for (role, content) in messages.iter() {
            let is_user = role == "user";
            if is_user {
                // User turns: ❯ prefix on the first line, continuation lines indented to match
                for (i, wrapped) in wrap_text(content, width.saturating_sub(2)).iter().enumerate() {
                    let prefix = if i == 0 { "❯ " } else { "  " };
                    all_lines.push(Line::from(Span::styled(
                        format!("{}{}", prefix, wrapped),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    )));
                }
            } else {
                // Assistant turns: plain gray text, no label
                for wrapped in wrap_text(content, width.saturating_sub(1)) {
                    all_lines.push(Line::from(Span::styled(
                        format!(" {}", wrapped),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            all_lines.push(Line::default());
        }

        // Show only the tail so the most recent message is always visible
        let start = all_lines.len().saturating_sub(height);
        let visible: Vec<Line> = all_lines[start..].to_vec();

        frame.render_widget(Paragraph::new(visible), inner);
    }
}

pub fn run(sessions: Vec<SessionEntry>, initial_selected: usize, index_path: &std::path::Path, ui_state: UiState) -> Result<(TuiAction, UiState), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(sessions, index_path.to_path_buf(), initial_selected, ui_state);

    let action = loop {
        terminal.draw(|f| app.render(f))?;

        if let Event::Key(key) = event::read()? {
            app.error_message = None;
            if app.filter_mode {
                match key.code {
                    // Esc cancels the filter entirely and returns to normal mode
                    KeyCode::Esc => {
                        app.filter.clear();
                        app.filter_mode = false;
                        app.clamp_selected();
                    }
                    // Enter confirms the filter and resumes the selected session
                    KeyCode::Enter => {
                        app.filter_mode = false;
                        let filtered = app.filtered();
                        if !filtered.is_empty() {
                            let s = filtered[app.selected];
                            break TuiAction::Resume {
                                session_id: s.session_id.clone(),
                                cwd: s.cwd.clone(),
                            };
                        }
                    }
                    KeyCode::Backspace => {
                        app.filter.pop();
                        app.clamp_selected();
                    }
                    // Arrow keys navigate even while typing a filter
                    KeyCode::Up => app.move_up(),
                    KeyCode::Down => app.move_down(),
                    KeyCode::Char(c) => {
                        app.filter.push(c);
                        app.clamp_selected();
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                    KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                    KeyCode::Char('/') => app.filter_mode = true,
                    KeyCode::Esc => {
                        if !app.filter.is_empty() {
                            // First Esc clears the filter; second Esc (no filter) quits
                            app.filter.clear();
                            app.clamp_selected();
                        } else {
                            break TuiAction::Quit;
                        }
                    }
                    KeyCode::Enter => {
                        let filtered = app.filtered();
                        if !filtered.is_empty() {
                            let s = filtered[app.selected];
                            break TuiAction::Resume {
                                session_id: s.session_id.clone(),
                                cwd: s.cwd.clone(),
                            };
                        }
                    }
                    KeyCode::Char('o') => {
                        let filtered = app.filtered();
                        if !filtered.is_empty() {
                            let s = filtered[app.selected];
                            let mut cmd = std::process::Command::new("wezterm");
                            cmd.args(["cli", "spawn"]);
                            if let Some(ref dir) = s.cwd {
                                if !dir.is_empty() {
                                    cmd.args(["--cwd", dir]);
                                }
                            }
                            let config = Config::load().unwrap_or(Config { scan: Default::default(), resume_command: None });
                            cmd.arg("--").args(config.resume_argv(&s.session_id));
                            cmd.stdout(std::process::Stdio::null());
                            match cmd.spawn() {
                                Ok(_) => {}
                                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                    app.error_message = Some("wezterm not found — 'open in tab' requires WezTerm".to_string());
                                }
                                Err(e) => {
                                    app.error_message = Some(format!("open in tab failed: {}", e));
                                }
                            }
                        }
                    }
                    KeyCode::Right => app.show_preview = true,
                    KeyCode::Left => app.show_preview = false,
                    KeyCode::Char('a') => {
                        app.show_all = !app.show_all;
                        app.clamp_selected();
                    }
                    KeyCode::Char('F') => {
                        app.flagged_only = !app.flagged_only;
                        app.clamp_selected();
                    }
                    KeyCode::Char('q') => break TuiAction::Quit,
                    KeyCode::Char('x') => {
                        let id = app.filtered().get(app.selected).map(|s| s.session_id.clone());
                        if let Some(id) = id { app.handle_mark_done(&id); }
                    }
                    KeyCode::Char('f') => {
                        let id = app.filtered().get(app.selected).map(|s| s.session_id.clone());
                        if let Some(id) = id { app.handle_flag(&id); }
                    }
                    KeyCode::Char('*') => {
                        if let Some(session) = app.filtered().get(app.selected) {
                            record_false_positive(session);
                        }
                    }
                    KeyCode::Char('c')
                        if key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        break TuiAction::Quit;
                    }
                    _ => {}
                }
            }
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let ui_state = UiState {
        show_preview: app.show_preview,
        show_all: app.show_all,
        flagged_only: app.flagged_only,
    };
    Ok((action, ui_state))
}
