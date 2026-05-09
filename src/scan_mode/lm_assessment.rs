use crate::scan_mode::jsonl_parser::ExtractedContext;

#[derive(Debug, Clone)]
pub struct AssessmentResponse {
    pub status: String, // "in-progress" or "done"
    pub summary: String,
    pub left_off: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub async fn assess_session(
    context: &ExtractedContext,
    model: &str,
    api_key: &str,
    prompt_template: &str,
) -> Result<AssessmentResponse, Box<dyn std::error::Error>> {
    // TODO: Implement LLM assessment
    // 1. Build full prompt with context
    // 2. Call Claude API with prompt
    // 3. Parse response for status, summary, left_off
    // 4. Extract token usage from response
    // 5. Return AssessmentResponse

    Err("Not implemented".into())
}

fn build_assessment_prompt(context: &ExtractedContext, template: &str) -> String {
    // TODO: Build prompt with context injected
    format!("{}\n\nContext:\n{}", template, context.first_message)
}

fn parse_assessment_response(response: &str) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    // TODO: Parse response format:
    // status: in-progress
    // summary: ...
    // left_off: ...

    Err("Not implemented".into())
}
