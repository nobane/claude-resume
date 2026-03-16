use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, BufRead},
    path::PathBuf,
    process::Command,
};

#[derive(Deserialize)]
struct HistoryEntry {
    display: Option<String>,
    timestamp: Option<u64>,
    project: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct HyprClient {
    pid: i64,
    workspace: HyprWorkspace,
    address: String,
}

#[derive(Deserialize)]
struct HyprWorkspace {
    #[allow(dead_code)]
    id: i64,
    name: String,
}

#[derive(Clone, Serialize)]
pub struct Turn {
    pub role: String, // "user" or "assistant"
    pub text: String,
}

// Deserialize Turn from either {"role":"...","text":"..."} or a plain string (legacy)
impl<'de> Deserialize<'de> for Turn {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct TurnVisitor;

        impl<'de> de::Visitor<'de> for TurnVisitor {
            type Value = Turn;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a Turn object or a string")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Turn, E> {
                Ok(Turn {
                    role: "user".into(),
                    text: v.to_string(),
                })
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Turn, E> {
                Ok(Turn {
                    role: "user".into(),
                    text: v,
                })
            }

            fn visit_map<M: de::MapAccess<'de>>(self, map: M) -> Result<Turn, M::Error> {
                #[derive(Deserialize)]
                struct TurnInner {
                    role: String,
                    text: String,
                }
                let inner = TurnInner::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(Turn {
                    role: inner.role,
                    text: inner.text,
                })
            }
        }

        deserializer.deserialize_any(TurnVisitor)
    }
}

/// Info about an active session's window
pub struct ActiveInfo {
    pub pid: u32,
    pub workspace: Option<String>,
    pub window_address: Option<String>,
    /// If this process was started with --resume <id>, the original session ID.
    pub resumed_from: Option<String>,
}

pub struct Session {
    pub id: String,
    pub project: String,
    pub last_ts: u64,
    pub msg_count: usize,
    pub first_msg: String,
    pub last_msg: String,
    pub last_cwd: Option<String>,
    pub active: Option<ActiveInfo>,
    pub messages: Vec<Turn>,
}

pub struct Project {
    pub path: String,
    pub session_count: usize,
    pub last_ts: u64,
}

#[derive(Deserialize)]
struct SessionEntry {
    cwd: Option<String>,
}

/// Walk up the process tree from a PID to find the terminal (foot) PID.
fn find_terminal_pid(mut pid: u32) -> Option<u32> {
    for _ in 0..20 {
        if pid <= 1 {
            return None;
        }
        let stat = fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
        let comm_start = stat.find('(')?;
        let comm_end = stat.rfind(')')?;
        let comm = &stat[comm_start + 1..comm_end];
        if comm == "foot" || comm == "footclient" {
            return Some(pid);
        }
        let rest = &stat[comm_end + 2..];
        let ppid: u32 = rest.split_whitespace().nth(1)?.parse().ok()?;
        pid = ppid;
    }
    None
}

/// Read the --resume argument from a process's cmdline to find the original session ID.
fn read_resume_arg(pid: u32) -> Option<String> {
    let cmdline = fs::read_to_string(format!("/proc/{}/cmdline", pid)).ok()?;
    let args: Vec<&str> = cmdline.split('\0').collect();
    let mut found_resume = false;
    for arg in &args {
        if found_resume {
            let id = arg.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        if *arg == "--resume" {
            found_resume = true;
        }
    }
    None
}

/// Get hyprland window info for active sessions.
/// Returns map of session_id -> ActiveInfo with workspace and window address.
pub fn find_active_sessions() -> HashMap<String, ActiveInfo> {
    let sessions_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude/sessions");

    let mut session_pids: Vec<(String, u32)> = Vec::new();

    if let Ok(entries) = fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let pid_file: serde_json::Value = match serde_json::from_str(&content) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let pid = match pid_file.get("pid").and_then(|v| v.as_u64()) {
                Some(p) => p as u32,
                None => continue,
            };
            let session_id = match pid_file.get("sessionId").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let is_running = unsafe { libc::kill(pid as i32, 0) == 0 };
            if is_running {
                session_pids.push((session_id, pid));
            }
        }
    }

    if session_pids.is_empty() {
        return HashMap::new();
    }

    let hypr_clients: Vec<HyprClient> = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or_default();

    let mut active = HashMap::new();
    for (sid, claude_pid) in session_pids {
        let terminal_pid = find_terminal_pid(claude_pid);
        let resumed_from = read_resume_arg(claude_pid);
        let mut info = ActiveInfo {
            pid: claude_pid,
            workspace: None,
            window_address: None,
            resumed_from,
        };

        if let Some(tpid) = terminal_pid {
            for client in &hypr_clients {
                if client.pid == tpid as i64 {
                    info.workspace = Some(client.workspace.name.clone());
                    info.window_address = Some(client.address.clone());
                    break;
                }
            }
        }

        active.insert(sid, info);
    }

    active
}

