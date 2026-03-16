mod remote;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, stdout, BufRead},
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
struct SessionPidFile {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
}

#[derive(Deserialize)]
struct HyprClient {
    pid: i64,
    workspace: HyprWorkspace,
    address: String,
}

#[derive(Deserialize)]
struct HyprWorkspace {
    id: i64,
    name: String,
}

/// Info about an active session's window
struct ActiveInfo {
    pid: u32,
    workspace: Option<String>,
    window_address: Option<String>,
}

struct Session {
    id: String,
    project: String,
    last_ts: u64,
    msg_count: usize,
    first_msg: String,
    last_msg: String,
    last_cwd: Option<String>,
    active: Option<ActiveInfo>,
    messages: Vec<String>,
}

struct Project {
    path: String,
    session_count: usize,
    last_ts: u64,
}

#[derive(PartialEq)]
enum View {
    Folders,
    FolderSessions,
    AllSessions,
    RemoteHosts,
    RemoteSessions,
}

struct App {
    sessions: Vec<Session>,
    projects: Vec<Project>,
    view: View,
    folder_state: ListState,
    folder_filtered: Vec<usize>,
    session_state: ListState,
    session_filtered: Vec<usize>,
    selected_project: Option<String>,
    filter: String,
    filtering: bool,
    expand_lines: usize,
    // Remote
    remote_hosts: Vec<remote::HostConfig>,
    remote_host_state: ListState,
    remote_sessions: Vec<remote::RemoteSession>,
    remote_session_state: ListState,
    remote_selected_host: Option<String>,  // ssh host name
    remote_selected_host_name: Option<String>,  // display name
    remote_error: Option<String>,
    remote_loading: bool,
}

