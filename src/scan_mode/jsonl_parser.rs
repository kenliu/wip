// Parses Claude Code JSONL session files and extracts a minimal context block
// for LLM assessment. The goal is to send as few tokens as possible while
// giving the assessor enough signal to determine status and summarize the work.
//
// Claude Code JSONL format: one JSON object per line. Each record has a "type"
// field. We care about "user" and "assistant" records, which carry a "message"
// object with "role" and "content". Content can be a plain string (user
// messages) or an array of typed blocks (assistant messages, which mix text,
// thinking, tool_use, etc). All other record types are ignored.

use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ExtractedContext {
    pub first_message: String,
    pub recent_messages: Vec<(String, String)>, // (role, content)
    // Working directory from the session file, used to resume in the right directory
    pub cwd: Option<String>,
}

pub fn parse_and_extract(path: &Path) -> Result<ExtractedContext, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let mut messages: Vec<(String, String)> = Vec::new();
    let mut cwd: Option<String> = None;

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
            _ => {}
        }
    }

    if messages.is_empty() {
        return Err("No messages found".into());
    }

    let first_message = messages.first()
        .map(|(_, c)| truncate(c, 500))
        .unwrap_or_default();

    // Keep only the tail of the conversation — the recent context is what matters
    // for determining status and what was left off
    let recent_start = messages.len().saturating_sub(15);
    let recent_messages = messages[recent_start..].to_vec();

    Ok(ExtractedContext { first_message, recent_messages, cwd })
}

fn extract_user_content(v: &Value) -> Option<String> {
    let content = v.get("message")?.get("content")?;
    match content {
        // Simple user messages are plain strings
        Value::String(s) => Some(truncate(s, 1000)),
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
            if text.is_empty() { None } else { Some(truncate(&text, 1000)) }
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
    if text.is_empty() { None } else { Some(truncate(&text, 1000)) }
}

// Builds the context string sent to the assessment LLM. Includes the first
// message for topic context and the recent tail for status/left-off signal.
pub fn build_context(ctx: &ExtractedContext) -> String {
    let mut parts = vec![format!("First message: {}", ctx.first_message)];
    parts.push("\nRecent messages:".to_string());
    for (role, content) in &ctx.recent_messages {
        parts.push(format!("{}: {}", role, truncate(content, 300)));
    }
    parts.join("\n")
}

#[allow(dead_code)]
pub fn estimate_tokens(text: &str) -> usize {
    // Rough heuristic: 1 token ≈ 4 characters
    (text.len() + 3) / 4
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        // Count by chars not bytes to avoid splitting multi-byte UTF-8 characters
        s.chars().take(max.saturating_sub(3)).collect::<String>() + "..."
    }
}
