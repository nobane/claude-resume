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

/// Build an SSH command with multiplexing and optional port
fn ssh_command_with_port(ssh_host: &str, port: Option<u16>) -> Command {
    let mut cmd = Command::new("ssh");
    for arg in ssh_multiplex_args() {
        cmd.arg(arg);
    }
    cmd.arg("-o").arg("ConnectTimeout=5");
    if let Some(p) = port {
        cmd.arg("-p").arg(p.to_string());
    }
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
    #[serde(default)]
    in_tmux: Option<bool>,
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
    pub in_tmux: bool,
    pub messages: Vec<Turn>,
    #[allow(dead_code)]
    pub host: String,
}

pub fn fetch_remote_sessions(host: &HostConfig) -> Result<Vec<RemoteSession>, String> {
    let resume_bin = host.remote_resume_bin();
    let output = ssh_command_with_port(&host.ssh, host.port)
        .arg(format!("{} --json", resume_bin))
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
            in_tmux: s.in_tmux.unwrap_or(false),
            messages: s.messages,
            host: host.name.clone(),
        })
        .collect())
}

/// Check if a PID is still running on the remote host
pub fn is_remote_pid_alive(ssh_host: &str, port: Option<u16>, pid: u32) -> bool {
    ssh_command_with_port(ssh_host, port)
        .arg(format!("kill -0 {}", pid))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn kill_remote_pid(ssh_host: &str, port: Option<u16>, pid: u32) -> Result<(), String> {
    let output = ssh_command_with_port(ssh_host, port)
        .arg(format!("kill {}", pid))
        .output()
        .map_err(|e| format!("SSH kill failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Kill error: {}", stderr.trim()));
    }
    Ok(())
}

/// Kill a tmux session on the remote host.
pub fn kill_remote_tmux(ssh_host: &str, port: Option<u16>, session_id: &str, tmux_bin: &str) -> Result<(), String> {
    let short_id = &session_id[..8.min(session_id.len())];
    let tmux_name = format!("claude-{}", short_id);
    let output = ssh_command_with_port(ssh_host, port)
        .arg(format!("{} kill-session -t {}", tmux_bin, shell_escape(&tmux_name)))
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tmux kill error: {}", stderr.trim()));
    }
    Ok(())
}

/// Check if a session's tmux session exists on the remote host.
pub fn is_in_tmux_session(ssh_host: &str, port: Option<u16>, session_id: &str, tmux_bin: &str) -> bool {
    let short_id = &session_id[..8.min(session_id.len())];
    let tmux_name = format!("claude-{}", short_id);
    ssh_command_with_port(ssh_host, port)
        .arg(format!("{} has-session -t {}", tmux_bin, shell_escape(&tmux_name)))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn open_remote_session_by_id(host: &HostConfig, session_id: &str, project: &str) -> std::process::ExitStatus {
    let short_id = &session_id[..8.min(session_id.len())];
    let tmux_name = format!("claude-{}", short_id);
    let tmux_bin = host.remote_tmux_bin();
    let claude_bin = host.remote_claude_bin();

    let ssh_cmd = format!(
        "export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 && {tmux} attach-session -t {name} 2>/dev/null || {tmux} new-session -s {name} -c {dir} \"{claude} --dangerously-skip-permissions --resume {sid}\"",
        tmux = tmux_bin,
        name = shell_escape(&tmux_name),
        dir = shell_escape(project),
        claude = claude_bin,
        sid = session_id,
    );

    let mut cmd = Command::new("ssh");
    cmd.arg("-t");
    for arg in ssh_multiplex_args() {
        cmd.arg(arg);
    }
    if let Some(p) = host.port {
        cmd.arg("-p").arg(p.to_string());
    }
    cmd.arg(&host.ssh);
    cmd.arg(&ssh_cmd);
    cmd.status().unwrap_or_else(|_| std::process::ExitStatus::default())
}

/// Fetch turns for a single remote session on-demand.
pub fn fetch_remote_messages(host: &HostConfig, session_id: &str) -> Result<Vec<Turn>, String> {
    let resume_bin = host.remote_resume_bin();
    let output = ssh_command_with_port(&host.ssh, host.port)
        .arg(format!("{} --json-messages {}", resume_bin, shell_escape(session_id)))
        .output()
        .map_err(|e| format!("SSH failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("SSH error: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("Parse error: {}", e))
}

pub fn fetch_remote_dirs(host: &HostConfig) -> Result<Vec<crate::session::DirEntry>, String> {
    let resume_bin = host.remote_resume_bin();
    let output = ssh_command_with_port(&host.ssh, host.port)
        .arg(format!("{} --list-dirs", resume_bin))
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
            let display = d.path.clone();
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

pub fn open_new_remote_session(host: &HostConfig, dir: &str) -> std::process::ExitStatus {
    let tmux_bin = host.remote_tmux_bin();
    let claude_bin = host.remote_claude_bin();
    let ssh_cmd = format!(
        "export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 && {tmux} new-session -c {dir} \"{claude} --dangerously-skip-permissions\"",
        tmux = tmux_bin,
        dir = shell_escape(dir),
        claude = claude_bin,
    );

    let mut cmd = Command::new("ssh");
    cmd.arg("-t");
    for arg in ssh_multiplex_args() {
        cmd.arg(arg);
    }
    if let Some(p) = host.port {
        cmd.arg("-p").arg(p.to_string());
    }
    cmd.arg(&host.ssh);
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

    #[test]
    fn test_shell_escape_newlines() {
        assert_eq!(shell_escape("a\nb"), "'a\nb'");
    }

    #[test]
    fn test_shell_escape_backticks() {
        assert_eq!(shell_escape("$(cmd)"), "'$(cmd)'");
    }
}
