# WIP: LLM Session Manager - Specification

## 1. Overview

**wip** is a CLI tool for tracking and resuming active LLM sessions. It solves the "too many terminal tabs" problem: once you have several chat sessions going simultaneously, it becomes hard to keep track of them and costly to close any — because finding your way back requires effort. **wip** makes it safe to close sessions by ensuring you can always find them again quickly.

The tool runs passively in the background, scanning for session files written by LLM CLIs (Claude Code, OpenCode, etc.), summarizing their content with an LLM, and maintaining a fast local index. When you want to return to a session, `wip` shows you a list of your in-progress work with enough context to immediately recognize and resume it.

## 2. Core Purpose

- **Close with confidence** — sessions are automatically tracked; closing a terminal tab doesn't mean losing context
- **Return quickly** — `wip` opens instantly showing pre-computed summaries; no waiting
- **Passive by default** — runs in the background via cron; no user action required to "save" a session
- **Quick resumption** — resume a session with a single keystroke
- **Never block** — the UI is always immediate; scanning never blocks the user
- **Multi-provider support** — start with Claude Code and OpenCode, expand to others

## 3. Design Goals

### User Performance First
wip is built in Rust for fast startup and low latency. Every user-facing action should feel immediate:
- `wip` shows the session list instantly from the cached index
- Scanning happens in the background and never blocks the UI
- Index reads are the hot path; writes happen asynchronously

**Performance target**: `wip` (no flags) should display the session list in < 100ms.

### Passive by Default
The tool requires zero ongoing user effort. Sessions are discovered and summarized automatically by a background cron job. The user's only job is to occasionally run `wip` to browse and resume. No "saving" or "checking in" required.

### Minimal UI, Maximum Signal
The session list shows only what matters: in-progress sessions, sorted by most recent. Done sessions are not shown — they are noise. Each session shows enough context to immediately recognize it without opening it.

## 4. Key Concepts

### Session File
JSONL files created by LLM CLI tools (e.g., `~/.claude/sessions/session-id.jsonl`). Each line is a JSON object representing a message or interaction in the conversation.

### Session Index
wip maintains a local index at `~/.wip/index.json` with:
- File path and provider
- Session status (in-progress / done)
- LLM-generated summary and "left off" description
- Last file modification and scan timestamps
- Token usage per assessment

### Provider
An LLM CLI tool (Claude Code, OpenCode, etc.). Configuration includes glob patterns for finding session files, a command template for resuming sessions, and JSONL format details.

## 5. Modes & Commands

wip has three modes: **TUI mode** (default), **fast mode**, and **scan mode**.

### TUI Mode (Default)
Information-rich interactive interface for session browsing and resumption.

```
wip                   # TUI mode (instant, from cached index)
wip --scan            # TUI mode + kick off background scan (non-blocking)
```

**Behavior:**
1. Load index from disk instantly
2. Display in-progress sessions sorted by most recent modification, with arrow-key navigation
3. If `--scan` flag is set, start a background scan concurrently — does not block or delay display
4. User navigates with ↑/↓ and presses Enter to select; cursor starts on most recent session
5. Screen clears, configured CLI launches in the current terminal

The UI is never blocked. The cached index is always shown immediately. If a background scan completes while the UI is open and finds new results, the display updates.

**Keybindings:**
- `↑` / `↓` — navigate sessions
- `Enter` — resume selected session (screen clears, CLI takes over)
- `q` — quit

**Display:**
```
  IN-PROGRESS SESSIONS

▶ Project X Analysis       claude-code    47m ago
  Left off: waiting on feedback about rate-limiting design.

  Data Processing          claude-code    2h ago
  Left off: debugging records with missing zip codes.

  Bug Investigation        opencode       5h ago
  Left off: need to review logs from staging environment.

  ↑↓ navigate   enter resume   q quit
```

**After selection:**
```
[screen clears]
[configured CLI launches and takes over the terminal]
```

### Fast Mode
Keyboard-optimized mode for maximum speed. Uses fzf for fuzzy filtering and selection.

