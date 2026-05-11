// Parses Claude Code JSONL session files and extracts a minimal context block
// for LLM summarization. The goal is to send as few tokens as possible while
// giving the summarizer enough signal to determine status and summarize the work.
//
// Claude Code JSONL format: one JSON object per line. Each record has a "type"
// field. We care about "user" and "assistant" records, which carry a "message"
// object with "role" and "content". Content can be a plain string (user
// messages) or an array of typed blocks (assistant messages, which mix text,
// thinking, tool_use, etc). All other record types are ignored.

use serde_json::Value;
use std::io::BufRead;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ExtractedContext {
    pub first_message: String,
    pub recent_messages: Vec<(String, String)>, // (role, content)
    // Working directory from the session file, used to resume in the right directory
    pub cwd: Option<String>,
    // The last thing the user typed, from the `last-prompt` record written at session end
    pub last_prompt: Option<String>,
    // User-assigned name from the `/rename` command, stored in `custom-title` records
    pub custom_title: Option<String>,
    // Number of user turns in the session
    pub turn_count: u32,
    // Total user + assistant messages with extractable text
    pub message_count: u32,
    // Duration from first to last timestamped record, in seconds
    pub duration_secs: Option<i64>,
}

/// Returns true if this session was started via a queue-operation (an automated
/// continuation). These sessions embed prior conversation history and are part of
/// a chain — only the latest in the chain is relevant to the user.
pub fn is_continuation_session(path: &Path) -> bool {
    let Ok(f) = std::fs::File::open(path) else { return false };
    for line in std::io::BufReader::new(f).lines().take(5) {
        let Ok(line) = line else { continue };
        let line = line.trim();
        if line.is_empty() { continue }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        return v.get("type").and_then(|t| t.as_str()) == Some("queue-operation");
    }
    false
}

pub fn parse_and_extract(path: &Path) -> Result<ExtractedContext, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let mut messages: Vec<(String, String)> = Vec::new();
    let mut cwd: Option<String> = None;
    let mut last_prompt: Option<String> = None;
    let mut custom_title: Option<String> = None;
    let mut first_ts: Option<i64> = None;
    let mut last_ts: Option<i64> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        // cwd appears on user/assistant records; capture it from the first one found
        if cwd.is_none() {
            if let Some(s) = v.get("cwd").and_then(|s| s.as_str()) {
                cwd = Some(s.to_string());
            }
        }

        // Track session time span using the timestamp field present on most record types
        if let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()).and_then(parse_iso8601_secs) {
            if first_ts.is_none() {
                first_ts = Some(ts);
            }
            last_ts = Some(ts);
        }

        match v.get("type").and_then(|t| t.as_str()) {
            Some("user") => {
                if let Some(text) = extract_user_content(&v) {
                    messages.push(("user".to_string(), text));
                }
            }
            Some("assistant") => {
                if let Some(text) = extract_assistant_content(&v) {
                    messages.push(("assistant".to_string(), text));
                }
            }
            // Multiple last-prompt records can appear; keep the final one
            Some("last-prompt") => {
                if let Some(s) = v.get("lastPrompt").and_then(|s| s.as_str()) {
                    last_prompt = Some(s.to_string());
                }
            }
            // Written by /rename; the last record wins if there are multiple
            Some("custom-title") => {
                if let Some(s) = v.get("customTitle").and_then(|s| s.as_str()) {
                    custom_title = Some(s.to_string());
                }
            }
            _ => {}
        }
    }

    if messages.is_empty() {
        return Err("No messages found".into());
    }

    let first_message = messages.first()
        .map(|(_, c)| c.clone())
        .unwrap_or_default();

    let turn_count = messages.iter().filter(|(role, _)| role == "user").count() as u32;

    // Keep only the tail of the conversation — the recent context is what matters
    // for determining status and what was left off
    let recent_start = messages.len().saturating_sub(15);
    let recent_messages = messages[recent_start..].to_vec();

    let message_count = messages.len() as u32;
    let duration_secs = match (first_ts, last_ts) {
        (Some(first), Some(last)) if last > first => Some(last - first),
        _ => None,
    };

    Ok(ExtractedContext { first_message, recent_messages, cwd, last_prompt, custom_title, turn_count, message_count, duration_secs })
}

// Parses "YYYY-MM-DDTHH:MM:SS[.mmm]Z" into a Unix timestamp (seconds).
// Avoids a chrono dependency for this one narrow use case.
fn parse_iso8601_secs(s: &str) -> Option<i64> {
    let s = s.trim_end_matches('Z');
    let (date_part, time_part) = s.split_once('T')?;

    let mut d = date_part.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;

    let time_no_frac = time_part.split('.').next()?;
    let mut t = time_no_frac.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next()?.parse().ok()?;

    let is_leap = |y: i64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days_in_month = |y: i64, m: i64| match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31i64,
        4 | 6 | 9 | 11 => 30,
        2 => if is_leap(y) { 29 } else { 28 },
        _ => 0,
    };

    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    for m in 1..month {
        days += days_in_month(year, m);
    }
    days += day - 1;

    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

fn extract_user_content(v: &Value) -> Option<String> {
    let content = v.get("message")?.get("content")?;
    match content {
        // Simple user messages are plain strings
        Value::String(s) => Some(s.to_string()),
        // Tool result messages use the content block array format
        Value::Array(arr) => {
            let text: String = arr.iter()
                .filter_map(|item| {
                    if item.get("type")?.as_str()? == "text" {
                        item.get("text")?.as_str().map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            if text.is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}

fn extract_assistant_content(v: &Value) -> Option<String> {
    // Assistant content is always an array of typed blocks (text, thinking, tool_use, etc).
    // We only extract "text" blocks; thinking blocks are opaque and tool_use is too verbose.
    let arr = v.get("message")?.get("content")?.as_array()?;
    let text: String = arr.iter()
        .filter_map(|item| {
            if item.get("type")?.as_str()? == "text" {
                item.get("text")?.as_str().map(str::to_string)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() { None } else { Some(text) }
}

// Builds the context string sent to the summarizer LLM. Includes the first
// message for topic context and the recent tail for status/left-off signal.
pub fn build_context(ctx: &ExtractedContext) -> String {
    let mut parts = vec![format!("First message: {}", ctx.first_message)];
    parts.push("\nRecent messages:".to_string());
    for (role, content) in &ctx.recent_messages {
        parts.push(format!("{}: {}", role, content));
    }
    // XML tags make it unambiguous to the model that this is data to analyze,
    // not a live conversation to continue (sessions often end with an assistant turn).
    format!("<session>\n{}\n</session>", parts.join("\n"))
}

#[allow(dead_code)]
pub fn estimate_tokens(text: &str) -> usize {
    // Rough heuristic: 1 token ≈ 4 characters
    (text.len() + 3) / 4
}