impl App {
    fn new(mut sessions: Vec<Session>) -> Self {
        sessions.sort_by(|a, b| {
            // Active sessions first, then by last_ts
            let a_active = a.active.is_some();
            let b_active = b.active.is_some();
            b_active.cmp(&a_active).then(b.last_ts.cmp(&a.last_ts))
        });

        let mut proj_map: HashMap<String, (usize, u64)> = HashMap::new();
        for s in &sessions {
            let entry = proj_map.entry(s.project.clone()).or_insert((0, 0));
            entry.0 += 1;
            entry.1 = entry.1.max(s.last_ts);
        }
        let mut projects: Vec<Project> = proj_map
            .into_iter()
            .map(|(path, (count, ts))| Project {
                path,
                session_count: count,
                last_ts: ts,
            })
            .collect();
        projects.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));

        let folder_filtered: Vec<usize> = (0..projects.len()).collect();
        let mut folder_state = ListState::default();
        if !folder_filtered.is_empty() {
            folder_state.select(Some(0));
        }

        let session_filtered: Vec<usize> = (0..sessions.len()).collect();
        let mut session_state = ListState::default();
        if !session_filtered.is_empty() {
            session_state.select(Some(0));
        }

        let remote_hosts = remote::load_hosts();
        let mut remote_host_state = ListState::default();
        if !remote_hosts.is_empty() {
            remote_host_state.select(Some(0));
        }

        App {
            sessions,
            projects,
            view: View::AllSessions,
            folder_state,
            folder_filtered,
            session_state,
            session_filtered,
            selected_project: None,
            filter: String::new(),
            filtering: false,
            expand_lines: 0,
            remote_hosts,
            remote_host_state,
            remote_sessions: Vec::new(),
            remote_session_state: ListState::default(),
            remote_selected_host: None,
            remote_selected_host_name: None,
            remote_error: None,
            remote_loading: false,
        }
    }

    fn enter_folder(&mut self) {
        let idx = match self.folder_state.selected() {
            Some(i) => match self.folder_filtered.get(i) {
                Some(&i) => i,
                None => return,
            },
            None => return,
        };
        let project_path = self.projects[idx].path.clone();
        self.selected_project = Some(project_path.clone());
        self.session_filtered = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.project == project_path)
            .map(|(i, _)| i)
            .collect();
        self.session_state = ListState::default();
        if !self.session_filtered.is_empty() {
            self.session_state.select(Some(0));
        }
        self.view = View::FolderSessions;
        self.filter.clear();
        self.filtering = false;
        self.expand_lines = 0;
    }

    fn enter_all_sessions(&mut self) {
        self.session_filtered = (0..self.sessions.len()).collect();
        self.session_state = ListState::default();
        if !self.session_filtered.is_empty() {
            self.session_state.select(Some(0));
        }
        self.selected_project = None;
        self.view = View::AllSessions;
        self.filter.clear();
        self.filtering = false;
        self.expand_lines = 0;
    }

    fn back_to_folders(&mut self) {
        self.view = View::Folders;
        self.filter.clear();
        self.filtering = false;
        self.expand_lines = 0;
        self.apply_filter();
    }

    fn apply_filter(&mut self) {
        let q = self.filter.to_lowercase();
        match self.view {
            View::Folders => {
                self.folder_filtered = self
                    .projects
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| q.is_empty() || p.path.to_lowercase().contains(&q))
                    .map(|(i, _)| i)
                    .collect();
                if self.folder_filtered.is_empty() {
                    self.folder_state.select(None);
                } else {
                    self.folder_state.select(Some(0));
                }
            }
            View::FolderSessions => {
                let proj = self.selected_project.clone().unwrap_or_default();
                self.session_filtered = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        s.project == proj
                            && (q.is_empty()
                                || s.last_msg.to_lowercase().contains(&q)
                                || s.first_msg.to_lowercase().contains(&q)
                                || s.id.starts_with(&q))
                    })
                    .map(|(i, _)| i)
                    .collect();
                if self.session_filtered.is_empty() {
                    self.session_state.select(None);
                } else {
                    self.session_state.select(Some(0));
                }
            }
            View::AllSessions => {
                self.session_filtered = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        q.is_empty()
                            || s.project.to_lowercase().contains(&q)
                            || s.last_msg.to_lowercase().contains(&q)
                            || s.first_msg.to_lowercase().contains(&q)
                            || s.id.starts_with(&q)
                    })
                    .map(|(i, _)| i)
                    .collect();
                if self.session_filtered.is_empty() {
                    self.session_state.select(None);
                } else {
                    self.session_state.select(Some(0));
                }
            }
            View::RemoteHosts | View::RemoteSessions => {
                // No filtering for remote views currently
            }
        }
    }

    fn selected_session(&self) -> Option<&Session> {
        self.session_state
            .selected()
            .and_then(|i| self.session_filtered.get(i))
            .map(|&idx| &self.sessions[idx])
    }

    fn current_list_state_mut(&mut self) -> &mut ListState {
        match self.view {
            View::Folders => &mut self.folder_state,
            View::FolderSessions | View::AllSessions => &mut self.session_state,
            View::RemoteHosts => &mut self.remote_host_state,
            View::RemoteSessions => &mut self.remote_session_state,
        }
    }

    fn current_list_len(&self) -> usize {
        match self.view {
            View::Folders => self.folder_filtered.len(),
            View::FolderSessions | View::AllSessions => self.session_filtered.len(),
            View::RemoteHosts => self.remote_hosts.len(),
            View::RemoteSessions => self.remote_sessions.len(),
        }
    }

    fn enter_remote_hosts(&mut self) {
        self.view = View::RemoteHosts;
        self.filter.clear();
        self.filtering = false;
        self.expand_lines = 0;
        self.remote_error = None;
        if !self.remote_hosts.is_empty() && self.remote_host_state.selected().is_none() {
            self.remote_host_state.select(Some(0));
        }
    }

    fn enter_remote_host(&mut self) {
        let idx = match self.remote_host_state.selected() {
            Some(i) => i,
            None => return,
        };
        let host = match self.remote_hosts.get(idx) {
            Some(h) => h.clone(),
            None => return,
        };

        self.remote_loading = true;
        self.remote_selected_host = Some(host.ssh.clone());
        self.remote_selected_host_name = Some(host.name.clone());

        match remote::fetch_remote_sessions(&host) {
            Ok(mut sessions) => {
                sessions.sort_by(|a, b| {
                    let a_active = a.active_pid.is_some();
                    let b_active = b.active_pid.is_some();
                    b_active.cmp(&a_active).then(b.last_ts.cmp(&a.last_ts))
                });
                self.remote_sessions = sessions;
                self.remote_error = None;
                self.remote_session_state = ListState::default();
                if !self.remote_sessions.is_empty() {
                    self.remote_session_state.select(Some(0));
                }
                self.view = View::RemoteSessions;
            }
            Err(e) => {
                self.remote_error = Some(e);
            }
        }
        self.remote_loading = false;
        self.expand_lines = 0;
    }

    fn selected_remote_session(&self) -> Option<&remote::RemoteSession> {
        self.remote_session_state
            .selected()
            .and_then(|i| self.remote_sessions.get(i))
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.current_list_len() as i32;
        if len == 0 {
            return;
        }
        let state = self.current_list_state_mut();
        let current = state.selected().unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, len - 1) as usize;
        state.select(Some(next));
        self.expand_lines = 0;
    }

    fn jump_top(&mut self) {
        if self.current_list_len() > 0 {
            self.current_list_state_mut().select(Some(0));
            self.expand_lines = 0;
        }
    }

    fn jump_bottom(&mut self) {
        let len = self.current_list_len();
        if len > 0 {
            self.current_list_state_mut().select(Some(len - 1));
            self.expand_lines = 0;
        }
    }
}

