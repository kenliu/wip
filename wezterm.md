# WezTerm CLI Reference

`wip` uses `wezterm cli spawn` to open sessions in new tabs/windows.

```bash
wezterm cli spawn [OPTIONS] [-- PROG...]
```

**Key options:**

| Flag | Description |
|---|---|
| `--cwd <CWD>` | Set working directory for the spawned program |
| `--new-window` | Spawn into a new window instead of a new tab |
| `--window-id <ID>` | Spawn tab into a specific window |
| `--workspace <NAME>` | Set workspace name (requires `--new-window`) |
| `--pane-id <ID>` | Override current pane (defaults to `$WEZTERM_PANE`) |
| `--domain-name <NAME>` | Spawn into named multiplexer domain |

**Examples:**
```bash
wezterm cli spawn                        # new tab, default shell
wezterm cli spawn -- claude              # new tab running claude
wezterm cli spawn --new-window -- bash   # new window
wezterm cli spawn --cwd /some/dir -- bash
```

Use `--` before the program args to avoid flag ambiguity. Returns the pane-id on success.
