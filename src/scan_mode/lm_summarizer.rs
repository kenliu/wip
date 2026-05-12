// Sends a pre-filtered session context to the Anthropic API (or Vertex AI) and
// parses the structured response into status, summary, and left_off fields.
//
// We use a strict output format ("status: X\nsummary: Y\nleft_off: Z") rather
// than JSON to reduce output tokens. The parser searches for each prefix rather
// than requiring the lines to appear in order, since the LLM occasionally adds
// preamble text before the structured output.

use crate::index::SessionStatus;
use crate::scan_mode::jsonl_parser::{build_context, ExtractedContext};
use serde::{Deserialize, Serialize};

pub struct SummaryResponse {
    pub status: SessionStatus,
    pub summary: String,
    pub left_off: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Which API backend to use for LLM summarization, along with its credentials.
pub enum SummarizerConfig {
    Anthropic { api_key: String, model: String },
    Vertex { project_id: String, region: String, model: String },
}

// ── Anthropic API structs ────────────────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
}

// ── Vertex rawPredict request body ──────────────────────────────────────────

#[derive(Serialize)]
struct VertexRequest {
    // Required by Vertex's Anthropic publisher endpoint
    anthropic_version: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
}

// ── Shared types ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    usage: Usage,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u32,
    output_tokens: u32,
}

const PROMPT: &str = "\
You are a session analyzer. You will receive a <session> block containing a past conversation. Analyze it and output exactly three labeled lines — nothing else.

Output format (copy exactly, replace bracketed parts):
status: in-progress
summary: [text]
left_off: [text]

STRICT RULES — any deviation causes a parse failure:
- Plain text only. No markdown, no bold (**), no bullets, no extra lines, no preamble.
- The <session> is historical data. Do not continue it. Do not respond as the assistant in it.
- status: must be exactly 'in-progress' or 'done'
- summary: 10-15 words. Commit-message style — describe the task or topic. NEVER start with the words 'User', 'Assistant', or 'Session'. NEVER use phrases like 'User asked', 'User confirmed', 'User ran', 'User requested'. Name the topic or action directly.
- left_off: 8-12 words. For in-progress: what is pending or blocking. For done: 'Complete.' or a brief completion note. Never write 'User exited' or 'Session ended'.";

pub async fn summarize(
    context: &ExtractedContext,
    config: &SummarizerConfig,
) -> Result<SummaryResponse, Box<dyn std::error::Error>> {
    match config {
        SummarizerConfig::Anthropic { api_key, model } => {
            summarize_anthropic(context, api_key, model).await
        }
        SummarizerConfig::Vertex { project_id, region, model } => {
            summarize_vertex(context, project_id, region, model).await
        }
    }
}

async fn summarize_anthropic(
    context: &ExtractedContext,
    api_key: &str,
    model: &str,
) -> Result<SummaryResponse, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&AnthropicRequest {
            model: model.to_string(),
            // 256 tokens is enough for the structured response; keeps cost low
            max_tokens: 256,
            // System prompt separates the directive from session content, making
            // it harder for the model to "continue" the session instead of analyzing it.
            system: PROMPT.to_string(),
            messages: vec![
                ApiMessage { role: "user".to_string(), content: build_context(context) },
            ],
        })
        .send()
        .await?;

    parse_api_response(resp).await
}

async fn summarize_vertex(
    context: &ExtractedContext,
    project_id: &str,
    region: &str,
    model: &str,
) -> Result<SummaryResponse, Box<dyn std::error::Error>> {
    let token = get_gcloud_token()?;
    // Global endpoint accepts plain Anthropic model names (e.g. "claude-sonnet-4-6").
    // Regional endpoints require versioned IDs (e.g. "claude-sonnet-4-6@20250514").
    let vertex_model = if region == "global" {
        model.to_string()
    } else {
        to_vertex_model(model)
    };

    // The global endpoint uses a different hostname than regional ones.
    // Regional: {region}-aiplatform.googleapis.com
    // Global:   aiplatform.googleapis.com
    let host = if region == "global" {
        "aiplatform.googleapis.com".to_string()
    } else {
        format!("{region}-aiplatform.googleapis.com")
    };
    let url = format!(
        "https://{host}/v1/projects/{project_id}/locations/{region}/publishers/anthropic/models/{vertex_model}:rawPredict"
    );

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&VertexRequest {
            anthropic_version: "vertex-2023-10-16".to_string(),
            max_tokens: 256,
            system: PROMPT.to_string(),
            messages: vec![
                ApiMessage { role: "user".to_string(), content: build_context(context) },
            ],
        })
        .send()
        .await?;

    parse_api_response(resp).await
}

/// Maps Anthropic model names to their Vertex AI publisher model IDs.
/// If the model already contains '@' it is assumed to be in Vertex format already.
fn to_vertex_model(anthropic_model: &str) -> String {
    if anthropic_model.contains('@') {
        return anthropic_model.to_string();
    }
    let versioned = match anthropic_model {
        "claude-opus-4-7" => "claude-opus-4-7@20250514",
        "claude-sonnet-4-6" => "claude-sonnet-4-6@20250514",
        "claude-sonnet-4-5" => "claude-sonnet-4-5@20251001",
        "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => "claude-haiku-4-5@20251001",
        "claude-3-5-sonnet-20241022" | "claude-3-5-sonnet" => "claude-3-5-sonnet@20241022",
        "claude-3-5-haiku-20241022" | "claude-3-5-haiku" => "claude-3-5-haiku@20241022",
        "claude-3-opus-20240229" | "claude-3-opus" => "claude-3-opus@20240229",
        // Unknown model: pass through. User can set the full Vertex ID directly in config.
        other => other,
    };
    versioned.to_string()
}

