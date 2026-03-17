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
| `q` | Quit |

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

Active session detection uses `/proc/<pid>` to check liveness and read `--resume` arguments. On Wayland (Hyprland), active sessions show their workspace and can be focused directly.

## Features

- [x] Browse and resume local sessions
- [x] Full conversation history (user + assistant messages)
- [x] Active session detection and window focus (Hyprland)
- [x] Project grouping and filtering
- [x] Remote session browsing and resume over SSH
- [x] Remote new session with directory picker
- [x] tmux session persistence for remote sessions
- [x] Loading indicators for remote connections
- [x] SSH error surfacing in TUI

## Platform Support

| Feature | Linux | macOS | Windows |
|---------|-------|-------|---------|
| Session browsing | Yes | Untested | Untested |
| Active session detection | Yes (`/proc`) | Planned (`ps`) | Planned |
| Window focus | Hyprland | No | No |
| Remote SSH sessions | Yes | Should work | Needs SSH client |
| tmux integration | Yes | Yes | WSL only |

Core session browsing likely works anywhere Rust and Claude Code run. Active session detection and window focus are Linux-specific today.

## Config

| File | Purpose |
|------|---------|
| `~/.config/claude-resume/hosts.toml` | Remote host definitions |
| `~/.config/claude-resume/recent-dirs.json` | Recently used project directories |

## License

MIT
