# WIP: LLM Session Manager - Specification

## 1. Overview

**wip** is a CLI tool for discovering, analyzing, and resuming active LLM sessions across multiple providers (Claude, OpenAI, Google Gemini, etc.). It scans the filesystem for JSONL session files created by various LLM CLIs, analyzes their state, and provides a unified interface to quickly resume work-in-progress conversations.

## 2. Core Purpose

- **Auto-discover sessions** - scan filesystem for JSONL session files
- **Assess session state** - use an LLM to summarize and determine if sessions are done or in-progress
- **Filter WIP sessions** - show only active/incomplete sessions
- **Quick resumption** - resume sessions with a single keystroke
- **Unattended analysis** - scan mode for periodic analysis via cron
- **Multi-provider support** - start with Claude Code and OpenCode, expand to others

## 3. Key Concepts

### Session File
JSONL files created by LLM CLI tools (e.g., `~/.local/share/claude/sessions/session-id.jsonl`). Each line is a JSON object representing a message/interaction in the conversation.

### Session Index
wip maintains a local index of discovered sessions with:
- File path
- Provider (detected from content or config)
- Session status (done/in-progress)
- Last assessment timestamp
- Last file modification timestamp
- LLM summarization result
- Display name / description

### Provider
An LLM CLI tool (Claude, OpenAI, Google Gemini). Configuration includes:
- Session file glob pattern(s)
- Command template to launch (with parameter placeholders)
- Default model/parameters
- How to parse JSONL files

## 4. Modes & Commands

### User Mode (Default)
Interactive TUI for session browsing and resumption.

```
wip                         # Launch user mode (interactive list)
wip [search-term]           # Filter sessions by search term
```

**User Mode Behavior:**
1. Scans for sessions (if index is stale)
2. Displays list of in-progress sessions, sorted by most recent
3. Shows: session name/path, provider, CLI, last modified time
4. User presses Enter to resume selected session
5. Launches configured CLI command for that session in current terminal

### Scan Mode
Unattended mode for periodic analysis (cron-friendly).

```
wip scan                    # Scan filesystem for new/updated sessions
wip scan --force            # Force re-assessment of all sessions
wip scan --provider claude  # Scan only Claude sessions
```

**Scan Mode Behavior:**
1. Searches filesystem for JSONL files matching configured patterns
2. Filters for new/modified sessions since last scan
3. Analyzes each session with an LLM prompt: "Is this session done or in progress?"
4. Stores assessment in index with timestamps
5. Exits silently (cron-friendly)

### Configuration & Utilities
```
wip config                  # Show current configuration
wip config edit             # Edit config file
wip index show              # Show current session index
wip index clear             # Clear index (forces rescan on next use)
wip stats                   # Show token usage and scan statistics
```

**Stats Output Example**:
```
$ wip stats
Token Usage Summary:
  Total tokens: 2,847
  Input: 1,634 | Output: 1,213
  Assessments run: 12 | Skipped (cached): 84
  Last scan: 2 hours ago

Estimated cost: $0.14 (based on claude-3-opus pricing)

Per-provider breakdown:
  claude-code: 2,847 tokens (12 assessments)
  opencode: 0 tokens (0 assessments)
```

## 5. Data Model & Storage

### Directory Structure
```
~/.wip/
├── config.json            # Configuration, provider patterns, CLI templates
└── index.json             # Session index with assessment results
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
      "summary": "Designing REST API endpoints for user management. Discussed authentication strategy, debated pagination approach. Left off: waiting on feedback about rate-limiting design.",
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
      "summary": "Analyzed Q1 sales metrics. Generated charts and identified top-performing regions. Task completed, all findings documented.",
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
    "estimatedCost": 0.03
  }
}
```

## 6. Workflows