#[derive(Deserialize)]
struct SessionEntry {
    cwd: Option<String>,
}

/// Walk up the process tree from a PID to find the terminal (foot) PID.
fn find_terminal_pid(mut pid: u32) -> Option<u32> {
    for _ in 0..20 {
        // safety limit
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
        // Get PPID (field after the closing paren)
        let rest = &stat[comm_end + 2..];
        let ppid: u32 = rest.split_whitespace().nth(1)?.parse().ok()?;
        pid = ppid;
    }
    None
}

/// Get hyprland window info for active sessions.
/// Returns map of session_id -> ActiveInfo with workspace and window address.
fn find_active_sessions() -> HashMap<String, ActiveInfo> {
    let sessions_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude/sessions");

    // First, collect active session PIDs
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
            let pid_file: SessionPidFile = match serde_json::from_str(&content) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let is_running = unsafe { libc::kill(pid_file.pid as i32, 0) == 0 };
            if is_running {
                session_pids.push((pid_file.session_id, pid_file.pid));
            }
        }
    }

    if session_pids.is_empty() {
        return HashMap::new();
    }

    // Get hyprland clients
    let hypr_clients: Vec<HyprClient> = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or_default();

    // For each active session, walk up to foot terminal and find it in hyprland
    let mut active = HashMap::new();
    for (sid, claude_pid) in session_pids {
        let terminal_pid = find_terminal_pid(claude_pid);
        let mut info = ActiveInfo {
            pid: claude_pid,
            workspace: None,
            window_address: None,
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

fn find_resumable_sessions() -> HashMap<String, Option<String>> {
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
                    resumable.insert(stem.to_string(), last_cwd);
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

fn load_sessions() -> Vec<Session> {
    let history_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude/history.jsonl");

    let file = match fs::File::open(&history_path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let resumable = find_resumable_sessions();
    let mut active = find_active_sessions();

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

        // Must be resumable on disk OR currently active
        let last_cwd = if let Some(cwd) = resumable.get(&sid) {
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
        let messages: Vec<String> = msgs.into_iter().map(|(_, d)| d).collect();

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

    // Add active sessions that had no history.jsonl entries
    // (e.g. just started, no user messages yet)
    for (sid, info) in active {
        // Use the cwd from the session PID file as project/cwd
        let cwd = read_session_cwd(&sid);
        let project = cwd.clone().unwrap_or_default();
        sessions.push(Session {
            id: sid,
            project,
            last_ts: 0,
            msg_count: 0,
            first_msg: String::new(),
            last_msg: "(no messages yet)".into(),
            last_cwd: cwd,
            active: Some(info),
            messages: vec![],
        });
    }

    sessions
}

/// Read the cwd from a session's PID file in ~/.claude/sessions/
fn read_session_cwd(session_id: &str) -> Option<String> {
    let sessions_dir = dirs::home_dir()?.join(".claude/sessions");
    if let Ok(entries) = fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let content = fs::read_to_string(entry.path()).ok()?;
            if let Ok(pid_file) = serde_json::from_str::<SessionPidFile>(&content) {
                if pid_file.session_id == session_id {
                    // SessionPidFile has cwd but we didn't deserialize it - let's grab it raw
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        return val.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                    }
                }
            }
        }
    }
    None
}

fn format_time_ago(ts_ms: u64) -> String {
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

fn short_project(path: &str) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let home_str = home.to_string_lossy();
    if path.starts_with(home_str.as_ref()) {
        format!("~{}", &path[home_str.len()..])
    } else {
        path.to_string()
    }
}

/// Focus an active session's terminal window via hyprctl
fn focus_active_session(session: &Session) {
    if let Some(ref info) = session.active {
        if let Some(ref addr) = info.window_address {
            // Switch workspace and focus window
            let _ = Command::new("hyprctl")
                .args(["dispatch", "focuswindow", &format!("address:{}", addr)])
                .output();
        }
    }
}

fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(f.area());

    let view_label = match app.view {
        View::Folders => format!("  {} projects", app.folder_filtered.len()),
        View::FolderSessions => {
            let proj = app
                .selected_project
                .as_deref()
                .map(short_project)
                .unwrap_or_default();
            format!("  {} > {} sessions", proj, app.session_filtered.len())
        }
        View::AllSessions => format!("  {} sessions (all)", app.session_filtered.len()),
        View::RemoteHosts => format!("  {} hosts", app.remote_hosts.len()),
        View::RemoteSessions => {
            let host_name = app.remote_selected_host_name.as_deref().unwrap_or("?");
            format!("  {} > {} sessions", host_name, app.remote_sessions.len())
        }
    };

    let make_tab = |label: &str, active: bool| -> Span {
        if active {
            Span::styled(
                format!(" [{}] ", label),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!(" {} ", label), Style::default().fg(Color::DarkGray))
        }
    };

    let tab_all = make_tab("All", app.view == View::AllSessions);
    let tab_folders = make_tab("Folders", app.view == View::Folders || app.view == View::FolderSessions);
    let tab_remote = make_tab("Remote", app.view == View::RemoteHosts || app.view == View::RemoteSessions);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " Claude Resume",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        tab_all,
        tab_folders,
        tab_remote,
        Span::raw(view_label),
    ]));
    f.render_widget(header, chunks[0]);

    match app.view {
        View::Folders => draw_folders(f, app, chunks[1]),
        View::FolderSessions | View::AllSessions => draw_sessions(f, app, chunks[1]),
        View::RemoteHosts => draw_remote_hosts(f, app, chunks[1]),
        View::RemoteSessions => draw_remote_sessions(f, app, chunks[1]),
    }

    let footer_text = if app.filtering {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.filter),
            Span::styled("█", Style::default().fg(Color::Yellow)),
        ])
    } else {
        let mut hints = vec![
            Span::styled(" enter", Style::default().fg(Color::Green)),
            Span::raw(match app.view {
                View::Folders | View::RemoteHosts => " open  ",
                _ => " resume/focus  ",
            }),
        ];
        if app.view == View::FolderSessions || app.view == View::RemoteSessions {
            hints.push(Span::styled("esc", Style::default().fg(Color::Green)));
            hints.push(Span::raw(" back  "));
        }
        if matches!(app.view, View::FolderSessions | View::AllSessions | View::RemoteSessions) {
            hints.push(Span::styled("l/h", Style::default().fg(Color::Green)));
            hints.push(Span::raw(" expand  "));
        }
        hints.push(Span::styled("tab", Style::default().fg(Color::Green)));
        hints.push(Span::raw(" view  "));
        if !matches!(app.view, View::RemoteHosts | View::RemoteSessions) {
            hints.push(Span::styled("/", Style::default().fg(Color::Green)));
            hints.push(Span::raw(" filter  "));
        }
        hints.push(Span::styled("q", Style::default().fg(Color::Green)));
        hints.push(Span::raw(" quit"));
        Line::from(hints)
    };
    let footer = Paragraph::new(footer_text);
    f.render_widget(footer, chunks[2]);
}

