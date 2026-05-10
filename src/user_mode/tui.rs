use crate::index::SessionEntry;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};
use std::io::{self, Write};
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

fn project_name(cwd: &str) -> String {
    std::path::Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

struct App<'a> {
    sessions: &'a [&'a SessionEntry],
    selected: usize,
    filter: String,
    // When true, keyboard input goes to the filter string instead of navigation
    filter_mode: bool,
}

impl<'a> App<'a> {
    fn new(sessions: &'a [&'a SessionEntry]) -> Self {
        Self {
            sessions,
            selected: 0,
            filter: String::new(),
            filter_mode: false,
        }
    }

    // Returns sessions matching the current filter (all sessions when filter is empty).
    // Matches against the project name (last path component of cwd).
    fn filtered(&self) -> Vec<&'a SessionEntry> {
        if self.filter.is_empty() {
            return self.sessions.iter().copied().collect();
        }
        let q = self.filter.to_lowercase();
        self.sessions
            .iter()
            .copied()
            .filter(|s| {
                let p = project_name(s.cwd.as_deref().unwrap_or(""));
                p.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn filtered_count(&self) -> usize {
        if self.filter.is_empty() {
            return self.sessions.len();
        }
        let q = self.filter.to_lowercase();
        self.sessions
            .iter()
            .filter(|s| {
                let p = project_name(s.cwd.as_deref().unwrap_or(""));
                p.to_lowercase().contains(&q)
            })
            .count()
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

    fn render(&self, frame: &mut ratatui::Frame) {
        let filtered = self.filtered();
        let area = frame.area();

        let [header_area, list_area, footer_area] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(area);

        // ── Header ──────────────────────────────────────────────────────────
        let header_text = if self.filter.is_empty() {
            "  IN-PROGRESS SESSIONS".to_string()
        } else {
            format!(
                "  IN-PROGRESS SESSIONS  ({} of {})",
                filtered.len(),
                self.sessions.len()
            )
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                header_text,
                Style::default().add_modifier(Modifier::BOLD),
            ))),
            header_area,
        );

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
                let project = project_name(session.cwd.as_deref().unwrap_or(""));
                let age = format_age(session.file_modified_at);

                // Row 1: cursor + project + provider + age
                lines.push(Line::from(vec![
                    Span::styled(format!("{} {:<22}", cursor, project), row_style),
                    Span::styled(format!("{:<14}", session.provider), dim_style),
                    Span::styled(format!("{:>8}", age), dim_style),
                ]));

                // Row 2: topic summary — what this session is about
                if lines.len() < list_height as usize {
                    let text = format!("  {}", session.summary);
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

        // ── Footer ───────────────────────────────────────────────────────────
        let bold = Style::default().add_modifier(Modifier::BOLD).fg(Color::DarkGray);
        let dim = Style::default().fg(Color::DarkGray);

        if self.filter_mode {
            // Show filter input with a block cursor indicator
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("  / {}▌", self.filter),
                    Style::default().fg(Color::Yellow),
                ))),
                footer_area,
            );
        } else if !self.filter.is_empty() {
            // Filter is active but not being edited — show edit/clear hints
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("↑↓", bold),
                    Span::styled(" navigate   ", dim),
                    Span::styled("enter", bold),
                    Span::styled(" resume   ", dim),
                    Span::styled("/", bold),
                    Span::styled(" edit filter   ", dim),
                    Span::styled("esc", bold),
                    Span::styled(" clear", dim),
                ])),
                footer_area,
            );
        } else {
            // Normal mode — no active filter
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("↑↓", bold),
                    Span::styled(" navigate   ", dim),
                    Span::styled("enter", bold),
                    Span::styled(" resume   ", dim),
                    Span::styled("/", bold),
                    Span::styled(" filter   ", dim),
                    Span::styled("q", bold),
                    Span::styled(" quit", dim),
                ])),
                footer_area,
            );
        }
    }
}

pub fn run(sessions: &[&SessionEntry]) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(sessions);

    let chosen = loop {
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
                            break Some(filtered[app.selected]);
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
                            break None;
                        }
                    }
                    KeyCode::Enter => {
                        let filtered = app.filtered();
                        if !filtered.is_empty() {
                            break Some(filtered[app.selected]);
                        }
                    }
                    KeyCode::Char('q') => break None,
                    KeyCode::Char('c')
                        if key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        break None
                    }
                    _ => {}
                }
            }
        }
    };

    // Restore terminal before exec'ing claude or returning
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Some(session) = chosen {
        print!("\x1B[2J\x1B[1;1H");
        io::stdout().flush()?;

        // exec() replaces this process entirely — no wip process remains in the process table
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new("claude");
        cmd.arg("--resume").arg(&session.session_id);
        if let Some(cwd) = &session.cwd {
            if !cwd.is_empty() {
                cmd.current_dir(cwd);
            }
        }
        return Err(format!("Failed to launch claude: {}", cmd.exec()).into());
    }

    Ok(())
}
