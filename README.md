# claude-resume

TUI for browsing and resuming [Claude Code](https://claude.com/claude-code) sessions — local and remote.

![Rust](https://img.shields.io/badge/rust-stable-orange) ![Platform](https://img.shields.io/badge/platform-linux-blue)

## What it does

Claude Code saves session history but doesn't have a built-in way to browse or resume old sessions. `claude-resume` reads your session data and gives you a fast TUI to:

- Browse all sessions with timestamps, project paths, and message previews
- Expand conversations to see full user/assistant message history
- Resume any session with Enter (launches `claude --resume`)
- Focus active sessions (jumps to the running terminal window)
- Filter and search across sessions
- Group sessions by project folder
- Connect to remote hosts over SSH and browse/resume sessions there

## Install

```bash
cargo install --path .
```

Or build manually:
```bash
cargo build --release
cp target/release/claude-resume ~/.local/bin/
```

## Usage

```bash
claude-resume
```

### Keybinds

| Key | Action |
|-----|--------|
| `j/k` or arrows | Navigate |
| `Enter` | Resume session / enter folder / connect to host |
| `l/h` | Expand/collapse conversation messages |
| `a` | All sessions view |
| `f` | Folders (projects) view |
| `r` | Remote hosts view |
| `n` | New session (directory picker) |
| `/` | Filter/search |
| `Tab` | Cycle views |
| `Esc` | Back / collapse |
| `g/G` | Jump to top/bottom |
| `t` | Toggle tmux mode (wrap new sessions in tmux) |
| `q` | Quit |

### tmux Support

Sessions can run inside tmux for persistence — if your terminal closes, the session keeps running and can be reattached.

**Local tmux mode**: Press `t` to toggle. When enabled, new sessions and resumes launch inside named tmux sessions (`claude-{id}`). Active tmux sessions show `●T` in the session list and can be reattached with Enter.

**Auto-detection**: claude-resume automatically detects sessions already running inside tmux (regardless of the toggle) and reattaches with `tmux attach -d` when you press Enter.

#### tmux Configuration

Claude Code requires specific tmux settings to work properly. Without these, keybinds (especially Ctrl+Enter to submit) break and rendering is wrong.

Create `~/.tmux.conf`:

```
set -g allow-passthrough on
set -g default-terminal "tmux-256color"
set -ag terminal-overrides ",foot:RGB"
set -ag terminal-overrides ",xterm-256color:RGB"
set -sg escape-time 0
set -g mouse on
set -g set-clipboard on
set -g focus-events on
set -g extended-keys on
set -as terminal-features "xterm*:extkeys"
set -g history-limit 250000
```

**Critical settings explained:**
- `allow-passthrough on` — Claude Code uses passthrough escape sequences for its TUI
- `escape-time 0` — default 500ms delay on Escape makes the TUI feel laggy
- `extended-keys on` + `terminal-features extkeys` — required for Ctrl+Enter (submit) to pass through tmux to Claude Code
- `set-clipboard on` — enables clipboard sharing via OSC 52

**tmux version**: 3.2+ required for `extended-keys`, 3.3+ for `allow-passthrough`. Arch `tmux` package (3.6+) works. On Ubuntu/Debian you may need to build from git.

### Remote Sessions

Configure remote hosts in `~/.config/claude-resume/hosts.toml`:

```toml
[[host]]
name = "my-server"
ssh = "user@hostname"
```

Press `r` to see remote hosts, Enter to connect. The remote host needs `claude-resume` installed at `~/.local/bin/claude-resume` — it runs `--json` over SSH to fetch sessions.

Remote sessions are resumed inside tmux on the remote host. If you disconnect, the tmux session keeps running. Reconnecting reattaches instead of killing the process.

## How it works

Session data comes from:
- `~/.claude/history.jsonl` — user message history (timestamps, projects, session IDs)
- `~/.claude/projects/<dir>/<id>.jsonl` — full conversation transcripts (user + assistant turns)
- `~/.claude/sessions/*.json` — PID files for active sessions

Active session detection uses `/proc/<pid>` to check liveness and read `--resume` arguments. Active sessions show their workspace number and can be focused directly.

## Features

- [x] Browse and resume local sessions
- [x] Full conversation history (user + assistant messages)
- [x] Active session detection and window focus (Hyprland + i3)
- [x] Project grouping and filtering
- [x] Remote session browsing and resume over SSH
- [x] Remote new session with directory picker
- [x] Local tmux mode (persistent sessions that survive terminal close)
- [x] Auto-detection of tmux sessions with reattach
- [x] tmux session persistence for remote sessions
- [x] Loading indicators for remote connections
- [x] SSH error surfacing in TUI

## Window Manager Support

The window manager is auto-detected at runtime:

| WM | Detection | Window Focus | Workspace Tracking | Notes |
|----|-----------|-------------|-------------------|-------|
| Hyprland | `HYPRLAND_INSTANCE_SIGNATURE` env | `hyprctl dispatch focuswindow` | Yes | Native JSON API |
| i3 | `I3SOCK` env | `i3-msg [con_id=N] focus` | Yes | Uses `xdotool` for PID resolution |

**i3 requirements**: `xdotool` must be installed (`sudo pacman -S xdotool` / `sudo apt install xdotool`). Used to resolve X11 window IDs to process PIDs. For multi-window terminals like WezTerm, `wezterm cli` is used for TTY-based disambiguation.

Core session browsing works without any window manager. Active session detection requires Linux (`/proc`).

## Config

| File | Purpose |
|------|---------|
| `~/.config/claude-resume/hosts.toml` | Remote host definitions |
| `~/.config/claude-resume/recent-dirs.json` | Recently used project directories |

## License

MIT