fn draw_folders(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
        .folder_filtered
        .iter()
        .map(|&idx| {
            let p = &app.projects[idx];
            let time_ago = format_time_ago(p.last_ts);
            let path = short_project(&p.path);

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>8} ", time_ago),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled("  ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{:<40} ", path),
                    Style::default().fg(Color::Blue),
                ),
                Span::styled(
                    format!("{} sessions", p.session_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().style(Style::default().bg(Color::Rgb(30, 30, 40))))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(50, 50, 70))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    f.render_stateful_widget(list, area, &mut app.folder_state.clone());
}

/// Wrap a long string into multiple lines of a given width, with a left indent.
fn wrap_text(text: &str, width: usize, indent: usize, style: Style) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let prefix: String = " ".repeat(indent);
    let usable = width.saturating_sub(indent);
    if usable == 0 {
        return lines;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut pos = 0;
    while pos < chars.len() {
        let end = (pos + usable).min(chars.len());
        let chunk: String = chars[pos..end].iter().collect();
        let mut spans = vec![Span::raw(prefix.clone())];
        spans.push(Span::styled(chunk, style));
        lines.push(Line::from(spans));
        pos = end;
    }
    lines
}

fn draw_sessions(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let selected_idx = app.session_state.selected();
    let w = area.width as usize;

    let items: Vec<ListItem> = app
        .session_filtered
        .iter()
        .enumerate()
        .map(|(list_pos, &idx)| {
            let s = &app.sessions[idx];
            let time_ago = format_time_ago(s.last_ts);

            let cwd_display = s
                .last_cwd
                .as_deref()
                .map(short_project)
                .unwrap_or_else(|| short_project(&s.project));

            // Active marker
            let active_marker = if let Some(ref info) = s.active {
                let ws = info.workspace.as_deref().unwrap_or("?");
                Span::styled(
                    format!(" ●{}", ws),
                    Style::default().fg(Color::Green),
                )
            } else {
                Span::styled("  ", Style::default())
            };

            // Line 1: marker  time  dir  msg_count
            let line1 = Line::from(vec![
                active_marker,
                Span::styled(
                    format!(" {:>8}", time_ago),
                    Style::default().fg(Color::Rgb(100, 100, 120)),
                ),
                Span::raw("  "),
                Span::styled(
                    cwd_display.clone(),
                    Style::default().fg(Color::Rgb(100, 160, 220)),
                ),
                Span::styled(
                    format!("  {}m", s.msg_count),
                    Style::default().fg(Color::Rgb(80, 80, 100)),
                ),
            ]);

            // Line 2: last message (full width)
            let msg_width = w.saturating_sub(5);
            let preview: String = s.last_msg.chars().take(msg_width).collect();
            let line2 = Line::from(vec![
                Span::raw("     "),
                Span::styled(preview, Style::default().fg(Color::Rgb(180, 180, 190))),
            ]);

            let is_selected = selected_idx == Some(list_pos);
            if is_selected && app.expand_lines > 0 {
                let mut lines = vec![line1, line2];

                let msg_count = s.messages.len();
                let skip = if msg_count > 1 { 1 } else { 0 };
                let available = msg_count.saturating_sub(skip);
                let show = app.expand_lines.min(available);
                let start = msg_count.saturating_sub(skip + show);
                let end = msg_count.saturating_sub(skip);

                for i in (start..end).rev() {
                    let msg = &s.messages[i];
                    let label = format!("  [{}] ", i + 1);
                    let label_len = label.len();
                    // Wrap message text
                    let msg_lines = wrap_text(
                        msg,
                        w.saturating_sub(2),
                        label_len,
                        Style::default().fg(Color::Rgb(150, 150, 165)),
                    );
                    // First line gets the label
                    if let Some(first) = msg_lines.first() {
                        let mut first_spans = vec![
                            Span::styled(
                                label.clone(),
                                Style::default().fg(Color::Rgb(70, 70, 90)),
                            ),
                        ];
                        // Take the text portion (skip the indent)
                        let text_part: String = msg.chars().take(w.saturating_sub(label_len + 2)).collect();
                        first_spans.push(Span::styled(
                            text_part,
                            Style::default().fg(Color::Rgb(150, 150, 165)),
                        ));
                        lines.push(Line::from(first_spans));
                        // Remaining wrapped lines
                        for wrap_line in msg_lines.iter().skip(1) {
                            lines.push(wrap_line.clone());
                        }
                    }
                }

                // Blank separator line
                lines.push(Line::raw(""));

                ListItem::new(lines)
            } else {
                // Blank line between items for spacing
                ListItem::new(vec![line1, line2, Line::raw("")])
            }
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().style(Style::default().bg(Color::Rgb(30, 30, 40))))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(50, 50, 70)),
        )
        .highlight_symbol("▸ ");

    f.render_stateful_widget(list, area, &mut app.session_state.clone());
}

