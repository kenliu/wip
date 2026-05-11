# Code Review

## Dead code

**`keychain.rs` is an orphaned file.** It is not included via `mod keychain` in `main.rs`,
and `keyring` is not in `Cargo.toml`. It would fail to compile if ever included. Safe to
delete; the plist embed-API-key approach in `install_mode.rs` replaced it.

**`#[allow(dead_code)]` on `SummaryResponse`** (`lm_summarizer.rs:14`) is wrong — all five
fields are actively consumed in `scan_mode/mod.rs`. The attribute should be removed.

---

## Code duplication

**`format_age` exists in three files** with slightly different output strings:

- `tui.rs:34` — "3m ago" / "2h ago" / "1d ago"
- `fast_mode.rs:9` — same
- `stats_mode.rs:29` — "just now" / "3 min ago" / "2 hours ago" / "1 days ago"

**`project_name`** is duplicated between `tui.rs:59` and `fast_mode.rs:33`.

These belong in a shared `util.rs` module.

**JSONL parsing is duplicated** between `jsonl_parser.rs` and `tui.rs`. The
`extract_user_content`/`extract_assistant_content` pair in `jsonl_parser.rs` and the
`extract_preview_user`/`extract_preview_assistant` pair in `tui.rs` are nearly identical —
the TUI versions just add a `starts_with('<')` filter for meta-injections. The preview
loader in `tui.rs` re-implements the whole JSONL parsing loop instead of reusing
`jsonl_parser`.

**Agent-session filtering is duplicated** in `user_mode/mod.rs:49` and `fast_mode.rs:49`
even though the scanner never indexes agent sessions (it skips them at scan time and prunes
them after). These defensive filters are unreachable dead branches.

---

## Structural / design concerns

**`status` should be an enum, not a `String`.** The field is compared as a string literal
in at least six places across three files. A `SessionStatus { InProgress, Done }` enum
would eliminate the stringly-typed comparisons and make invalid states unrepresentable.

**Config fields are parsed but ignored.** Three fields in `Config` describe a flexible
multi-provider system that was never wired up:

- `providers: HashMap<String, ProviderConfig>` — the scanner hardcodes `"claude-code"` and
  the glob `~/.claude/projects/**/*.jsonl`
- `storage_dir` — `index_path()` and `lock_path()` hardcode `~/.wip`
- `index_refresh_threshold` — read from config, stored, never used

Either wire them up or drop them; as-is the config file implies capabilities that don't exist.

**`open in tab` (`o`) hardcodes WezTerm** (`tui.rs:672`). The footer shows it as a general
feature with no indication it's terminal-specific. If `wezterm` isn't installed,
`cmd.spawn()` fails silently. At minimum show an error in the TUI; ideally make the
launcher configurable via config.

**API key stored in plist plaintext** (`install_mode.rs`). The plist is world-readable by
default on macOS (mode 0644), so any process or user on the machine can read it from
`~/Library/LaunchAgents/`. The printed warning at install time is easy to miss.

---

## Bugs

**`timestamp_str()` in `scan_mode/mod.rs:43` produces incorrect timestamps.** The month
calculation `day_of_year / 30 + 1` can produce month 13 (for `day_of_year >= 360`), and
`year = 1970 + days / 365` drifts with accumulated leap years. Running today it prints
`2026-01-15` instead of `2026-05-10` — off by ~4 months. Since `unix_ts` is already in the
log and `stats_mode` reads it directly, the easiest fix is to drop `timestamp_str()` and
remove the `timestamp` field from the log (or just format from `unix_ts` using a
well-tested calculation).

**Scroll offset assumes fixed 6 lines per session** (`tui.rs:296`):

```rust
let per_page = ((list_height as usize) / 6).max(1);
```

Sessions with a `last_prompt` render 5 rows (header + summary + left_off + prompt + blank);
without one, 4 rows. The fixed divisor of 6 causes the selected item to partially scroll
out of view in many cases.

---

## Minor

**`list_area = area` in `render_session_list`** (`tui.rs:361`) is a no-op rename, a
leftover from when the function had a sub-area layout inside it.

**`filtered()` is called multiple times per frame** — once each in `render_header`,
`render_session_list`, `render_preview`, and also in `move_up`/`move_down` via
`filtered_count()`. For small lists this is fine, but caching it per frame or invalidating
on state change would be cleaner.

---

## Summary by priority

| Priority   | Issue |
|------------|-------|
| Fix        | `timestamp_str()` producing wrong dates |
| Fix        | Remove `#[allow(dead_code)]` from `SummaryResponse` |
| Fix        | Scroll offset with variable row heights |
| Clean up   | Delete `keychain.rs` |
| Clean up   | Extract `format_age` / `project_name` to `util.rs` |
| Clean up   | Drop unused config fields or wire them up |
| Improve    | `open in tab` should handle missing wezterm gracefully |
| Improve    | `status` as enum |
| Improve    | Deduplicate JSONL preview parsing with `jsonl_parser` |
