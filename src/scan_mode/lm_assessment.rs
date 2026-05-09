// Sends a pre-filtered session context to the Anthropic API and parses the
// structured response into status, summary, and left_off fields.
//
// We use a strict output format ("status: X\nsummary: Y\nleft_off: Z") rather
// than JSON to reduce output tokens. The parser searches for each prefix rather
// than requiring the lines to appear in order, since the LLM occasionally adds
// preamble text before the structured output.

use crate::scan_mode::jsonl_parser::{build_context, ExtractedContext};
use serde::{Deserialize, Serialize};

// input_tokens and output_tokens are tracked for cost reporting in scan.log.jsonl
#[allow(dead_code)]
pub struct AssessmentResponse {
    pub status: String,
    pub summary: String,
    pub left_off: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
}

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
- summary: 10-15 words. Commit-message style — describe the task or topic. Never start with 'User', 'Assistant', or 'Session'.
- left_off: 8-12 words. For in-progress: what is pending or blocking. For done: 'Complete.' or a brief completion note. Never write 'User exited' or 'Session ended'.";

pub async fn assess(
    context: &ExtractedContext,
    api_key: &str,
) -> Result<AssessmentResponse, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&ApiRequest {
            model: "claude-sonnet-4-6".to_string(),
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

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, text).into());
    }

    let api: ApiResponse = resp.json().await?;
    let text = api.content
        .iter()
        .find(|b| b.block_type == "text")
        .and_then(|b| b.text.as_deref())
        .ok_or("No text in API response")?;

    let (status, summary, left_off) = parse_response(text)?;
    Ok(AssessmentResponse {
        status,
        summary,
        left_off,
        input_tokens: api.usage.input_tokens,
        output_tokens: api.usage.output_tokens,
    })
}

fn parse_response(text: &str) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    let mut status = String::new();
    let mut summary = String::new();
    let mut left_off = String::new();

    for line in text.lines() {
        let line = line.trim();
        if status.is_empty() {
            if let Some(v) = line.strip_prefix("status:") {
                let v = v.trim();
                // Only accept the two valid values; rejects partial or malformed responses
                if v == "in-progress" || v == "done" {
                    status = v.to_string();
                }
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

    if status.is_empty() || summary.is_empty() || left_off.is_empty() {
        return Err(format!("Could not parse response: {}", &text[..text.len().min(200)]).into());
    }
    Ok((status, summary, left_off))
}