fn draw_remote_hosts(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut items: Vec<ListItem> = app
        .remote_hosts
        .iter()
        .map(|h| {
            ListItem::new(Line::from(vec![
                Span::styled("  ", Style::default().fg(Color::Magenta)),
                Span::styled(
                    format!("{:<20} ", h.name),
                    Style::default().fg(Color::Magenta),
                ),
                Span::styled(
                    format!("(ssh {})", h.ssh),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    if let Some(ref err) = app.remote_error {
        items.push(ListItem::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("Error: {}", err),
                Style::default().fg(Color::Red),
            ),
        ])));
    }

    let list = List::new(items)
        .block(Block::default().style(Style::default().bg(Color::Rgb(30, 30, 40))))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(50, 50, 70))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    f.render_stateful_widget(list, area, &mut app.remote_host_state.clone());
}

fn draw_remote_sessions(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.remote_sessions.is_empty() {
        let msg = Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("No sessions found", Style::default().fg(Color::DarkGray)),
        ]))
        .block(Block::default().style(Style::default().bg(Color::Rgb(30, 30, 40))));
        f.render_widget(msg, area);
        return;
    }

    let selected_idx = app.remote_session_state.selected();
    let w = area.width as usize;

    let items: Vec<ListItem> = app
        .remote_sessions
        .iter()
        .enumerate()
        .map(|(list_pos, s)| {
            let time_ago = format_time_ago(s.last_ts);

            let cwd_display = s
                .last_cwd
                .as_deref()
                .unwrap_or(&s.project);

            let active_marker = if let Some(pid) = s.active_pid {
                Span::styled(
                    format!(" ●{}", pid),
                    Style::default().fg(Color::Green),
                )
            } else {
                Span::styled("  ", Style::default())
            };

            let line1 = Line::from(vec![
                active_marker,
                Span::styled(
                    format!(" {:>8}", time_ago),
                    Style::default().fg(Color::Rgb(100, 100, 120)),
                ),
                Span::raw("  "),
                Span::styled(
                    cwd_display.to_string(),
                    Style::default().fg(Color::Rgb(100, 160, 220)),
                ),
                Span::styled(
                    format!("  {}m", s.msg_count),
                    Style::default().fg(Color::Rgb(80, 80, 100)),
                ),
            ]);

            let msg_width = w.saturating_sub(5);
            let preview: String = s.last_msg.chars().take(msg_width).collect();
            let line2 = Line::from(vec![
                Span::raw("     "),
                Span::styled(preview, Style::default().fg(Color::Rgb(180, 180, 190))),
            ]);

            let is_selected = selected_idx == Some(list_pos);
            if is_selected && app.expand_lines > 0 {
                let mut lines = vec![line1, line2];

                let msg_count = s.messages.len();
                let skip = if msg_count > 1 { 1 } else { 0 };
                let available = msg_count.saturating_sub(skip);
                let show = app.expand_lines.min(available);
                let start = msg_count.saturating_sub(skip + show);
                let end = msg_count.saturating_sub(skip);

                for i in (start..end).rev() {
                    let msg = &s.messages[i];
                    let label = format!("  [{}] ", i + 1);
                    let label_len = label.len();
                    let msg_lines = wrap_text(
                        msg,
                        w.saturating_sub(2),
                        label_len,
                        Style::default().fg(Color::Rgb(150, 150, 165)),
                    );
                    if let Some(_first) = msg_lines.first() {
                        let mut first_spans = vec![
                            Span::styled(
                                label.clone(),
                                Style::default().fg(Color::Rgb(70, 70, 90)),
                            ),
                        ];
                        let text_part: String = msg.chars().take(w.saturating_sub(label_len + 2)).collect();
                        first_spans.push(Span::styled(
                            text_part,
                            Style::default().fg(Color::Rgb(150, 150, 165)),
                        ));
                        lines.push(Line::from(first_spans));
                        for wrap_line in msg_lines.iter().skip(1) {
                            lines.push(wrap_line.clone());
                        }
                    }
                }

                lines.push(Line::raw(""));
                ListItem::new(lines)
            } else {
                ListItem::new(vec![line1, line2, Line::raw("")])
            }
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().style(Style::default().bg(Color::Rgb(30, 30, 40))))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(50, 50, 70)),
        )
        .highlight_symbol("▸ ");

    f.render_stateful_widget(list, area, &mut app.remote_session_state.clone());
}

