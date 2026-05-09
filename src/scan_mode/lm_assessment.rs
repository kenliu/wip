use crate::scan_mode::jsonl_parser::{build_context, ExtractedContext};
use serde::{Deserialize, Serialize};

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
Analyze this LLM session and provide:
1. status: 'in-progress' or 'done'
2. summary: 1-2 sentences (20-30 words) about the topic/goal
3. left_off: 1 sentence (10-15 words) about the last action or next step

Reply exactly in this format:
status: in-progress
summary: [text]
left_off: [text]";

pub async fn assess(
    context: &ExtractedContext,
    api_key: &str,
) -> Result<AssessmentResponse, Box<dyn std::error::Error>> {
    let body = format!("{}\n\nSession:\n{}", PROMPT, build_context(context));

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&ApiRequest {
            model: "claude-haiku-4-5-20251001".to_string(),
            max_tokens: 256,
            messages: vec![ApiMessage { role: "user".to_string(), content: body }],
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