```
wip fast              # Fast mode: fzf-powered session picker
wip fast --scan       # Fast mode + background scan (non-blocking)
```

**Behavior:**
1. Load index from disk instantly
2. Pipe session list into fzf; user types to filter, arrows to navigate, Enter to select
3. Screen clears, configured CLI launches in the current terminal

fzf is a required dependency for fast mode. If fzf is not installed, wip exits with a clear error message and installation instructions.

**fzf display:**
```
> Project X Analysis    claude-code    47m    waiting on rate-limiting feedback
  Data Processing       claude-code    2h     debugging missing zip codes
  Bug Investigation     opencode       5h     need to review staging logs
  3/3 ────────────────────────────────────────────────
> _
```

The user types to fuzzy-filter, arrows to navigate, Enter to launch. Same clean handoff as TUI mode.

### Scan Mode
Unattended mode for periodic background analysis. Cron-friendly, exits silently.

```
wip scan                     # Scan for new/modified sessions
wip scan --force             # Force re-assessment of all sessions
wip scan --provider claude   # Scan only one provider
```

**Behavior:**
1. Find all JSONL files matching configured patterns
2. Skip files unchanged since last scan (0 tokens consumed)
3. Skip files modified < 5 minutes ago (may still be actively written)
4. Assess new/modified sessions with an LLM
5. Update index atomically
6. Exit silently

**Recommended cron schedule**: every 5-10 minutes.

**Optional hook facility**: Users can configure a shell hook to trigger `wip scan` when closing a terminal. This is not required and not the default — the cron job is the primary mechanism. Session closure must never block waiting for a scan.

### Configuration & Utilities
```
wip config          # Show current configuration
wip config edit     # Open config file in $EDITOR
wip index show      # Show raw session index
wip index clear     # Clear index (forces full rescan on next scan run)
wip stats           # Show token usage and scan statistics
```

**Stats Output Example:**
```
$ wip stats
Token Usage Summary:
  Total tokens: 2,847
  Input: 1,634 | Output: 1,213
  Assessments run: 12 | Skipped (cached): 84
  Last scan: 2 hours ago

Estimated cost: $0.014 (based on claude-haiku-4-5 pricing)

Per-provider breakdown:
  claude-code: 2,847 tokens (12 assessments)
  opencode: 0 tokens (0 assessments)
```

## 6. Data Model & Storage

### Directory Structure
```
~/.wip/
├── config.json     # Provider patterns, CLI templates, scan settings
└── index.json      # Session index with pre-computed summaries
```

### Index Format
```json
{
  "sessions": [
    {
      "path": "/Users/ken/.claude/sessions/abc123.jsonl",
      "provider": "claude-code",
      "displayName": "Project X Analysis",
      "status": "in-progress",
      "fileModifiedAt": 1715335200,
      "lastScannedAt": 1715338800,
      "summary": "Designing REST API endpoints for user management. Discussed authentication strategy, debated pagination approach.",
      "leftOff": "waiting on feedback about rate-limiting design",
      "cliLauncher": "claude-code",
      "assessment": {
        "tokensUsed": 287,
        "inputTokens": 156,
        "outputTokens": 131
      }
    },
    {
      "path": "/Users/ken/.opencode/sessions/def456.jsonl",
      "provider": "opencode",
      "displayName": "Data Analysis",
      "status": "done",
      "fileModifiedAt": 1715248800,
      "lastScannedAt": 1715338800,
      "summary": "Analyzed Q1 sales metrics. Generated charts and identified top-performing regions.",
      "leftOff": "task completed, all findings documented",
      "cliLauncher": "opencode",
      "assessment": {
        "tokensUsed": 312,
        "inputTokens": 178,
        "outputTokens": 134
      }
    }
  ],
  "lastFullScan": "2026-05-09T15:00:00Z",
  "tokenUsageStats": {
    "totalTokensUsed": 599,
    "totalInputTokens": 334,
    "totalOutputTokens": 265,
    "assessmentsRun": 2,
    "assessmentsSkipped": 18,
    "estimatedCost": 0.003
  }
}
```

## 7. Workflows