### Typical User Workflow
```
$ wip
✓ Scanning for sessions...

IN-PROGRESS SESSIONS:
  1. Project X Analysis          [claude-code]      Modified 47 min ago
     Designing REST API endpoints. Discussed auth strategy, debated pagination.
     Left off: waiting on feedback about rate-limiting design.

  2. Data Processing             [claude-code]      Modified 2h ago
     Processing customer data imports. CSV parsing complete, now handling edge cases.
     Left off: debugging records with missing zip codes.

  3. Bug Investigation           [opencode]         Modified 5h ago
     Tracking down race condition in payment processor. Added logging, narrowed scope.
     Left off: need to review logs from staging environment.

Select session (1-3, or q to quit): 1
✓ Resuming: Project X Analysis
$ claude ~/.claude/sessions/abc123.jsonl
```

### Cron Scan Workflow
```cron
# Run scan every 2 hours
0 */2 * * * /usr/local/bin/wip scan
```

**Scan Process:**
1. Find all JSONL files in configured locations
2. Compare against index (skip unchanged files)
3. For modified sessions:
   - Read JSONL to get recent context
   - Call LLM: "Based on this conversation, is the user still working on this (in-progress) or has it been completed (done)?"
   - Store assessment result
4. Update index with new results
5. Exit

### Search/Filter Workflow
```
$ wip project
✓ Showing sessions matching "project"

IN-PROGRESS SESSIONS:
  1. Project X Analysis          [claude-cli]       Modified 14:22
  
Select session (1, or q to quit): 1
```

## 7. Initial Implementation Scope

### Phase 1 (MVP)
- **User Mode**: Interactive TUI for browsing and resuming in-progress sessions
- **Scan Mode**: Token-efficient scanning with Rust pre-filtering and timestamp-based caching
  - JSONL parsing and field extraction (Rust)
  - Modification time tracking and smart skip logic
  - LLM assessment only on modified files
- Claude Code provider support with configurable CLI launcher
- OpenCode provider support
- JSON-based index storage with mtime tracking
- Keychain integration for API keys
- Configuration file parsing
- Token usage tracking and cost estimation

### Phase 2
- Search/filter in user mode
- Parallel scanning for multiple providers
- Session export/reporting
- Performance optimizations (batch assessment requests)

### Phase 3
- Additional providers (Gemini, Codex, others)
- Advanced filtering and sorting options
- TUI improvements (pagination, detailed views, session details screen)

## 8. Technical Requirements

### Language & Build
- **Language**: Rust
- **Distribution**: Single binary (no runtime dependencies)
- **Minimum Rust**: 1.70+

### Key Dependencies
- **CLI/TUI**: `clap` (argument parsing), `crossterm` or `termion` (terminal control)
- **JSON**: `serde` / `serde_json`
- **Networking**: `reqwest` (HTTP client)
- **Glob patterns**: `glob`
- **Keychain**: `keyring` crate (macOS/Linux/Windows support)
- **API clients**: `anthropic-sdk` or direct HTTP for Claude; `openai-api-rs` or HTTP for OpenAI

### API Integration
- Claude Code: Use Anthropic SDK (Rust) for assessment
- OpenCode: Use raw HTTP API for assessment
- Assessment: Prompt LLM with recent conversation context, parse response for "in-progress" or "done"
- Credentials: Retrieve from system keychain at runtime, never store in config

### File Operations
- Glob patterns for discovering JSONL files
- JSONL parsing (line-by-line JSON)
- File modification time tracking
- Atomic writes for index updates