/// Returns (last_cwd, session_file_path) for each resumable session.
pub fn find_resumable_sessions() -> HashMap<String, (Option<String>, PathBuf)> {
    let projects_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude/projects");

    let mut resumable = HashMap::new();

    let entries = match fs::read_dir(&projects_dir) {
        Ok(e) => e,
        Err(_) => return resumable,
    };

    for project_entry in entries.flatten() {
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }
        let inner = match fs::read_dir(&project_path) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in inner.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                if path.to_string_lossy().contains("subagents") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let last_cwd = read_last_cwd(&path);
                    resumable.insert(stem.to_string(), (last_cwd, path.clone()));
                }
            }
        }
    }

    resumable
}

fn read_last_cwd(path: &std::path::Path) -> Option<String> {
    let file = fs::read_to_string(path).ok()?;
    for line in file.lines().rev() {
        if let Ok(entry) = serde_json::from_str::<SessionEntry>(line) {
            if let Some(cwd) = entry.cwd {
                return Some(cwd);
            }
        }
    }
    None
}

/// Read user/assistant turns from a session JSONL file.
fn read_session_turns(path: &std::path::Path) -> Vec<Turn> {
    let file = match fs::read_to_string(path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let mut turns: Vec<Turn> = Vec::new();
    // Track last assistant text to deduplicate streaming chunks
    let mut last_assistant_text: Option<String> = None;

    for line in file.lines() {
        let val: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let msg_type = match val.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => continue,
        };

        if msg_type != "user" && msg_type != "assistant" {
            continue;
        }

        let content = match val.get("message").and_then(|m| m.get("content")) {
            Some(c) => c,
            None => continue,
        };

        let text = extract_text_content(content);
        if text.is_empty() {
            continue;
        }

        if msg_type == "assistant" {
            // Claude streams multiple assistant entries per turn; keep the longest
            if let Some(ref prev) = last_assistant_text {
                if text.len() > prev.len() {
                    last_assistant_text = Some(text);
                    // Update the last turn in place
                    if let Some(last) = turns.last_mut() {
                        last.text = last_assistant_text.as_ref().unwrap().clone();
                    }
                }
            } else {
                last_assistant_text = Some(text.clone());
                turns.push(Turn {
                    role: "assistant".into(),
                    text,
                });
            }
        } else {
            // New user message resets assistant tracking
            last_assistant_text = None;
            turns.push(Turn {
                role: "user".into(),
                text,
            });
        }
    }

    turns
}

/// Extract text from a message content field (string or array of content blocks).
fn extract_text_content(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Array(arr) => {
            let texts: Vec<&str> = arr
                .iter()
                .filter_map(|block| {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        block.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            texts.join(" ").trim().to_string()
        }
        _ => String::new(),
    }
}

/// Build a map of session_id -> cwd from all PID files in ~/.claude/sessions/
fn build_session_cwd_map() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let sessions_dir = match dirs::home_dir() {
        Some(h) => h.join(".claude/sessions"),
        None => return map,
    };
    if let Ok(entries) = fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let (Some(sid), Some(cwd)) = (
                        val.get("sessionId").and_then(|v| v.as_str()),
                        val.get("cwd").and_then(|v| v.as_str()),
                    ) {
                        map.insert(sid.to_string(), cwd.to_string());
                    }
                }
            }
        }
    }
    map
}

