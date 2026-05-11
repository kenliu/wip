# wip

Find and resume your in-progress LLM sessions.

If you work with multiple Claude Code (or other LLM CLI) sessions simultaneously, you know the problem: too many terminal tabs, hard to keep track of what you were working on, painful to close anything because getting back to context takes effort.

**wip** makes it safe to close sessions. It scans your session files in the background, uses an LLM to summarize each one, and gives you a fast picker to find and resume exactly where you left off.

## How it works

1. `wip scan` finds your Claude Code session files, summarizes each one with Claude, and stores the results in a local index (`~/.wip/index.json`)
2. `wip` opens an interactive TUI showing your sessions with summaries — select one and it resumes instantly in your current terminal

The index is pre-computed, so the TUI is always instant. Use `wip install` to set up a launchd agent that keeps the index fresh automatically, or run `wip scan` on a cron schedule.

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
wip                        # Open interactive TUI session browser
wip --background-scan      # TUI + run a scan silently in the background
wip fast                   # fzf-powered minimal picker (requires fzf)
wip scan                   # Scan for new/updated sessions
wip scan --force           # Re-summarize all sessions
wip install                # Install launchd agent for automatic scanning (macOS)
wip uninstall              # Remove the launchd agent
wip stats                  # Show token usage and scan history
wip index clear            # Delete the index (forces full rescan on next scan)
```

### Recommended: automatic background scanning

**macOS (launchd)** — runs `wip scan` every 10 minutes, starts at login:

```bash
wip install
```

**Cron (all platforms):**

```cron
# Scan every 10 minutes
*/10 * * * * /usr/local/bin/wip scan
```

## TUI

`wip` opens a full-screen TUI. Use arrow keys to browse, Enter to resume.

Key bindings:
- `↑`/`↓` — navigate sessions
- `Enter` — resume selected session
- `→`/`←` — toggle right-pane chat preview
- `a` — toggle show-all (include done sessions)
- `f` — flag/unflag session
- `F` — show only flagged sessions
- `x` — mark session as done
- `q` / `Esc` — quit

## fzf picker (`wip fast`)

`wip fast` is a lightweight fzf-based alternative for keyboard-optimized selection. Requires `fzf` (`brew install fzf`).

```
wip > 
  wip          Implementing MVP session scanner and TUI             8m ago  ↩ fix unicode panic in truncate
  todoist      Debugging race condition in sync engine             2h ago  ↩ review logs from staging
  todoist      Adding pagination to task list API                  4h ago  ↩ write tests for edge cases
  flipboard    Building article card component                     1d ago  ↩ handle missing thumbnail case
```

Type to fuzzy-filter. Press Enter to resume. The screen clears and Claude picks up where you left off.

## What gets scanned

- Claude Code sessions: `~/.claude/projects/**/*.jsonl`
- Sessions modified more than 30 seconds ago (to avoid files still being written)
- Sessions modified within the last 30 days
- Subagent sessions (`agent-*`) are ignored

## Storage

```
~/.wip/
├── index.json        # Session index with pre-computed summaries
├── index.lock        # Advisory lock (prevents concurrent scans)
├── ui_state.json     # Persisted TUI toggle state
├── scan.log.jsonl    # Scan history (one JSON entry per run)
└── launchd.log       # launchd agent stdout/stderr (macOS only)
```

## Configuration

wip reads `~/.wip/config.json` if present. On first run of `wip scan` in a terminal, an interactive setup wizard creates it. Without a config file in non-interactive mode (cron, pipe), wip falls back to `ANTHROPIC_API_KEY` and the default model.

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
