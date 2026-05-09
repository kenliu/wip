pub mod jsonl_parser;
pub mod lm_assessment;

pub async fn run(force: bool, provider_filter: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Implement scan mode
    // 1. Load config
    // 2. Load existing index
    // 3. Discover JSONL files matching provider patterns
    // 4. Filter files based on:
    //    - Force flag (reassess all)
    //    - Provider filter
    //    - Modification time (skip unchanged)
    //    - File age (skip < 5 min old)
    // 5. For each file to assess:
    //    - Parse JSONL and extract relevant fields (Rust)
    //    - Count estimated tokens
    //    - Call LLM for assessment
    //    - Update index entry
    // 6. Update aggregate token stats
    // 7. Save index
    // 8. Exit silently

    println!("Scan mode not yet implemented");
    Ok(())
}
