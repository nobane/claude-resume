# claude-resume

TUI for browsing and resuming [Claude Code](https://claude.com/claude-code) sessions — local and remote.

Built for tmux-heavy workflows where Claude Code sessions run persistently across machines. SSH into a remote server, resume a session, disconnect, come back later — the session is still there.

![Rust](https://img.shields.io/badge/rust-stable-orange) ![Platform](https://img.shields.io/badge/platform-linux-blue)

## What it does

Claude Code saves session history but doesn't have a built-in way to browse or resume old sessions. `claude-resume` gives you a fast TUI to:

- Browse all sessions with timestamps, project paths, and message previews
- Resume any session with Enter (launches `claude --resume`)
- Connect to **remote hosts over SSH** and browse/resume sessions there
- Run sessions inside **tmux** for persistence — disconnect and reattach anytime
- Focus active sessions (jumps to the running terminal window)
- Kill active sessions (local or remote, tmux or bare)
- Auto-refresh — the UI updates as sessions start and stop
- Expand conversations to see full message history
- Filter and search across sessions
- Group sessions by project folder

## Install

```bash
cargo install --path .
```

Or build manually:
```bash
cargo build --release
cp target/release/claude-resume ~/.local/bin/
```

For remote hosts, build a static binary with musl:
```bash
cargo build --release --target x86_64-unknown-linux-musl
scp target/x86_64-unknown-linux-musl/release/claude-resume remote:~/.local/bin/
```

## Usage

```bash
claude-resume
```

### Keybinds

| Key | Action |
|-----|--------|
| `↑/↓` | Navigate |
| `Enter` | Resume / attach / focus / open |
| `←/→` | Expand/collapse conversation messages |
| `a` | All sessions view |
| `f` | Folders (projects) view |
| `r` | Remote hosts view |
| `n` | New session (directory picker) |
| `k` | Kill active session |
| `t` | Toggle tmux mode |
| `/` | Filter/search |
| `g/G` | Jump to top/bottom |
| `Tab` | Cycle views |
| `Esc` | Back / collapse |
| `q` | Quit |

## Remote Sessions

The main draw — manage Claude Code sessions across machines from a single TUI.

Configure remote hosts in `~/.config/claude-resume/hosts.toml`:

```toml
[[host]]
name = "my-server"
ssh = "user@hostname"

[[host]]
name = "gpu-box"
ssh = "user@gpu-host"
port = 2222
gpu = true
```

Press `r` to see remote hosts, Enter to connect. The remote host needs `claude-resume` installed — it runs `--json` over SSH to fetch session data.

**How remote sessions work:**
1. You select a remote session and press Enter
2. claude-resume SSHs in and attaches to the tmux session running Claude Code
3. You interact with Claude normally
4. When you disconnect (close terminal, lose connection), the tmux session keeps running
5. When you come back, claude-resume reattaches to the existing tmux session

You can also start new remote sessions with `n` — a directory picker fetches project directories from the remote host.

## tmux Integration

tmux is what makes sessions persistent. Without it, closing your terminal kills Claude. With it, sessions survive disconnects, reboots, and SSH drops.

**tmux mode**: Press `t` to toggle. When enabled, new sessions and resumes launch inside named tmux sessions (`claude-{id}`).

**Auto-detection**: claude-resume detects sessions already running inside tmux and reattaches with `tmux attach` when you press Enter. Active tmux sessions show `●T` in the session list.

**Process handoff**: When launching a session, claude-resume uses `exec()` to replace itself with the target process (tmux or claude). No orphaned parent process sitting in the foreground.

### tmux Configuration

Claude Code requires specific tmux settings. Without these, keybinds (especially Ctrl+Enter to submit) break.

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

**Critical settings:**
- `allow-passthrough on` — Claude Code uses passthrough escape sequences for its TUI
- `escape-time 0` — default 500ms Escape delay makes the TUI laggy
- `extended-keys on` + `terminal-features extkeys` — required for Ctrl+Enter (submit) to pass through tmux
- `set-clipboard on` — clipboard sharing via OSC 52

**tmux version**: 3.2+ for `extended-keys`, 3.3+ for `allow-passthrough`.

## How it works

Session data comes from:
- `~/.claude/history.jsonl` — user messages (timestamps, projects, session IDs)
- `~/.claude/projects/<dir>/<id>.jsonl` — full transcripts (user + assistant turns)
- `~/.claude/sessions/*.json` — PID files for active sessions

Active session detection checks `/proc/<pid>` for liveness and reads `--resume` arguments from `/proc/<pid>/cmdline` to map ephemeral resumed sessions back to their originals.

The UI auto-refreshes every 5 seconds, picking up sessions that start or stop externally.

## Window Manager Support

Active session focusing is supported on:

| WM | Focus Method | Workspace Tracking |
|----|-------------|-------------------|
| Hyprland | `hyprctl dispatch focuswindow` | Yes |
| i3 | `i3-msg [con_id=N] focus` | Yes (requires `xdotool`) |

Core session browsing works without any window manager. Active session detection requires Linux (`/proc`).

## Config

| File | Purpose |
|------|---------|
| `~/.config/claude-resume/hosts.toml` | Remote host definitions |
| `~/.config/claude-resume/recent-dirs.json` | Recently used project directories |

## License

MIT
