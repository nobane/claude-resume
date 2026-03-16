use serde::Deserialize;
use std::process::Command;

use crate::config::HostConfig;
use crate::session::Turn;

#[derive(Deserialize)]
struct RemoteSessionJson {
    id: String,
    project: String,
    last_ts: u64,
    msg_count: usize,
    first_msg: String,
    last_msg: String,
    last_cwd: Option<String>,
    active_pid: Option<u32>,
    messages: Vec<Turn>,
}

#[derive(Deserialize)]
struct RemoteOutput {
    sessions: Vec<RemoteSessionJson>,
}

pub struct RemoteSession {
    pub id: String,
    pub project: String,
    pub last_ts: u64,
    pub msg_count: usize,
    #[allow(dead_code)]
    pub first_msg: String,
    pub last_msg: String,
    pub last_cwd: Option<String>,
    pub active_pid: Option<u32>,
    pub messages: Vec<Turn>,
    #[allow(dead_code)]
    pub host: String,
}

pub fn fetch_remote_sessions(host: &HostConfig) -> Result<Vec<RemoteSession>, String> {
    let output = Command::new("ssh")
        .arg("-o")
        .arg("ConnectTimeout=5")
        .arg(&host.ssh)
        .arg("~/.local/bin/claude-resume --json")
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("SSH error: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let remote: RemoteOutput =
        serde_json::from_str(&stdout).map_err(|e| format!("Parse error: {}", e))?;

    Ok(remote
        .sessions
        .into_iter()
        .map(|s| RemoteSession {
            id: s.id,
            project: s.project,
            last_ts: s.last_ts,
            msg_count: s.msg_count,
            first_msg: s.first_msg,
            last_msg: s.last_msg,
            last_cwd: s.last_cwd,
            active_pid: s.active_pid,
            messages: s.messages,
            host: host.name.clone(),
        })
        .collect())
}

/// Check if a PID is still running on the remote host
pub fn is_remote_pid_alive(ssh_host: &str, pid: u32) -> bool {
    Command::new("ssh")
        .arg("-o")
        .arg("ConnectTimeout=5")
        .arg(ssh_host)
        .arg(format!("kill -0 {}", pid))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn kill_remote_pid(ssh_host: &str, pid: u32) -> Result<(), String> {
    let output = Command::new("ssh")
        .arg(ssh_host)
        .arg(format!("kill {}", pid))
        .output()
        .map_err(|e| format!("SSH kill failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Kill error: {}", stderr.trim()));
    }
    Ok(())
}

/// Run an SSH command, optionally wrapped with waypipe for GPU/GUI forwarding.
fn run_ssh(ssh_host: &str, remote_cmd: &str, gpu: bool) -> std::process::ExitStatus {
    if gpu {
        Command::new("waypipe")
            .arg("--compress")
            .arg("zstd")
            .arg("ssh")
            .arg("-t")
            .arg(ssh_host)
            .arg(remote_cmd)
            .status()
            .unwrap_or_else(|_| std::process::ExitStatus::default())
    } else {
        Command::new("ssh")
            .arg("-t")
            .arg(ssh_host)
            .arg(remote_cmd)
            .status()
            .unwrap_or_else(|_| std::process::ExitStatus::default())
    }
}

pub fn open_remote_session_by_id(ssh_host: &str, session_id: &str, project: &str, gpu: bool) -> std::process::ExitStatus {
    let short_id = &session_id[..8.min(session_id.len())];

    if gpu {
        // With waypipe: always create a fresh tmux session so Claude inherits
        // WAYLAND_DISPLAY from the waypipe shell. Use a gpu-prefixed name to
        // avoid colliding with any existing non-waypipe tmux session.
        let tmux_name = format!("gpu-claude-{}", short_id);
        let ssh_cmd = format!(
            "export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 && \
             /usr/bin/tmux kill-session -t {} 2>/dev/null; \
             /usr/bin/tmux new-session -s {} -c {} \"$HOME/.local/bin/claude --dangerously-skip-permissions --resume {}\"",
            shell_escape(&tmux_name),
            shell_escape(&tmux_name),
            shell_escape(project),
            session_id,
        );
        run_ssh(ssh_host, &ssh_cmd, gpu)
    } else {
        let tmux_name = format!("claude-{}", short_id);
        let ssh_cmd = format!(
            "export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 && \
             /usr/bin/tmux attach-session -t {} 2>/dev/null || \
             /usr/bin/tmux new-session -s {} -c {} \"$HOME/.local/bin/claude --dangerously-skip-permissions --resume {}\"",
            shell_escape(&tmux_name),
            shell_escape(&tmux_name),
            shell_escape(project),
            session_id,
        );
        run_ssh(ssh_host, &ssh_cmd, gpu)
    }
}

pub fn fetch_remote_dirs(ssh_host: &str) -> Result<Vec<crate::session::DirEntry>, String> {
    let output = Command::new("ssh")
        .arg("-o")
        .arg("ConnectTimeout=5")
        .arg(ssh_host)
        .arg("~/.local/bin/claude-resume --list-dirs")
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("SSH error: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    #[derive(Deserialize)]
    struct RemoteDirEntry {
        path: String,
        has_claude_md: bool,
        has_git: bool,
    }

    #[derive(Deserialize)]
    struct RemoteDirsOutput {
        dirs: Vec<RemoteDirEntry>,
    }

    let remote: RemoteDirsOutput =
        serde_json::from_str(&stdout).map_err(|e| format!("Parse error: {}", e))?;

    Ok(remote
        .dirs
        .into_iter()
        .map(|d| {
            let display = d.path.clone(); // Remote paths shown as-is
            let mut score: i32 = 0;
            if d.has_claude_md {
                score += 100;
            }
            if d.has_git {
                score += 10;
            }
            crate::session::DirEntry {
                path: d.path,
                display,
                has_claude_md: d.has_claude_md,
                has_git: d.has_git,
                score,
            }
        })
        .collect())
}

pub fn open_new_remote_session(ssh_host: &str, dir: &str, gpu: bool) -> std::process::ExitStatus {
    let ssh_cmd = format!(
        "export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 && /usr/bin/tmux new-session -c {} \"$HOME/.local/bin/claude --dangerously-skip-permissions\"",
        shell_escape(dir),
    );

    run_ssh(ssh_host, &ssh_cmd, gpu)
}

pub fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn test_shell_escape_spaces() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn test_shell_escape_single_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn test_shell_escape_special_chars() {
        assert_eq!(shell_escape("a\"b$c"), "'a\"b$c'");
    }
}
