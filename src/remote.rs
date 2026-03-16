use serde::Deserialize;
use std::process::Command;

#[derive(Deserialize, Clone)]
pub struct HostConfig {
    pub name: String,
    pub ssh: String,
}

#[derive(Deserialize, Clone)]
struct HostsFile {
    #[serde(default)]
    host: Vec<HostConfig>,
}

pub fn load_hosts() -> Vec<HostConfig> {
    let path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/claude-resume/hosts.toml");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    match toml::from_str::<HostsFile>(&content) {
        Ok(f) => f.host,
        Err(_) => vec![],
    }
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
    messages: Vec<String>,
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
    pub first_msg: String,
    pub last_msg: String,
    pub last_cwd: Option<String>,
    pub active_pid: Option<u32>,
    pub messages: Vec<String>,
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

pub fn open_remote_session(ssh_host: &str, session: &RemoteSession) {
    let cwd = session
        .last_cwd
        .as_deref()
        .unwrap_or(&session.project);
    let short_id = &session.id[..8.min(session.id.len())];
    let tmux_name = format!("claude-{}", short_id);

    // Full paths since non-interactive SSH doesn't load shell profile
    // Set UTF-8 locale so box-drawing chars and icons render correctly
    // If tmux session already exists, reattach; otherwise create new
    let ssh_cmd = format!(
        "export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 TERM=xterm-256color && /usr/bin/tmux attach-session -t {} 2>/dev/null || (cd {} && /usr/bin/tmux new-session -s {} '~/.local/bin/claude --dangerously-skip-permissions --resume {}')",
        shell_escape(&tmux_name),
        shell_escape(cwd),
        shell_escape(&tmux_name),
        shell_escape(&session.id),
    );

    let _ = Command::new("foot")
        .arg("-e")
        .arg("ssh")
        .arg("-t")
        .arg(ssh_host)
        .arg(&ssh_cmd)
        .spawn();
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