## 9. Configuration

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
    "assessmentModel": "claude-3-opus-20250219",
    "assessmentApiKey": {
      "keychainKey": "wip-claude-api-key"
    },
    "assessmentPrompt": "Analyze the conversation and provide:\n1. status: 'in-progress' or 'done'\n2. summary: 1-2 sentences (20-30 words) about the topic/goal\n3. left_off: 1 sentence (10-15 words) about the last action or next step\n\nKeep responses scannable and concise. Reply in format: status: X\nsummary: Y\nleft_off: Z",
    "pricing": {
      "inputTokensPerMillion": 3,
      "outputTokensPerMillion": 15
    }
  },
  "storageDir": "~/.wip",
  "indexRefreshThreshold": 3600
}
```

### Config Notes

- **sessionPatterns**: Glob patterns to find session files. Can have multiple per provider.
- **cliLauncher**: Command template to resume a session. `{sessionPath}` is replaced with the actual file path.
- **assessmentModel**: Single LLM model used for all session assessments (e.g., `claude-3-opus-20250219`).
- **assessmentApiKey.keychainKey**: Keychain entry storing the assessment model's API key.
- **assessmentPrompt**: LLM prompt for determining session status. Can be customized.
- **pricing**: Optional. Per-million token costs (in dollars) for input/output of the assessment model. Used to calculate estimated costs.
- **indexRefreshThreshold**: Seconds before index is considered stale (default 1 hour).

## 10. Assessment Logic (Scan Mode)

### Token-Efficient Scanning Strategy

**Philosophy**: Use Rust to filter/extract, use LLM only for synthesis. Minimize token consumption.

### Two-Phase Assessment

#### Phase 1: Rust-Based Pre-filtering
For each session file:

1. **Check modification time**:
   - If file hasn't changed since last assessment → skip, use cached result
   - If file was modified very recently (< 5 min) → skip, defer until next scan
   
2. **Parse JSONL and extract only relevant fields**:
   - Read entire file, but extract only: `timestamp`, `role` (user/assistant), `content`
   - Discard metadata, formatting, unused fields
   - Extract **last 5-10 user messages** and **last 5 assistant responses**
   - Also extract **first user message** (session topic context)
   
3. **Build minimal context block**:
   ```
   First message: [user's initial message ~50-100 words]
   
   Recent (last ~30 messages):
   [timestamps and role:content pairs, omitting verbose output]
   ```
   
4. **Calculate approximate token count**:
   - Rough estimate: 1 token ≈ 4 chars
   - If estimated tokens > threshold (e.g., 500), truncate more aggressively

#### Phase 2: LLM Assessment (Only on Modified Files)
Send pre-filtered content to the configured assessment model (same model for all sessions, all providers):

```
Analyze this session snippet and provide:
1. status: 'in-progress' or 'done'
2. summary: 1-2 sentences (20-30 words) about the topic/goal
3. left_off: 1 sentence (10-15 words) about the last action/next step

Recent session:
[pre-filtered content from Phase 1]

Reply exactly as:
status: in-progress
summary: [text]
left_off: [text]
```

**Token Tracking**:
- Capture `usage` from LLM response (input_tokens, output_tokens, total_tokens)
- Store per-session: `assessment.tokensUsed`, `assessment.inputTokens`, `assessment.outputTokens`
- Track aggregate stats: total tokens, number of assessments run vs. skipped
- Calculate estimated cost based on assessment model's configured pricing

### Caching & Optimization
- **Timestamp tracking in index**:
  ```json
  {
    "path": "/path/to/session.jsonl",
    "lastFileModified": 1715335200,
    "lastScanned": 1715338800,
    "assessment": {...}
  }
  ```
- **Skip conditions**:
  - File mtime ≤ lastFileModified → skip (no changes)
  - File age < 5 minutes → skip (still writing)
  - Assessment exists and file unchanged → use cached result
  
- **Batch processing**: If many files need assessment, batch them (e.g., 5 files per LLM request) to reduce overhead
- **Incremental scans**: Only process new/modified files; leave unchanged files alone

### Expected Token Savings
- **Before**: ~500-2000 tokens per session (full conversation)
- **After**: ~200-400 tokens per session (pre-filtered + first message)
- **With caching**: Most scans reuse cached assessments (0 tokens)

### Edge Cases
- Empty JSONL: Skip
- Malformed JSONL: Log error, keep previous assessment if exists
- File still being written: Skip, retry next scan
- LLM API failures: Keep previous assessment, flag for retry
- Parse failures: Use previous summary if available
