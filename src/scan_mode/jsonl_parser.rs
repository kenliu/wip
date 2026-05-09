use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ExtractedContext {
    pub first_message: String,
    pub recent_messages: Vec<(String, String)>, // (role, content)
    pub estimated_tokens: usize,
}

pub fn parse_and_extract(path: &Path) -> Result<ExtractedContext, Box<dyn std::error::Error>> {
    // TODO: Implement JSONL parsing
    // 1. Read file line by line
    // 2. Parse each line as JSON
    // 3. Extract: timestamp, role (user/assistant), content
    // 4. Keep first message (topic context)
    // 5. Keep last 5-10 user messages + 5 assistant responses
    // 6. Build minimal context block
    // 7. Estimate token count (~1 token per 4 chars)

    Err("Not implemented".into())
}

pub fn estimate_tokens(text: &str) -> usize {
    // Rough estimate: 1 token ≈ 4 chars
    (text.len() + 3) / 4
}
