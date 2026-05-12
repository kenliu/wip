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
pub(crate) fn parse_iso8601_secs(s: &str) -> Option<i64> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn write_temp_jsonl(lines: &[Value]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        f
    }

    // ── extract_user_content ────────────────────────────────────────────

    #[test]
    fn user_content_string() {
        let v = json!({"message": {"content": "hello world"}});
        assert_eq!(extract_user_content(&v).unwrap(), "hello world");
    }

    #[test]
    fn user_content_array_text_blocks() {
        let v = json!({"message": {"content": [
            {"type": "text", "text": "first"},
            {"type": "text", "text": "second"}
        ]}});
        assert_eq!(extract_user_content(&v).unwrap(), "first second");
    }

    #[test]
    fn user_content_array_skips_non_text() {
        let v = json!({"message": {"content": [
            {"type": "tool_result", "content": "stuff"},
            {"type": "text", "text": "actual message"}
        ]}});
        assert_eq!(extract_user_content(&v).unwrap(), "actual message");
    }

    #[test]
    fn user_content_empty_string_returns_none() {
        let v = json!({"message": {"content": ""}});
        // String variant returns Some("") which is non-empty check is in the caller
        // The function itself returns Some for empty strings
        let result = extract_user_content(&v);
        assert_eq!(result, Some("".to_string()));
    }

    #[test]
    fn user_content_empty_array_returns_none() {
        let v = json!({"message": {"content": []}});
        assert!(extract_user_content(&v).is_none());
    }

    #[test]
    fn user_content_missing_message_returns_none() {
        let v = json!({"type": "user"});
        assert!(extract_user_content(&v).is_none());
    }

    // ── extract_assistant_content ────────────────────────────────────────

    #[test]
    fn assistant_content_text_blocks() {
        let v = json!({"message": {"content": [
            {"type": "text", "text": "response here"}
        ]}});
        assert_eq!(extract_assistant_content(&v).unwrap(), "response here");
    }

    #[test]
    fn assistant_content_skips_thinking_and_tool_use() {
        let v = json!({"message": {"content": [
            {"type": "thinking", "thinking": "hmm..."},
            {"type": "tool_use", "name": "bash", "input": {}},
            {"type": "text", "text": "the answer"}
        ]}});
        assert_eq!(extract_assistant_content(&v).unwrap(), "the answer");
    }

    #[test]
    fn assistant_content_all_non_text_returns_none() {
        let v = json!({"message": {"content": [
            {"type": "thinking", "thinking": "hmm"}
        ]}});
        assert!(extract_assistant_content(&v).is_none());
    }

    #[test]
    fn assistant_content_not_array_returns_none() {
        let v = json!({"message": {"content": "plain string"}});
        assert!(extract_assistant_content(&v).is_none());
    }

    // ── parse_iso8601_secs ──────────────────────────────────────────────

    #[test]
    fn iso8601_basic() {
        // 2024-01-01T00:00:00Z = known epoch value
        let ts = parse_iso8601_secs("2024-01-01T00:00:00Z").unwrap();
        // 2024-01-01 is 54 years after epoch, with leap years accounted for
        assert_eq!(ts, 1704067200);
    }

    #[test]
    fn iso8601_with_millis() {
        let ts = parse_iso8601_secs("2024-01-01T12:30:45.123Z").unwrap();
        let base = parse_iso8601_secs("2024-01-01T12:30:45Z").unwrap();
        assert_eq!(ts, base); // millis are ignored
    }

    #[test]
    fn iso8601_invalid_returns_none() {
        assert!(parse_iso8601_secs("not a date").is_none());
        assert!(parse_iso8601_secs("").is_none());
        assert!(parse_iso8601_secs("2024-01-01").is_none()); // missing time part
    }

    #[test]
    fn iso8601_leap_year() {
        // 2024-03-01 should account for Feb having 29 days in 2024
        let mar1 = parse_iso8601_secs("2024-03-01T00:00:00Z").unwrap();
        let feb28 = parse_iso8601_secs("2024-02-28T00:00:00Z").unwrap();
        assert_eq!(mar1 - feb28, 2 * 86400); // Feb 29 + Mar 1
    }

    #[test]
    fn iso8601_non_leap_year() {
        // 2023-03-01 — Feb has 28 days
        let mar1 = parse_iso8601_secs("2023-03-01T00:00:00Z").unwrap();
        let feb28 = parse_iso8601_secs("2023-02-28T00:00:00Z").unwrap();
        assert_eq!(mar1 - feb28, 86400); // exactly 1 day
    }

    // ── is_continuation_session ─────────────────────────────────────────

    #[test]
    fn continuation_session_detected() {
        let f = write_temp_jsonl(&[
            json!({"type": "queue-operation", "content": "..."}),
            json!({"type": "user", "message": {"content": "hello"}}),
        ]);
        assert!(is_continuation_session(f.path()));
    }

    #[test]
    fn normal_session_not_continuation() {
        let f = write_temp_jsonl(&[
            json!({"type": "permission-mode", "mode": "default"}),
            json!({"type": "user", "message": {"content": "hello"}}),
        ]);
        assert!(!is_continuation_session(f.path()));
    }

    #[test]
    fn empty_file_not_continuation() {
        let f = tempfile::NamedTempFile::new().unwrap();
        assert!(!is_continuation_session(f.path()));
    }

    // ── parse_and_extract ───────────────────────────────────────────────

    #[test]
    fn parse_basic_session() {
        let f = write_temp_jsonl(&[
            json!({"type": "user", "cwd": "/home/user/project", "message": {"content": "fix the bug"}, "timestamp": "2024-06-01T10:00:00Z"}),
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "I'll look at it"}]}, "timestamp": "2024-06-01T10:01:00Z"}),
            json!({"type": "user", "message": {"content": "thanks"}, "timestamp": "2024-06-01T10:02:00Z"}),
        ]);
        let ctx = parse_and_extract(f.path()).unwrap();
        assert_eq!(ctx.first_message, "fix the bug");
        assert_eq!(ctx.cwd.as_deref(), Some("/home/user/project"));
        assert_eq!(ctx.turn_count, 2);
        assert_eq!(ctx.message_count, 3);
        assert_eq!(ctx.duration_secs, Some(120));
        assert!(ctx.last_prompt.is_none());
        assert!(ctx.custom_title.is_none());
    }

    #[test]
    fn parse_captures_last_prompt() {
        let f = write_temp_jsonl(&[
            json!({"type": "user", "message": {"content": "hello"}}),
            json!({"type": "last-prompt", "lastPrompt": "deploy to staging"}),
        ]);
        let ctx = parse_and_extract(f.path()).unwrap();
        assert_eq!(ctx.last_prompt.as_deref(), Some("deploy to staging"));
    }

    #[test]
    fn parse_captures_custom_title() {
        let f = write_temp_jsonl(&[
            json!({"type": "user", "message": {"content": "hello"}}),
            json!({"type": "custom-title", "customTitle": "auth refactor"}),
        ]);
        let ctx = parse_and_extract(f.path()).unwrap();
        assert_eq!(ctx.custom_title.as_deref(), Some("auth refactor"));
    }

    #[test]
    fn parse_last_custom_title_wins() {
        let f = write_temp_jsonl(&[
            json!({"type": "user", "message": {"content": "hello"}}),
            json!({"type": "custom-title", "customTitle": "first name"}),
            json!({"type": "custom-title", "customTitle": "final name"}),
        ]);
        let ctx = parse_and_extract(f.path()).unwrap();
        assert_eq!(ctx.custom_title.as_deref(), Some("final name"));
    }

    #[test]
    fn parse_empty_file_errors() {
        let f = tempfile::NamedTempFile::new().unwrap();
        assert!(parse_and_extract(f.path()).is_err());
    }

    #[test]
    fn parse_no_messages_errors() {
        let f = write_temp_jsonl(&[
            json!({"type": "permission-mode", "mode": "default"}),
        ]);
        let err = parse_and_extract(f.path()).unwrap_err();
        assert_eq!(err.to_string(), "No messages found");
    }

    #[test]
    fn parse_keeps_recent_tail() {
        let mut lines: Vec<Value> = Vec::new();
        for i in 0..30 {
            lines.push(json!({"type": "user", "message": {"content": format!("msg {}", i)}}));
            lines.push(json!({"type": "assistant", "message": {"content": [{"type": "text", "text": format!("reply {}", i)}]}}));
        }
        let f = write_temp_jsonl(&lines);
        let ctx = parse_and_extract(f.path()).unwrap();
        assert_eq!(ctx.recent_messages.len(), 15);
        assert_eq!(ctx.message_count, 60);
        assert_eq!(ctx.turn_count, 30);
    }

    // ── build_context ───────────────────────────────────────────────────

    #[test]
    fn build_context_format() {
        let ctx = ExtractedContext {
            first_message: "fix the bug".to_string(),
            recent_messages: vec![
                ("user".to_string(), "fix the bug".to_string()),
                ("assistant".to_string(), "done".to_string()),
            ],
            cwd: None,
            last_prompt: None,
            custom_title: None,
            turn_count: 1,
            message_count: 2,
            duration_secs: None,
        };
        let output = build_context(&ctx);
        assert!(output.starts_with("<session>"));
        assert!(output.ends_with("</session>"));
        assert!(output.contains("First message: fix the bug"));
        assert!(output.contains("user: fix the bug"));
        assert!(output.contains("assistant: done"));
    }

    // ── estimate_tokens ─────────────────────────────────────────────────

    #[test]
    fn estimate_tokens_basic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }
}