pub fn load_sessions() -> Vec<Session> {
    let history_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude/history.jsonl");

    let file = match fs::File::open(&history_path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let resumable = find_resumable_sessions();
    let mut active = find_active_sessions();
    let session_cwd_map = build_session_cwd_map();

    // Remap active sessions that were started with --resume <original-id>:
    // move their ActiveInfo to the original session ID so the original session
    // shows as active (instead of a phantom entry with no messages).
    let remapped: Vec<(String, String)> = active
        .iter()
        .filter_map(|(sid, info)| {
            info.resumed_from.as_ref().map(|orig| (sid.clone(), orig.clone()))
        })
        .collect();
    for (ephemeral_sid, original_sid) in remapped {
        if let Some(mut info) = active.remove(&ephemeral_sid) {
            info.resumed_from = None; // Clear to avoid confusion
            active.entry(original_sid).or_insert(info);
        }
    }

    let mut msg_map: HashMap<String, Vec<(u64, String)>> = HashMap::new();
    let mut meta_map: HashMap<String, (String, Option<String>)> = HashMap::new();

    let reader = io::BufReader::new(file);

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let entry: HistoryEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let sid = match entry.session_id {
            Some(s) => s,
            None => continue,
        };

        let last_cwd = if let Some((cwd, _path)) = resumable.get(&sid) {
            cwd.clone()
        } else if active.contains_key(&sid) {
            None
        } else {
            continue;
        };

        let ts = entry.timestamp.unwrap_or(0);
        let display = entry.display.unwrap_or_default();
        let project = entry.project.unwrap_or_default();

        meta_map
            .entry(sid.clone())
            .or_insert((project, last_cwd));

        msg_map.entry(sid).or_default().push((ts, display));
    }

    let mut sessions = Vec::new();
    for (sid, mut msgs) in msg_map {
        msgs.sort_by_key(|(ts, _)| *ts);
        let (project, last_cwd) = meta_map.remove(&sid).unwrap();
        let first_msg = msgs
            .first()
            .map(|(_, d)| d.chars().take(200).collect())
            .unwrap_or_default();
        let last_msg = msgs
            .last()
            .map(|(_, d)| d.chars().take(200).collect())
            .unwrap_or_default();
        let last_ts = msgs.last().map(|(ts, _)| *ts).unwrap_or(0);
        let msg_count = msgs.len();

        // Read full conversation turns from session file
        let messages = if let Some((_cwd, path)) = resumable.get(&sid) {
            read_session_turns(path)
        } else {
            // Fallback: user messages only from history
            msgs.into_iter()
                .map(|(_, d)| Turn { role: "user".into(), text: d })
                .collect()
        };

        let active_info = active.remove(&sid);

        sessions.push(Session {
            id: sid,
            project,
            last_ts,
            msg_count,
            first_msg,
            last_msg,
            last_cwd,
            active: active_info,
            messages,
        });
    }

    // Add active sessions that had no history.jsonl entries.
    // Only include if they also have a resumable transcript file — otherwise
    // they're phantom entries (e.g. from --resume with ephemeral IDs or broken processes).
    for (sid, info) in active {
        if !resumable.contains_key(&sid) {
            continue;
        }
        let cwd = session_cwd_map.get(&sid).cloned();
        let project = cwd.clone().unwrap_or_default();
        let display_msg = format!("(running in {})", short_project(&project));
        sessions.push(Session {
            id: sid,
            project,
            last_ts: 0,
            msg_count: 0,
            first_msg: String::new(),
            last_msg: display_msg,
            last_cwd: cwd,
            active: Some(info),
            messages: vec![],
        });
    }

    sessions
}

pub fn format_time_ago(ts_ms: u64) -> String {
    if ts_ms == 0 {
        return "active".into();
    }
    let now = chrono::Utc::now().timestamp_millis() as u64;
    if ts_ms > now {
        return "just now".into();
    }
    let diff_secs = (now - ts_ms) / 1000;
    if diff_secs < 60 {
        return "just now".into();
    }
    let mins = diff_secs / 60;
    if mins < 60 {
        return format!("{}m ago", mins);
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{}h ago", hours);
    }
    let days = hours / 24;
    if days < 30 {
        return format!("{}d ago", days);
    }
    let months = days / 30;
    format!("{}mo ago", months)
}

pub fn short_project(path: &str) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let home_str = home.to_string_lossy();
    if path.starts_with(home_str.as_ref()) {
        format!("~{}", &path[home_str.len()..])
    } else {
        path.to_string()
    }
}

/// A directory entry for the new session picker
#[derive(Clone)]
pub struct DirEntry {
    pub path: String,
    pub display: String,
    pub has_claude_md: bool,
    pub has_git: bool,
    pub score: i32,
}

