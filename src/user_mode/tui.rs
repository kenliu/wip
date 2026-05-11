use crate::index::SessionEntry;
use std::collections::HashMap;

pub enum TuiAction {
    Resume { session_id: String, cwd: Option<String> },
    MarkDone { session_id: String, cursor: usize },
    Flag { session_id: String, cursor: usize },
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

fn format_age(ts: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let secs = (now - ts).max(0);
    if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
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

fn project_name(cwd: &str) -> String {
    std::path::Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// Returns the user-assigned title if set, otherwise the project directory name.
fn session_display_name(session: &SessionEntry) -> String {
    session.custom_title.clone()
        .unwrap_or_else(|| project_name(session.cwd.as_deref().unwrap_or("")))
}

// ── Chat preview helpers ──────────────────────────────────────────────────────

fn load_preview(path: &str) -> Vec<(String, String)> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };
    let mut messages = Vec::new();
    for line in content.lines() {
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

struct App<'a> {
    sessions: &'a [&'a SessionEntry],
    selected: usize,
    filter: String,
    // When true, keyboard input goes to the filter string instead of navigation
    filter_mode: bool,
    // When true, only flagged sessions are shown
    flagged_only: bool,
    // Keyed by session_id; populated lazily on first render of each session
    preview_cache: HashMap<String, Vec<(String, String)>>,
}

impl<'a> App<'a> {
    fn new(sessions: &'a [&'a SessionEntry], initial_selected: usize) -> Self {
        let selected = initial_selected.min(sessions.len().saturating_sub(1));
        Self {
            sessions,
            selected,
            filter: String::new(),
            filter_mode: false,
            flagged_only: false,
            preview_cache: HashMap::new(),
        }
    }

    // Returns sessions matching the current filter and flagged_only mode.
    fn filtered(&self) -> Vec<&'a SessionEntry> {
        let q = self.filter.to_lowercase();
        self.sessions
            .iter()
            .copied()
            .filter(|s| {
                if self.flagged_only && !s.flagged { return false; }
                if !q.is_empty() && !session_display_name(s).to_lowercase().contains(&q) { return false; }
                true
            })
            .collect()
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

        if content_area.width >= 120 {
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
        let list_area = area;

        // ── Session list ─────────────────────────────────────────────────────
        let list_height = list_area.height;
        let offset = self.scroll_offset(list_height);
        let max_width = list_area.width as usize;

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
                let (row_style, dim_style) = if is_selected {
                    (
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                        Style::default().fg(Color::Cyan),
                    )
                } else {
                    (Style::default(), Style::default().fg(Color::DarkGray))
                };

                let cursor = if is_selected { "▶" } else { " " };
                let display_name = session_display_name(session);
                let age = format_age(session.file_modified_at);
                let size = format_size(session.file_size_bytes);

                // Row 1: cursor + display name + age + file size + turn count
                lines.push(Line::from(vec![
                    Span::styled(format!("{} {:<21}", cursor, display_name), row_style),
                    Span::styled(format!("{:<10}", age), dim_style),
                    Span::styled(format!("{:<7}", size), dim_style),
                    Span::styled(format!("{}t", session.turn_count), dim_style),
                ]));

                // Row 2: flag (only if set) + topic summary
                if lines.len() < list_height as usize {
                    let flag_prefix = if session.flagged { "🚩 " } else { "" };
                    let text = format!("  {}{}", flag_prefix, session.summary);
                    let truncated: String =
                        text.chars().take(max_width.saturating_sub(2)).collect();
                    lines.push(Line::from(Span::styled(truncated, row_style)));
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

        frame.render_widget(Paragraph::new(lines), list_area);
    }

    fn render_footer(&self, frame: &mut ratatui::Frame, area: Rect) {
        // Dark blue-gray bar — complements the cyan selection highlight
        let bg = Color::Rgb(40, 44, 52);
        let footer_base = Style::default().fg(Color::Gray).bg(bg);
        let key_style = Style::default().fg(Color::Cyan).bg(bg).add_modifier(Modifier::BOLD);
        let label_style = Style::default().fg(Color::Rgb(100, 110, 120)).bg(bg);

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
                    Span::styled("/", key_style),
                    Span::styled(" filter   ", label_style),
                    Span::styled("x", key_style),
                    Span::styled(" mark done   ", label_style),
                    Span::styled("f", key_style),
                    Span::styled(" flag   ", label_style),
                    Span::styled("F", key_style),
                    Span::styled(" flagged only   ", label_style),
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

        let filtered = self.filtered();
        let Some(session) = filtered.get(self.selected).copied() else { return };

        // Populate cache on first view of this session
        let messages = self.preview_cache
            .entry(session.session_id.clone())
            .or_insert_with(|| load_preview(&session.path));

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

pub fn run(sessions: &[&SessionEntry], initial_selected: usize) -> Result<TuiAction, Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(sessions, initial_selected);

    let action = loop {
        terminal.draw(|f| app.render(f))?;

        if let Event::Key(key) = event::read()? {
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
                    KeyCode::Char('F') => {
                        app.flagged_only = !app.flagged_only;
                        app.clamp_selected();
                    }
                    KeyCode::Char('q') => break TuiAction::Quit,
                    KeyCode::Char('x') => {
                        let filtered = app.filtered();
                        if !filtered.is_empty() {
                            let session_id = filtered[app.selected].session_id.clone();
                            break TuiAction::MarkDone { session_id, cursor: app.selected };
                        }
                    }
                    KeyCode::Char('f') => {
                        let filtered = app.filtered();
                        if !filtered.is_empty() {
                            let session_id = filtered[app.selected].session_id.clone();
                            break TuiAction::Flag { session_id, cursor: app.selected };
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

    Ok(action)
}
