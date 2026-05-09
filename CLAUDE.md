# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**wip** is a Rust CLI tool for discovering and resuming active LLM sessions across multiple providers (Claude Code, OpenCode, etc.). It scans the filesystem for JSONL session files, uses an LLM to assess their state (done/in-progress) and summarize context, then provides an interactive TUI to quickly resume sessions.

See `WIP_SPEC.md` for the complete specification.

## Build & Run

```bash
cargo build                 # Debug build
cargo build --release       # Optimized binary for production
cargo check                 # Quick syntax/type check (no binary)
cargo run                   # Run user mode (default)
cargo run -- scan           # Run scan mode
cargo run -- stats          # Show token usage stats
cargo run -- config         # Show configuration
cargo test                  # Run tests
cargo clippy                # Linting
cargo fmt                   # Format code
```

**Binary locations:**
- Debug: `target/debug/wip`
- Release: `target/release/wip`

**Run examples:**
```bash
cargo run -- scan --force
cargo run -- scan --provider claude-code
./target/release/wip                        # After release build
```

## Key Architectural Decisions

### Two Separate Modes

- **User Mode** (`main.rs` → user_mode module): Interactive TUI showing in-progress sessions with summaries. Sorted by recency, filters out done sessions.
- **Scan Mode** (`main.rs` → scan_mode module): Unattended filesystem scanning, LLM assessment, index updates. Cron-friendly.

### Token Efficiency (Critical)

The scanner is designed to minimize API token consumption:

1. **Rust Pre-filtering** (`scan_mode/jsonl_parser.rs`): Parse JSONL files and extract only relevant fields (first message + last 5-10 messages). Discard verbose output.
2. **Timestamp-Based Caching** (`index.rs`): Track file modification times. Skip unchanged files entirely (0 tokens).
3. **Skip Logic** (`scan_mode/skip_conditions.rs`): Don't assess files < 5 min old (still being written).
4. **Single Assessment Model**: All sessions assessed with same Claude model, reducing setup overhead.

Expected result: ~200-400 tokens per new/modified session, ~0 for cached ones.

### Configuration & Keychain

- Config file at `~/.wip/config.json` defines providers, CLI launchers, assessment model
- API keys stored in system keychain (via `keyring` crate), never in config
- Provider CLIs (claude, opencode) are just command templates—no API keys needed for them

### Index Storage

Single JSON file at `~/.wip/index.json`:
- Session metadata (path, provider, status, summaries)
- Per-session token usage (input/output/total)
- Aggregate stats (total tokens, assessments run vs. skipped, estimated cost)

## Module Structure (MVP Phase)

- `main.rs`: CLI argument parsing (clap), mode routing
- `user_mode/`: Interactive TUI, session filtering, session resumption
  - `tui.rs`: Terminal UI and selection logic
  - `session_list.rs`: Render in-progress sessions with summaries
- `scan_mode/`: Unattended scanning and assessment
  - `scanner.rs`: Main scan loop, orchestration
  - `jsonl_parser.rs`: JSONL parsing, field extraction, token counting
  - `lm_assessment.rs`: LLM prompt construction, response parsing
- `config.rs`: Load/parse `~/.wip/config.json`
- `index.rs`: Load/save session index, mtime tracking
- `keychain.rs`: Retrieve API keys from system keychain

## Code Comments

Add comments for readers who may be new to Rust. Focus on the *why*, not the *what*:
- Explain non-obvious design choices (e.g. why we use `exec()` instead of spawning a child process)
- Note constraints or gotchas (e.g. byte vs. char boundaries in string slicing)
- Clarify intent where the code alone is ambiguous

Keep comments concise — one line is usually enough. Don't restate what the code obviously does.

## Development Workflow

1. Keep token efficiency front-of-mind—profile token usage in tests
2. Use `WIP_SPEC.md` as source of truth for behavior and data formats
3. Test with real JSONL files from Claude Code and OpenCode
4. Config/index should be easily inspectable (human-readable JSON, pretty-printed)

## Testing Approach

- Unit tests for JSONL parsing and token counting (most critical)
- Integration tests with mock LLM responses
- Manual testing with actual session files before release