/// Walk ~ up to 3 levels deep, collecting directories for the new session picker
pub fn discover_dirs(recent_dirs: &[String]) -> Vec<DirEntry> {
    use std::collections::HashSet;

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return vec![],
    };

    let recent_set: HashSet<&str> = recent_dirs.iter().map(|s| s.as_str()).collect();

    let skip_names: HashSet<&str> = [
        "target", "node_modules", ".git", ".cache", ".cargo", ".rustup",
        ".local", ".mozilla", ".config", ".ssh", ".gnupg", ".pki",
        "__pycache__", ".venv", "venv", ".npm", ".nvm",
        ".dotfiles", ".claude",
    ].iter().copied().collect();

    let mut entries = Vec::new();

    fn walk(
        dir: &std::path::Path,
        depth: usize,
        max_depth: usize,
        skip_names: &HashSet<&str>,
        entries: &mut Vec<(String, bool, bool)>,
    ) {
        let read_dir = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => return,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if name.starts_with('.') || skip_names.contains(name.as_str()) {
                continue;
            }
            let path_str = path.to_string_lossy().to_string();
            let has_claude_md = path.join("CLAUDE.md").exists();
            let has_git = path.join(".git").exists();
            entries.push((path_str, has_claude_md, has_git));
            if depth < max_depth {
                walk(&path, depth + 1, max_depth, skip_names, entries);
            }
        }
    }

    walk(&home, 1, 3, &skip_names, &mut entries);

    let mut dir_entries: Vec<DirEntry> = entries
        .into_iter()
        .map(|(path, has_claude_md, has_git)| {
            let display = short_project(&path);
            let mut score: i32 = 0;
            if has_claude_md {
                score += 100;
            }
            if recent_set.contains(path.as_str()) {
                score += 50;
            }
            if has_git {
                score += 10;
            }
            DirEntry {
                path,
                display,
                has_claude_md,
                has_git,
                score,
            }
        })
        .collect();

    dir_entries.sort_by(|a, b| b.score.cmp(&a.score).then(a.display.cmp(&b.display)));
    dir_entries
}