#[derive(Serialize)]
struct JsonSession {
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

#[derive(Serialize)]
struct JsonOutput {
    sessions: Vec<JsonSession>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --json flag: dump session data as JSON for remote consumption
    if std::env::args().any(|a| a == "--json") {
        let sessions = load_sessions();
        let json_sessions: Vec<JsonSession> = sessions
            .into_iter()
            .map(|s| JsonSession {
                id: s.id,
                project: s.project,
                last_ts: s.last_ts,
                msg_count: s.msg_count,
                first_msg: s.first_msg,
                last_msg: s.last_msg,
                last_cwd: s.last_cwd,
                active_pid: s.active.map(|a| a.pid),
                messages: s.messages,
            })
            .collect();
        let output = JsonOutput { sessions: json_sessions };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

    let sessions = load_sessions();
    if sessions.is_empty() {
        eprintln!("No Claude sessions found in ~/.claude/history.jsonl");
        return Ok(());
    }

    let mut app = App::new(sessions);

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| draw(f, &app))?;

        if let Event::Key(key) = event::read()? {
            if app.filtering {
                match key.code {
                    KeyCode::Esc => {
                        app.filtering = false;
                        app.filter.clear();
                        app.apply_filter();
                    }
                    KeyCode::Enter => {
                        app.filtering = false;
                    }
                    KeyCode::Backspace => {
                        app.filter.pop();
                        app.apply_filter();
                    }
                    KeyCode::Char(c) => {
                        app.filter.push(c);
                        app.apply_filter();
                    }
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
                KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
                KeyCode::Char('l') | KeyCode::Right => {
                    if app.view == View::RemoteSessions {
                        if let Some(session) = app.selected_remote_session() {
                            let max = session.messages.len().saturating_sub(1);
                            if app.expand_lines < max {
                                app.expand_lines += 1;
                            }
                        }
                    } else if app.view != View::Folders && app.view != View::RemoteHosts {
                        if let Some(session) = app.selected_session() {
                            let max = session.messages.len().saturating_sub(1);
                            if app.expand_lines < max {
                                app.expand_lines += 1;
                            }
                        }
                    }
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    if app.expand_lines > 0 {
                        app.expand_lines = app.expand_lines.saturating_sub(1);
                    }
                }
                KeyCode::Char('G') => app.jump_bottom(),
                KeyCode::Char('g') => app.jump_top(),
                KeyCode::Char('/') => {
                    app.filtering = true;
                }
                KeyCode::PageDown => app.move_selection(20),
                KeyCode::PageUp => app.move_selection(-20),
                KeyCode::Tab => {
                    match app.view {
                        View::AllSessions => {
                            app.view = View::Folders;
                            app.filter.clear();
                            app.filtering = false;
                            app.expand_lines = 0;
                            app.folder_filtered = (0..app.projects.len()).collect();
                            if !app.folder_filtered.is_empty()
                                && app.folder_state.selected().is_none()
                            {
                                app.folder_state.select(Some(0));
                            }
                        }
                        View::Folders | View::FolderSessions => {
                            app.enter_remote_hosts();
                        }
                        View::RemoteHosts | View::RemoteSessions => {
                            app.enter_all_sessions();
                        }
                    }
                }
                KeyCode::Esc => {
                    if app.expand_lines > 0 {
                        app.expand_lines = 0;
                    } else {
                        match app.view {
                            View::FolderSessions => app.back_to_folders(),
                            View::RemoteSessions => app.enter_remote_hosts(),
                            View::Folders | View::AllSessions | View::RemoteHosts => break,
                        }
                    }
                }
                KeyCode::Enter => match app.view {
                    View::Folders => app.enter_folder(),
                    View::RemoteHosts => app.enter_remote_host(),
                    View::RemoteSessions => {
                        if let Some(session) = app.selected_remote_session() {
                            let ssh_host = app.remote_selected_host.clone().unwrap_or_default();

                            if let Some(pid) = session.active_pid {
                                // Kill the active process first
                                let _ = remote::kill_remote_pid(&ssh_host, pid);
                                // Small delay to let it die
                                std::thread::sleep(std::time::Duration::from_millis(500));
                            }

                            remote::open_remote_session(&ssh_host, session);

                            disable_raw_mode()?;
                            stdout().execute(LeaveAlternateScreen)?;
                            std::process::exit(0);
                        }
                    }
                    View::FolderSessions | View::AllSessions => {
                        if let Some(session) = app.selected_session() {
                            if session.active.is_some() {
                                // Focus the active session's terminal window
                                focus_active_session(session);
                                break;
                            }

                            let sid = session.id.clone();
                            let cwd = session
                                .last_cwd
                                .clone()
                                .unwrap_or_else(|| session.project.clone());

                            disable_raw_mode()?;
                            stdout().execute(LeaveAlternateScreen)?;

                            let status = Command::new("claude")
                                .arg("--dangerously-skip-permissions")
                                .arg("--resume")
                                .arg(&sid)
                                .current_dir(&cwd)
                                .status();

                            match status {
                                Ok(s) => std::process::exit(s.code().unwrap_or(0)),
                                Err(e) => {
                                    eprintln!("Failed to launch claude: {}", e);
                                    std::process::exit(1);
                                }
                            }
                        }
                    }
                },
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