### Typical Workflow
```
$ wip

  IN-PROGRESS SESSIONS

▶ Project X Analysis       claude-code    47m ago
  Left off: waiting on feedback about rate-limiting design.

  Data Processing          claude-code    2h ago
  Left off: debugging records with missing zip codes.

  Bug Investigation        opencode       5h ago
  Left off: need to review logs from staging environment.

  ↑↓ navigate   enter resume   q quit

[user presses Enter]
[screen clears]
[claude resumes session abc123.jsonl]
```

The list appears instantly from the cached index. Pressing Enter immediately resumes the most recent session — the common case requires zero navigation.

### Background Scan (Cron)
```cron
# Scan every 5 minutes
*/5 * * * * /usr/local/bin/wip scan
```

**Scan Process:**
1. Find all JSONL files in configured locations
2. Skip files unchanged since last scan (0 tokens)
3. Skip files < 5 min old (still being written)
4. For modified sessions: extract context, call LLM, store result
5. Update index atomically
6. Exit silently

### Scan on Startup (Optional)
```
$ wip --scan
IN-PROGRESS SESSIONS:        [scanning in background...]
  1. Project X Analysis      [claude-code]    Modified 47 min ago
     ...
```

The list appears immediately from the cache. The background scan runs concurrently and updates the index. If it finds new sessions, the display refreshes.

### Search/Filter
```
$ wip project
IN-PROGRESS SESSIONS (matching "project"):
  1. Project X Analysis      [claude-code]    Modified 47 min ago
     ...
```

## 8. Implementation Scope

### Phase 1 (MVP)
- **User Mode**: Instant TUI from cached index; single-keystroke session resumption
- **Scan Mode**: Token-efficient background scanning
  - Glob-based file discovery
  - Timestamp-based caching (skip unchanged files)
  - JSONL parsing and field extraction
  - LLM assessment for modified files only
- **Non-blocking `--scan` flag**: Background scan concurrent with UI display
- Claude Code provider support
- OpenCode provider support
- Atomic JSON index writes (write to temp, rename)
- Keychain integration for API keys
- Token usage tracking and cost estimation

### Phase 2
- TUI refresh when background scan finds new results
- Parallel scanning across multiple providers
- Performance optimizations (batch LLM requests)
- In-UI search/filter

### Phase 3
- Additional providers (Gemini, Codex CLI, others)
- Advanced filtering and sorting
- Detailed session view
- Optional terminal close hook integration

## Future Vision

These are longer-term product directions that should inform architectural decisions without driving MVP scope.

### Terminal Tab Integration
Resume sessions in new terminal tabs rather than the current terminal. Support for:
- **tmux**: `tmux new-window` (terminal-agnostic, most powerful)
- **iTerm2**: AppleScript API for new tabs
- **WezTerm**: `wezterm cli spawn`

### Workspace Restore
`wip restore` — open all in-progress sessions at once, each in its own tab. The equivalent of restoring a browser session after a crash. Useful at the start of a work session or after a terminal crash.

### Workspace Arrangement
`wip arrange` — programmatically lay out sessions across panes and windows (by project, recency, provider, etc.). tmux is the natural foundation for this given its scripting capabilities.

## 9. Technical Requirements

### Language & Build
- **Language**: Rust
- **Distribution**: Single binary, no runtime dependencies
- **Minimum Rust**: 1.70+
- **Performance target**: `wip` (no flags) must display the session list in < 100ms

### Key Dependencies
- **CLI/TUI**: `clap`, `ratatui`, `crossterm`
- **JSON**: `serde`, `serde_json`
- **Networking**: `reqwest`
- **Glob patterns**: `glob`
- **Keychain**: `keyring`
- **Time**: `chrono`

### External Dependencies
- **fzf**: Required for fast mode. wip shells out to fzf as a subprocess. If not installed, `wip fast` exits with an error and installation instructions.

### File Operations
- Glob-based JSONL file discovery
- Line-by-line JSONL parsing
- File modification time tracking
- **Atomic index writes** (write to temp file, rename into place): required to prevent corruption on crash or concurrent access