/// Compute format_time_ago from a specific "now" timestamp (for testing)
#[cfg(test)]
pub fn format_time_ago_from(ts_ms: u64, now_ms: u64) -> String {
    if ts_ms > now_ms {
        return "just now".into();
    }
    let diff_secs = (now_ms - ts_ms) / 1000;
    if diff_secs < 60 {
        return "just now".into();
    }
    let mins = diff_secs / 60;
    if mins < 60 {
        return format!("{}m ago", mins);
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{}h ago", hours);
    }
    let days = hours / 24;
    if days < 30 {
        return format!("{}d ago", days);
    }
    let months = days / 30;
    format!("{}mo ago", months)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_format_time_ago() {
        let now = 1_700_000_000_000u64; // some fixed "now" in ms

        // Just now (< 60s)
        assert_eq!(format_time_ago_from(now - 30_000, now), "just now");
        assert_eq!(format_time_ago_from(now, now), "just now");

        // Future timestamp
        assert_eq!(format_time_ago_from(now + 5_000, now), "just now");

        // Minutes
        assert_eq!(format_time_ago_from(now - 120_000, now), "2m ago");
        assert_eq!(format_time_ago_from(now - 3_540_000, now), "59m ago");

        // Hours
        assert_eq!(format_time_ago_from(now - 3_600_000, now), "1h ago");
        assert_eq!(format_time_ago_from(now - 7_200_000, now), "2h ago");

        // Days
        assert_eq!(format_time_ago_from(now - 86_400_000, now), "1d ago");
        assert_eq!(format_time_ago_from(now - 86_400_000 * 5, now), "5d ago");

        // Months
        assert_eq!(format_time_ago_from(now - 86_400_000 * 60, now), "2mo ago");
    }

    #[test]
    fn test_short_project() {
        // Non-home path stays as-is
        assert_eq!(short_project("/tmp/foo"), "/tmp/foo");

        // Home path gets ~ replacement
        let home = dirs::home_dir().unwrap();
        let test_path = format!("{}/dev/myproject", home.display());
        let result = short_project(&test_path);
        assert_eq!(result, "~/dev/myproject");
    }

    #[test]
    fn test_history_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let history_path = dir.path().join("history.jsonl");
        let mut f = fs::File::create(&history_path).unwrap();

        // Write some history entries
        writeln!(f, r#"{{"display":"hello world","timestamp":1000000,"project":"/tmp/proj","sessionId":"sess-1"}}"#).unwrap();
        writeln!(f, r#"{{"display":"second msg","timestamp":2000000,"project":"/tmp/proj","sessionId":"sess-1"}}"#).unwrap();
        writeln!(f, r#"{{"display":"other session","timestamp":3000000,"project":"/tmp/other","sessionId":"sess-2"}}"#).unwrap();
        drop(f);

        // Parse the file directly (can't use load_sessions since it depends on ~/.claude)
        let file = fs::File::open(&history_path).unwrap();
        let reader = io::BufReader::new(file);

        let mut msg_map: HashMap<String, Vec<(u64, String)>> = HashMap::new();

        for line in reader.lines() {
            let line = line.unwrap();
            #[derive(Deserialize)]
            struct Entry {
                display: Option<String>,
                timestamp: Option<u64>,
                #[serde(rename = "sessionId")]
                session_id: Option<String>,
            }
            let entry: Entry = serde_json::from_str(&line).unwrap();
            if let Some(sid) = entry.session_id {
                msg_map.entry(sid).or_default().push((
                    entry.timestamp.unwrap_or(0),
                    entry.display.unwrap_or_default(),
                ));
            }
        }

        assert_eq!(msg_map.len(), 2);
        assert_eq!(msg_map["sess-1"].len(), 2);
        assert_eq!(msg_map["sess-2"].len(), 1);
        assert_eq!(msg_map["sess-1"][0].1, "hello world");
        assert_eq!(msg_map["sess-1"][1].1, "second msg");
    }

    #[test]
    fn test_extract_text_content_string() {
        let val = serde_json::json!("hello world");
        assert_eq!(extract_text_content(&val), "hello world");
    }

    #[test]
    fn test_extract_text_content_array() {
        let val = serde_json::json!([
            {"type": "text", "text": "Hello"},
            {"type": "tool_use", "name": "bash"},
            {"type": "text", "text": "World"}
        ]);
        assert_eq!(extract_text_content(&val), "Hello World");
    }

    #[test]
    fn test_extract_text_content_empty() {
        let val = serde_json::json!([{"type": "tool_use", "name": "bash"}]);
        assert_eq!(extract_text_content(&val), "");
    }

    #[test]
    fn test_read_session_turns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-session.jsonl");
        let mut f = fs::File::create(&path).unwrap();

        // Write a minimal conversation: user, assistant, assistant (streaming), user, assistant
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"What is 2+2?"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","content":"The answer"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","content":"The answer is 4."}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"Thanks!"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"You're welcome!"}}]}}}}"#).unwrap();
        drop(f);

        let turns = read_session_turns(&path);
        assert_eq!(turns.len(), 4);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[0].text, "What is 2+2?");
        assert_eq!(turns[1].role, "assistant");
        assert_eq!(turns[1].text, "The answer is 4."); // Kept longest streaming chunk
        assert_eq!(turns[2].role, "user");
        assert_eq!(turns[2].text, "Thanks!");
        assert_eq!(turns[3].role, "assistant");
        assert_eq!(turns[3].text, "You're welcome!");
    }

    #[test]
    fn test_read_session_turns_skips_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-session.jsonl");
        let mut f = fs::File::create(&path).unwrap();

        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"Hi"}}}}"#).unwrap();
        // Assistant with only tool_use, no text
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","name":"bash"}}]}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"Ok"}}}}"#).unwrap();
        drop(f);

        let turns = read_session_turns(&path);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].text, "Hi");
        assert_eq!(turns[1].text, "Ok");
    }

    #[test]
    fn test_turn_deserialize_from_string() {
        // Legacy format: plain strings (from old remote binaries)
        let json = r#"["hello","world"]"#;
        let turns: Vec<Turn> = serde_json::from_str(json).unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[0].text, "hello");
        assert_eq!(turns[1].text, "world");
    }

    #[test]
    fn test_turn_deserialize_from_object() {
        // New format: Turn objects
        let json = r#"[{"role":"user","text":"hi"},{"role":"assistant","text":"hello!"}]"#;
        let turns: Vec<Turn> = serde_json::from_str(json).unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[0].text, "hi");
        assert_eq!(turns[1].role, "assistant");
        assert_eq!(turns[1].text, "hello!");
    }
}
