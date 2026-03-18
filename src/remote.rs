use serde::Deserialize;
use std::process::Command;

use crate::config::HostConfig;
use crate::session::Turn;

/// Directory for SSH ControlMaster sockets
fn ssh_control_dir() -> std::path::PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".cache/claude-resume/ssh");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Get the ControlPath for an SSH host.
/// Uses SSH tokens so the master and client connections agree on the socket name.
fn control_path() -> String {
    let dir = ssh_control_dir();
    format!("{}/%r@%h:%p", dir.display())
}

/// Common SSH args for connection multiplexing
fn ssh_multiplex_args() -> Vec<String> {
    let cp = control_path();
    vec![
        "-o".into(), format!("ControlPath={}", cp),
        "-o".into(), "ControlMaster=auto".into(),
        "-o".into(), "ControlPersist=60".into(),
    ]
}

/// Build an SSH command with multiplexing enabled
fn ssh_command(ssh_host: &str) -> Command {
    let mut cmd = Command::new("ssh");
    for arg in ssh_multiplex_args() {
        cmd.arg(arg);
    }
    cmd.arg("-o").arg("ConnectTimeout=5");
    cmd.arg(ssh_host);
    cmd
}

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
    let output = ssh_command(&host.ssh)
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
    ssh_command(ssh_host)
        .arg(format!("kill -0 {}", pid))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn kill_remote_pid(ssh_host: &str, pid: u32) -> Result<(), String> {
    let output = ssh_command(ssh_host)
        .arg(format!("kill {}", pid))
        .output()
        .map_err(|e| format!("SSH kill failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Kill error: {}", stderr.trim()));
    }
    Ok(())
}

/// Check if a session's tmux session exists on the remote host.
pub fn is_in_tmux_session(ssh_host: &str, session_id: &str) -> bool {
    let short_id = &session_id[..8.min(session_id.len())];
    let tmux_name = format!("claude-{}", short_id);
    ssh_command(ssh_host)
        .arg(format!("/usr/bin/tmux has-session -t {}", shell_escape(&tmux_name)))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn open_remote_session_by_id(ssh_host: &str, session_id: &str, project: &str) -> std::process::ExitStatus {
    let short_id = &session_id[..8.min(session_id.len())];
    let tmux_name = format!("claude-{}", short_id);

    let ssh_cmd = format!(
        "export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 && /usr/bin/tmux attach-session -t {} 2>/dev/null || /usr/bin/tmux new-session -s {} -c {} \"$HOME/.local/bin/claude --dangerously-skip-permissions --resume {}\"",
        shell_escape(&tmux_name),
        shell_escape(&tmux_name),
        shell_escape(project),
        session_id,
    );

    let mut cmd = Command::new("ssh");
    cmd.arg("-t");
    for arg in ssh_multiplex_args() {
        cmd.arg(arg);
    }
    cmd.arg(ssh_host);
    cmd.arg(&ssh_cmd);
    cmd.status().unwrap_or_else(|_| std::process::ExitStatus::default())
}

/// Fetch turns for a single remote session on-demand.
pub fn fetch_remote_messages(ssh_host: &str, session_id: &str) -> Result<Vec<Turn>, String> {
    let output = ssh_command(ssh_host)
        .arg(format!("~/.local/bin/claude-resume --json-messages {}", shell_escape(session_id)))
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("SSH error: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("Parse error: {}", e))
}

pub fn fetch_remote_dirs(ssh_host: &str) -> Result<Vec<crate::session::DirEntry>, String> {
    let output = ssh_command(ssh_host)
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

pub fn open_new_remote_session(ssh_host: &str, dir: &str) -> std::process::ExitStatus {
    let ssh_cmd = format!(
        "export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 && /usr/bin/tmux new-session -c {} \"$HOME/.local/bin/claude --dangerously-skip-permissions\"",
        shell_escape(dir),
    );

    let mut cmd = Command::new("ssh");
    cmd.arg("-t");
    for arg in ssh_multiplex_args() {
        cmd.arg(arg);
    }
    cmd.arg(ssh_host);
    cmd.arg(&ssh_cmd);
    cmd.status().unwrap_or_else(|_| std::process::ExitStatus::default())
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