## 10. Configuration

### Config File Location
`~/.wip/config.json`

### Example Config
```json
{
  "providers": {
    "claude-code": {
      "sessionPatterns": [
        "~/.claude/sessions/*.jsonl",
        "~/.local/share/claude/sessions/*.jsonl"
      ],
      "cliLauncher": {
        "name": "claude-code",
        "command": "claude",
        "args": ["{sessionPath}"]
      }
    },
    "opencode": {
      "sessionPatterns": [
        "~/.opencode/sessions/*.jsonl",
        "~/.local/share/opencode/sessions/*.jsonl"
      ],
      "cliLauncher": {
        "name": "opencode",
        "command": "opencode",
        "args": ["{sessionPath}"]
      }
    }
  },
  "scan": {
    "assessmentModel": "claude-haiku-4-5-20251001",
    "assessmentApiKey": {
      "keychainKey": "wip-claude-api-key"
    },
    "assessmentPrompt": "Analyze the conversation and provide:\n1. status: 'in-progress' or 'done'\n2. summary: 1-2 sentences (20-30 words) about the topic/goal\n3. left_off: 1 sentence (10-15 words) about the last action or next step\n\nReply exactly as:\nstatus: X\nsummary: Y\nleft_off: Z",
    "pricing": {
      "inputTokensPerMillion": 0.80,
      "outputTokensPerMillion": 4.00
    }
  },
  "storageDir": "~/.wip",
  "indexRefreshThreshold": 3600
}
```

### Config Notes
- **sessionPatterns**: Glob patterns for finding session files. Multiple patterns per provider.
- **cliLauncher**: Command template to resume a session. `{sessionPath}` is replaced with the actual file path.
- **assessmentModel**: LLM used for all session assessments. Haiku is recommended for cost efficiency.
- **assessmentApiKey.keychainKey**: Keychain entry name storing the API key. Never stored in config directly.
- **assessmentPrompt**: LLM prompt for assessing session status. Customizable.
- **pricing**: Optional. Per-million token costs (in dollars) for cost estimation in `wip stats`.
- **indexRefreshThreshold**: Seconds before index is considered stale (informational; does not trigger automatic scan).

## 11. Assessment Logic

### Two-Phase Strategy

**Philosophy**: Use Rust to filter and extract; use LLM only for synthesis. Minimize token consumption.

#### Phase 1: Rust Pre-filtering
For each session file:
1. Check mtime — if unchanged since last scan, skip entirely (0 tokens)
2. Skip if file was modified < 5 minutes ago (may still be actively written)
3. Parse JSONL line by line; extract only: `role`, `content`, `timestamp`
4. Discard metadata, tool output, formatting, and all other fields
5. Build context block: first user message + last 5-10 user messages + last 5 assistant responses
6. Estimate token count (1 token ≈ 4 chars); truncate aggressively if over threshold (~500 tokens)

#### Phase 2: LLM Assessment
Send pre-filtered context to the configured assessment model:

```
Analyze this session and provide:
1. status: 'in-progress' or 'done'
2. summary: 1-2 sentences (20-30 words) about the topic/goal
3. left_off: 1 sentence (10-15 words) about the last action or next step

Session context:
[pre-filtered content from Phase 1]

Reply exactly as:
status: in-progress
summary: [text]
left_off: [text]
```

**Token tracking**: Capture `usage` from LLM response. Store per-session (`assessment.inputTokens`, `assessment.outputTokens`) and aggregate in `tokenUsageStats`.

### Skip Conditions
- File mtime ≤ last scanned mtime → skip (cached result is current)
- File age < 5 minutes → skip (still being written)
- `--force` flag bypasses all skip conditions

### Expected Token Usage
- New or modified session: ~200-400 tokens
- Cached/unchanged session: 0 tokens
- Typical cron run: majority of sessions cached → very low marginal cost

### Edge Cases
- Empty JSONL → skip
- Malformed JSONL → log error, retain previous assessment if one exists
- LLM API failure → retain previous assessment, retry on next scan
- File deleted since last scan → remove from index