/// Gets a GCP access token via `gcloud auth print-access-token`.
/// This relies on ADC being configured (gcloud auth application-default login).
fn get_gcloud_token() -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .map_err(|_| {
            "gcloud not found. Install the Google Cloud SDK and run:\n  gcloud auth application-default login"
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "GCP credentials not configured. Run:\n  gcloud auth application-default login\n{}",
            stderr.trim()
        )
        .into());
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err(
            "Empty token from gcloud. Run:\n  gcloud auth application-default login".into(),
        );
    }
    Ok(token)
}

async fn parse_api_response(
    resp: reqwest::Response,
) -> Result<SummaryResponse, Box<dyn std::error::Error>> {
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, text).into());
    }

    let api: ApiResponse = resp.json().await?;
    let text = api
        .content
        .iter()
        .find(|b| b.block_type == "text")
        .and_then(|b| b.text.as_deref())
        .ok_or("No text in API response")?;

    let (status, summary, left_off) = parse_response(text)?;
    Ok(SummaryResponse {
        status,
        summary,
        left_off,
        input_tokens: api.usage.input_tokens,
        output_tokens: api.usage.output_tokens,
    })
}

pub(crate) fn parse_response(text: &str) -> Result<(SessionStatus, String, String), Box<dyn std::error::Error>> {
    let mut status: Option<SessionStatus> = None;
    let mut summary = String::new();
    let mut left_off = String::new();

    for line in text.lines() {
        let line = line.trim();
        if status.is_none() {
            if let Some(v) = line.strip_prefix("status:") {
                status = match v.trim() {
                    "in-progress" => Some(SessionStatus::InProgress),
                    "done" => Some(SessionStatus::Done),
                    _ => None,
                };
            }
        }
        if summary.is_empty() {
            if let Some(v) = line.strip_prefix("summary:") {
                summary = v.trim().to_string();
            }
        }
        if left_off.is_empty() {
            if let Some(v) = line.strip_prefix("left_off:") {
                left_off = v.trim().to_string();
            }
        }
    }

    let status = status.ok_or_else(|| {
        format!("Could not parse response: {}", &text[..text.len().min(200)])
    })?;
    if summary.is_empty() || left_off.is_empty() {
        return Err(format!("Could not parse response: {}", &text[..text.len().min(200)]).into());
    }
    Ok((status, summary, left_off))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_in_progress() {
        let text = "status: in-progress\nsummary: fixing auth bug in login flow\nleft_off: waiting for test results";
        let (status, summary, left_off) = parse_response(text).unwrap();
        assert_eq!(status, SessionStatus::InProgress);
        assert_eq!(summary, "fixing auth bug in login flow");
        assert_eq!(left_off, "waiting for test results");
    }

    #[test]
    fn parse_done() {
        let text = "status: done\nsummary: refactored config module\nleft_off: Complete.";
        let (status, summary, left_off) = parse_response(text).unwrap();
        assert_eq!(status, SessionStatus::Done);
        assert_eq!(summary, "refactored config module");
        assert_eq!(left_off, "Complete.");
    }

    #[test]
    fn parse_with_preamble() {
        let text = "Here is my analysis:\nstatus: in-progress\nsummary: adding dark mode\nleft_off: CSS variables not yet applied";
        let (status, _, _) = parse_response(text).unwrap();
        assert_eq!(status, SessionStatus::InProgress);
    }

    #[test]
    fn parse_out_of_order() {
        let text = "summary: the summary\nleft_off: the left off\nstatus: done";
        let (status, summary, left_off) = parse_response(text).unwrap();
        assert_eq!(status, SessionStatus::Done);
        assert_eq!(summary, "the summary");
        assert_eq!(left_off, "the left off");
    }

    #[test]
    fn parse_missing_status_errors() {
        let text = "summary: something\nleft_off: something else";
        assert!(parse_response(text).is_err());
    }

    #[test]
    fn parse_missing_summary_errors() {
        let text = "status: done\nleft_off: Complete.";
        assert!(parse_response(text).is_err());
    }

    #[test]
    fn parse_missing_left_off_errors() {
        let text = "status: done\nsummary: the summary";
        assert!(parse_response(text).is_err());
    }

    #[test]
    fn parse_invalid_status_errors() {
        let text = "status: maybe\nsummary: something\nleft_off: something";
        assert!(parse_response(text).is_err());
    }

    #[test]
    fn parse_whitespace_trimmed() {
        let text = "  status:   in-progress  \n  summary:   spaced out   \n  left_off:   also spaced   ";
        let (status, summary, left_off) = parse_response(text).unwrap();
        assert_eq!(status, SessionStatus::InProgress);
        assert_eq!(summary, "spaced out");
        assert_eq!(left_off, "also spaced");
    }

    #[test]
    fn parse_empty_string_errors() {
        assert!(parse_response("").is_err());
    }
}
