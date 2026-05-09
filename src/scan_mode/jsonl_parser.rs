use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ExtractedContext {
    pub first_message: String,
    pub recent_messages: Vec<(String, String)>, // (role, content)
    pub slug: Option<String>,
    pub cwd: Option<String>,
}

pub fn parse_and_extract(path: &Path) -> Result<ExtractedContext, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let mut messages: Vec<(String, String)> = Vec::new();
    let mut slug: Option<String> = None;
    let mut cwd: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if slug.is_none() {
            if let Some(s) = v.get("slug").and_then(|s| s.as_str()) {
                slug = Some(s.to_string());
            }
        }

        // cwd from the first record that has it
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

    let recent_start = messages.len().saturating_sub(15);
    let recent_messages = messages[recent_start..].to_vec();

    Ok(ExtractedContext { first_message, recent_messages, slug, cwd })
}

fn extract_user_content(v: &Value) -> Option<String> {
    let content = v.get("message")?.get("content")?;
    match content {
        Value::String(s) => Some(truncate(s, 1000)),
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
    (text.len() + 3) / 4
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max.saturating_sub(3)).collect::<String>() + "..."
    }
}
