use crate::config::Config;
use crate::index::{index_path, Index};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

fn scan_log_path() -> std::path::PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".wip")
        .join("scan.log.jsonl")
}

#[derive(Deserialize)]
struct ScanLogEntry {
    timestamp: String,
    #[serde(default)]
    unix_ts: i64,
    summaries_run: u32,
    #[serde(default)]
    tokens: TokenCounts,
}

#[derive(Deserialize, Default)]
struct TokenCounts {
    input: u64,
    output: u64,
}

fn format_age(unix_ts: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let secs = (now - unix_ts).max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86400 {
        format!("{} hours ago", secs / 3600)
    } else {
        format!("{} days ago", secs / 86400)
    }
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut total_input: u64 = 0;
    let mut total_output: u64 = 0;
    let mut total_summaries: u32 = 0;
    let mut last_unix_ts: i64 = 0;
    let mut last_timestamp_str: Option<String> = None;
    let mut scan_count: u32 = 0;

    let log_path = scan_log_path();
    if log_path.exists() {
        let content = std::fs::read_to_string(&log_path)?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(entry) = serde_json::from_str::<ScanLogEntry>(line) else {
                continue;
            };
            total_input += entry.tokens.input;
            total_output += entry.tokens.output;
            total_summaries += entry.summaries_run;
            if entry.unix_ts > last_unix_ts {
                last_unix_ts = entry.unix_ts;
            }
            last_timestamp_str = Some(entry.timestamp);
            scan_count += 1;
        }
    }

    let total_tokens = total_input + total_output;
    let config = Config::load().ok();
    let pricing = config.as_ref().and_then(|c| c.scan.pricing.as_ref());
    let model = config
        .as_ref()
        .map(|c| c.scan.summary_model.as_str())
        .unwrap_or("unknown");

    println!("Token Usage");
    println!("  Total:         {}", format_number(total_tokens));
    println!(
        "  Input: {}  Output: {}",
        format_number(total_input),
        format_number(total_output)
    );
    println!("  Summaries run: {}", total_summaries);
    println!("  Scans logged:  {}", scan_count);

    match (last_unix_ts > 0, last_timestamp_str.as_deref()) {
        (true, _) => println!("  Last scan:     {}", format_age(last_unix_ts)),
        (false, Some(ts)) => println!("  Last scan:     {}", ts),
        (false, None) => println!("  Last scan:     never"),
    }

    if let Some(p) = pricing {
        let cost = (total_input as f64 / 1_000_000.0) * p.input_tokens_per_million
            + (total_output as f64 / 1_000_000.0) * p.output_tokens_per_million;
        println!();
        println!("Estimated cost: ${:.4}  (based on {} pricing)", cost, model);
    }

    let index = Index::load(&index_path())?;
    let in_progress = index
        .sessions
        .iter()
        .filter(|s| s.status == "in-progress")
        .count();
    let done = index.sessions.iter().filter(|s| s.status == "done").count();

    println!();
    println!("Session Index");
    println!("  In progress: {}", in_progress);
    println!("  Done:        {}", done);

    // Per-provider session counts
    let mut by_provider: std::collections::BTreeMap<&str, (usize, usize)> =
        std::collections::BTreeMap::new();
    for s in &index.sessions {
        let entry = by_provider.entry(s.provider.as_str()).or_default();
        if s.status == "in-progress" {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    if !by_provider.is_empty() {
        println!();
        println!("By provider");
        for (provider, (ip, done)) in &by_provider {
            println!("  {}:  {} in-progress  {} done", provider, ip, done);
        }
    }

    Ok(())
}
