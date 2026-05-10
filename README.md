# wip

Find and resume your in-progress LLM sessions.

If you work with multiple Claude Code (or other LLM CLI) sessions simultaneously, you know the problem: too many terminal tabs, hard to keep track of what you were working on, painful to close anything because getting back to context takes effort.

**wip** makes it safe to close sessions. It scans your session files in the background, uses an LLM to summarize each one, and gives you a fast picker to find and resume exactly where you left off.

## How it works

1. `wip scan` finds your Claude Code session files, summarizes each one with Claude Haiku, and stores the results in a local index (`~/.wip/index.json`)
2. `wip` opens an fzf picker showing your in-progress sessions with summaries — select one and it resumes instantly in your current terminal

The index is pre-computed, so the picker is always instant. Run `wip scan` on a cron schedule to keep the index fresh automatically.

## Requirements

- [fzf](https://github.com/junegunn/fzf) — `brew install fzf`
- An Anthropic API key **or** Google Cloud credentials via Vertex AI (see [Configuration](#configuration))
- [Rust](https://rustup.rs/) (to build from source)

## Installation

```bash
git clone https://github.com/kenliu/wip
cd wip
cargo build --release
cp target/release/wip /usr/local/bin/wip
```

## Setup

Set your Anthropic API key:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
```

Add to your shell profile (`~/.zshrc` or `~/.bashrc`) to make it permanent.

Run an initial scan:

```bash
wip scan
```

## Usage

```bash
wip               # Open session picker
wip scan          # Scan for new/updated sessions
wip scan --force  # Re-summarize all sessions
```

### Recommended: run scan on a cron schedule

```cron
# Scan every 10 minutes
*/10 * * * * /usr/local/bin/wip scan
```

This keeps the index fresh so the picker always shows up-to-date summaries.

## Session picker

```
wip > 
  wip          Implementing MVP session scanner and fzf picker      8m ago  ↩ fix unicode panic in truncate
  todoist      Debugging race condition in sync engine             2h ago  ↩ review logs from staging
  todoist      Adding pagination to task list API                  4h ago  ↩ write tests for edge cases
  flipboard    Building article card component                     1d ago  ↩ handle missing thumbnail case
```

Type to fuzzy-filter. Press Enter to resume the selected session. The screen clears and Claude picks up where you left off.

## What gets scanned

- Claude Code sessions: `~/.claude/projects/**/*.jsonl`
- Sessions modified more than 30 seconds ago (to avoid files still being written)
- Sessions modified within the last 30 days
- Subagent sessions (`agent-*`) are ignored

## Storage

```
~/.wip/
├── index.json        # Session index with pre-computed summaries
└── scan.log.jsonl    # Scan history (one JSON entry per run)
```

## Configuration

wip reads `~/.wip/config.json` if present. Without a config file it falls back to `ANTHROPIC_API_KEY` and the default model.

### Vertex AI backend

If you authenticate via Google Cloud (enterprise accounts, GCP billing), you can use Claude through Vertex AI instead of a direct Anthropic API key.

**Prerequisites:** install the [Google Cloud SDK](https://cloud.google.com/sdk/docs/install) and run:

```bash
gcloud auth application-default login
```

**Config (`~/.wip/config.json`):**

```json
{
  "scan": {
    "summary_backend": "vertex",
    "vertex_project_id": "my-gcp-project",
    "vertex_region": "us-east5",
    "summary_model": "claude-sonnet-4-6"
  }
}
```

| Field | Required | Default | Description |
|---|---|---|---|
| `summary_backend` | no | `"anthropic"` | `"anthropic"` or `"vertex"` |
| `vertex_project_id` | when using vertex | — | GCP project ID |
| `vertex_region` | no | `"us-east5"` | Vertex AI region |
| `summary_model` | no | `"claude-sonnet-4-6"` | Model name (Anthropic format; translated to Vertex format automatically) |

Model names are translated automatically — for example `claude-sonnet-4-6` becomes `claude-sonnet-4-6@20250514` on Vertex. If you need a specific version, set `summary_model` to the full Vertex model ID (e.g. `claude-sonnet-4-6@20250514`) and it will be used as-is.

No API key is stored or needed when using the Vertex backend — credentials come from ADC (`gcloud auth application-default login` or `GOOGLE_APPLICATION_CREDENTIALS`).

## Status

Early MVP. Currently supports Claude Code sessions only. OpenCode and other providers coming later.
