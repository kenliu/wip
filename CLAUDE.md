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
cargo run -- fast           # fzf-powered session picker
cargo run -- install        # Install launchd agent (macOS)
cargo run -- uninstall      # Uninstall launchd agent
cargo run -- index clear    # Delete the index file
cargo run -- stats          # Show token usage stats
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
cargo run -- --background-scan     # TUI with background scan running silently
./target/release/wip               # After release build
```

## Key Architectural Decisions

### Three UI Modes

- **User Mode** (`main.rs` → user_mode module): Interactive ratatui TUI showing all sessions with filtering. Sorted by recency; supports flagging, a toggleable right-pane preview, and show-all. Accepts `--background-scan` to kick off a silent scan in the background while browsing.
- **Fast Mode** (`fast_mode.rs`): fzf-powered minimal picker for keyboard-optimized selection. Requires fzf installed (`brew install fzf`).
- **Scan Mode** (`scan_mode/mod.rs`): Unattended filesystem scanning, LLM summarization, index updates. Cron-friendly; also used by the launchd agent.

### Token Efficiency (Critical)

The scanner is designed to minimize API token consumption:

1. **Rust Pre-filtering** (`scan_mode/jsonl_parser.rs`): Parse JSONL files and extract only relevant fields (first message + last 5-10 messages). Discard verbose output.
2. **Timestamp-Based Caching** (`index.rs`): Track file modification times. Skip unchanged files entirely (0 tokens).
3. **Skip Logic** (inlined in `scan_mode/mod.rs`): Skip files < 30 seconds old (still being written) and > 30 days old.
4. **Single Assessment Model**: All sessions assessed with same Claude model, reducing setup overhead.

Expected result: ~200-400 tokens per new/modified session, ~0 for cached ones.

### Configuration & Setup

- Config file at `~/.wip/config.json` defines scan backend, model, and optional Vertex settings.
- **Interactive setup wizard** runs on first `wip scan` if config doesn't exist and stdin is a terminal. Prompts for backend choice (Anthropic or Vertex) and writes the config.
- Non-interactive (cron/pipe): falls back to `ANTHROPIC_API_KEY` env var, or prints a setup guide and exits.
- Two backends: `anthropic` (reads `ANTHROPIC_API_KEY` env var) and `vertex` (credentials from GCP ADC).
- `keychain.rs` provides a system keychain wrapper for future API key storage; not yet wired into the main flow.

### UI State Persistence

Toggle state is persisted across sessions in `~/.wip/ui_state.json`:
- `show_preview` — right-pane chat preview (toggled with `→`/`←`)
- `show_all` — include done sessions in the list (toggled with `a`)
- `flagged_only` — show only flagged sessions (toggled with `F`)

State is saved when the TUI exits (any action: resume, open-in-tab, or quit).

### Index Storage

Single JSON file at `~/.wip/index.json`:
- Session metadata (path, provider, status, summaries, cwd)
- LLM-generated fields: `summary`, `left_off`
- Rich stats: `file_size_bytes`, `turn_count`, `message_count`, `duration_secs`
- `custom_title` from `/rename` command (`custom-title` JSONL record)
- User state: `flagged`, `manually_done`, `continuation`
- Protected by advisory lock (`fd_lock` crate, `~/.wip/index.lock`)
- Atomic writes: written to a `.tmp` file, then renamed to avoid corruption on crash

### Scan Log

Each scan appends one JSON entry to `~/.wip/scan.log.jsonl`: timestamp, session counts (in-progress/done/pruned), and token usage. `wip stats` reads this file for the usage summary.

### launchd Agent (macOS)

`wip install` generates and loads a launchd plist at `~/Library/LaunchAgents/com.kenliu.wip.plist` that runs `wip scan` every 10 minutes. `wip uninstall` unloads and removes it. Agent logs go to `~/.wip/launchd.log`. The `ANTHROPIC_API_KEY` is embedded in the plist (launchd doesn't inherit shell env); keychain integration is planned for a future release.

## Module Structure

- `main.rs`: CLI argument parsing (clap), mode routing
- `user_mode/`: Interactive ratatui TUI
  - `mod.rs`: Index loading, UI state load/save, background scan spawn, exec() handoff
  - `tui.rs`: All rendering, key bindings, and interactive logic
- `fast_mode.rs`: fzf-based minimal session picker (requires fzf)
- `scan_mode/`: Unattended scanning and LLM summarization
  - `mod.rs`: Main scan loop, skip logic, setup wizard, scan log writing
  - `jsonl_parser.rs`: JSONL parsing, field extraction, token counting
  - `lm_summarizer.rs`: LLM prompt construction, response parsing; supports Anthropic and Vertex backends
- `config.rs`: Load/parse `~/.wip/config.json`; defines `SummaryBackend`, `ScanConfig`, `Config`
- `index.rs`: Load/save index, advisory locking, mtime tracking, `in_progress_sessions()` dedup logic
- `install_mode.rs`: launchd plist generation, install/uninstall commands
- `stats_mode.rs`: Parses `~/.wip/scan.log.jsonl`, displays token usage and scan history
- `keychain.rs`: System keychain wrapper (not yet wired into main flow; planned for issue #5)

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

## Claude Code JSONL Session File Format

Claude Code writes one JSON object per line. Known record types:

- `permission-mode` — first record in a normal user-initiated session
- `file-history-snapshot` — file state snapshot, written by Claude Code
- `user` / `assistant` — conversation messages (the records we care about)
- `system` — system-level messages, not part of the user conversation
- `attachment` — file attachments added to the conversation
- `progress` — sub-agent progress events
- `last-prompt` — records the last prompt the user typed; written at session end
- `custom-title` — written by the `/rename` command; contains `customTitle` (string). Multiple records may appear; the last one wins. `wip` uses this as the display name in the TUI, overriding the project directory name.
- `pr-link` — written when a PR is created during a session; contains `prNumber`, `prUrl`, `prRepository`
- `agent-setting` — first record in sessions spawned as named agents (filenames also start with `agent-`)
- `queue-operation` — first record in sessions spawned automatically by Claude Code for background tasks (e.g. generating thread titles, injecting prior conversation context). These are **not user-initiated**.

### `isSidechain` and Sub-agent Sessions

Records with `"isSidechain": true` appear only inside `agent-*` files, never in main session files. They reference their parent session via the `sessionId` field. Since `wip` already skips `agent-*` files by filename, sidechain records never reach the parser — no special handling needed.

### `queue-operation` Sessions

When the first record has `"type": "queue-operation"`, the session was spawned automatically, not by the user directly. The `content` field may embed prior conversation history as `<conversation_history>` text, but there is **no parent session ID field** — the link is implicit text only. Each queue-operation session has its own new UUID with no explicit reference back to the session it continues from.

`wip` sets `continuation: true` on these sessions in the index and deduplicates them in `in_progress_sessions()`: only the most recent continuation session per cwd is shown, and non-continuation sessions are always shown regardless.

## Testing Approach

- Unit tests for JSONL parsing and token counting (most critical)
- Integration tests with mock LLM responses
- Manual testing with actual session files before release
