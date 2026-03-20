mod app;
mod config;
mod remote;
mod session;
mod ui;
mod wm;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers, DisableMouseCapture},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::Terminal;
use serde::Serialize;
use std::{io::stdout, process::Command};
use std::os::unix::process::CommandExt;
extern crate libc;

use app::View;
use session::Session;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    in_tmux: Option<bool>,
    messages: Vec<session::Turn>,
}

#[derive(Serialize)]
struct JsonOutput {
    sessions: Vec<JsonSession>,
}

/// Focus an active session's terminal window via the detected window manager.
fn focus_active_session(session: &Session) {
    if let Some(ref info) = session.active {
        if let Some(ref id) = info.window_address {
            let detected_wm = wm::detect();
            wm::focus_window(detected_wm, id);
        }
    }
}

/// Attach to a tmux session, detaching any other clients first.
/// Uses exec to replace this process so tmux becomes the foreground process.
fn tmux_attach_detach(name: &str) -> ! {
    let err = Command::new("tmux")
        .args(["attach-session", "-d", "-t", name])
        .exec();
    eprintln!("Failed to exec tmux attach: {}", err);
    std::process::exit(1);
}

/// Resume a local session inside tmux. Tries to attach to existing tmux session,
/// or creates a new one with --resume. Uses exec to replace this process.
fn tmux_resume_local(session_id: &str, project: &str) -> ! {
    let tmux_name = session::tmux_session_name(session_id);

    // Check if tmux session exists already
    let has_session = Command::new("tmux")
        .args(["has-session", "-t", &tmux_name])
        .status()
        .map_or(false, |s| s.success());

    if has_session {
        // Attach to existing session (exec replaces this process)
        let err = Command::new("tmux")
            .args(["attach-session", "-t", &tmux_name])
            .exec();
        eprintln!("Failed to exec tmux attach: {}", err);
        std::process::exit(1);
    }

    // Create new tmux session with claude --resume (exec replaces this process)
    let cmd = format!(
        "claude --dangerously-skip-permissions --resume {}",
        session_id
    );
    let err = Command::new("tmux")
        .args(["new-session", "-s", &tmux_name, "-c", project, &cmd])
        .exec();
    eprintln!("Failed to exec tmux new-session: {}", err);
    std::process::exit(1);
}

/// Launch a new local session inside tmux. Uses exec to replace this process.
fn tmux_new_local(dir: &str) -> ! {
    let err = Command::new("tmux")
        .args([
            "new-session",
            "-c",
            dir,
            "claude --dangerously-skip-permissions",
        ])
        .exec();
    eprintln!("Failed to exec tmux new-session: {}", err);
    std::process::exit(1);
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = stdout().execute(LeaveAlternateScreen);
    let _ = stdout().execute(DisableMouseCapture);
    let _ = stdout().execute(cursor::Show);
    // Reset terminal modes via ANSI escapes
    let _ = stdout().execute(crossterm::style::ResetColor);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --list-dirs flag: dump directory listing as JSON for remote consumption
    if std::env::args().any(|a| a == "--list-dirs") {
        let recent = config::load_recent_dirs();
        let dirs = session::discover_dirs(&recent);
        #[derive(Serialize)]
        struct DirJson {
            path: String,
            has_claude_md: bool,
            has_git: bool,
        }
        #[derive(Serialize)]
        struct DirsOutput {
            dirs: Vec<DirJson>,
        }
        let output = DirsOutput {
            dirs: dirs
                .into_iter()
                .map(|d| DirJson {
                    path: d.path,
                    has_claude_md: d.has_claude_md,
                    has_git: d.has_git,
                })
                .collect(),
        };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

    // --json-messages <session-id>: dump turns for a single session (on-demand loading)
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(pos) = args.iter().position(|a| a == "--json-messages") {
            if let Some(session_id) = args.get(pos + 1) {
                let turns = session::load_session_turns(session_id);
                println!("{}", serde_json::to_string(&turns)?);
                return Ok(());
            } else {
                eprintln!("--json-messages requires a session ID argument");
                std::process::exit(1);
            }
        }
    }

    // --json flag: dump session data as JSON for remote consumption (lightweight — no messages)
    if std::env::args().any(|a| a == "--json") {
        let sessions = session::load_sessions_lightweight();
        let json_sessions: Vec<JsonSession> = sessions
            .into_iter()
            .map(|s| {
                let (active_pid, in_tmux) = match s.active {
                    Some(a) => (Some(a.pid), Some(a.in_tmux)),
                    None => (None, None),
                };
                JsonSession {
                    id: s.id,
                    project: s.project,
                    last_ts: s.last_ts,
                    msg_count: s.msg_count,
                    first_msg: s.first_msg,
                    last_msg: s.last_msg,
                    last_cwd: s.last_cwd,
                    active_pid,
                    in_tmux,
                    messages: vec![],
                }
            })
            .collect();
        let output = JsonOutput {
            sessions: json_sessions,
        };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

    let sessions = session::load_sessions();
    if sessions.is_empty() {
        eprintln!("No Claude sessions found in ~/.claude/history.jsonl");
        return Ok(());
    }

    let mut app = app::App::new(sessions);

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut last_refresh = std::time::Instant::now();
    let refresh_interval = std::time::Duration::from_secs(5);

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        // Poll with timeout for periodic refresh
        if !event::poll(std::time::Duration::from_secs(1))? {
            // No input — check if it's time to refresh
            if last_refresh.elapsed() >= refresh_interval {
                app.refresh();
                last_refresh = std::time::Instant::now();
            }
            continue;
        }

        let ev = event::read()?;
        let key = match ev {
            Event::Key(k) => k,
            _ => continue,
        };

        {
            // Kill confirmation mode — y to confirm, anything else cancels
            if app.confirm_kill {
                app.confirm_kill = false;
                if key.code == KeyCode::Char('y') {
                    // Get the session to kill based on current view
                    let local_kill_info: Option<(u32, bool, Option<String>)> = match app.view {
                        View::Folders => {
                            app.selected_folder_preview_session().and_then(|s| {
                                s.active.as_ref().map(|info| {
                                    (info.pid, info.in_tmux, info.tmux_session.clone())
                                })
                            })
                        }
                        View::FolderSessions | View::AllSessions => {
                            app.selected_session().and_then(|s| {
                                s.active.as_ref().map(|info| {
                                    (info.pid, info.in_tmux, info.tmux_session.clone())
                                })
                            })
                        }
                        _ => None,
                    };

                    let killed = if let Some((pid, in_tmux, tmux_sess)) = local_kill_info {
                        if in_tmux {
                            if let Some(ts) = tmux_sess {
                                let _ = Command::new("tmux")
                                    .args(["kill-session", "-t", &ts])
                                    .status();
                            }
                        }
                        unsafe { libc::kill(pid as i32, libc::SIGTERM); }
                        true
                    } else if app.view == View::RemoteSessions {
                        if let Some(session) = app.selected_remote_session() {
                            if let Some(pid) = session.active_pid {
                                let ssh_host = app.remote_selected_host.clone().unwrap_or_default();
                                let session_id = session.id.clone();
                                let in_tmux = session.in_tmux;
                                if in_tmux {
                                    let _ = remote::kill_remote_tmux(&ssh_host, app.remote_selected_port, &session_id);
                                }
                                let _ = remote::kill_remote_pid(&ssh_host, app.remote_selected_port, pid);
                                true
                            } else { false }
                        } else { false }
                    } else {
                        false
                    };
                    if killed {
                        // Give the process a moment to die, then refresh
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        app.refresh();
                        last_refresh = std::time::Instant::now();
                        app.status_msg = Some("Killed.".into());
                    }
                } else {
                    app.status_msg = None;
                }
                continue;
            }

            // NewSession / NewRemoteSession views handle ALL input themselves
            if matches!(app.view, View::NewSession | View::NewRemoteSession) {
                match key.code {
                    KeyCode::Esc => {
                        // Go back to previous view
                        if let Some(prev) = app.prev_view.take() {
                            app.view = *prev;
                        } else {
                            app.view = View::AllSessions;
                        }
                        app.dir_query.clear();
                        app.dir_list.clear();
                        app.dir_filtered.clear();
                    }
                    KeyCode::Enter => {
                        if let Some(dir) = app.selected_dir().cloned() {
                            config::add_recent_dir(&dir.path);

                            if app.view == View::NewRemoteSession {
                                let ssh_host = app.remote_selected_host.clone().unwrap_or_default();
                                let host_name = app.remote_selected_host_name.clone().unwrap_or_default();
                                app.status_msg = Some(format!("Connecting to {}...", host_name));
                                terminal.draw(|f| ui::draw(f, &app))?;
                                restore_terminal();
                                let status = remote::open_new_remote_session(&ssh_host, app.remote_selected_port, &dir.path);
                                std::process::exit(status.code().unwrap_or(0));
                            } else if app.tmux_mode {
                                // Local tmux: wrap in tmux session
                                app.status_msg = Some("Starting tmux session...".into());
                                terminal.draw(|f| ui::draw(f, &app))?;
                                restore_terminal();
                                tmux_new_local(&dir.path);
                            } else {
                                // Local: replace TUI process with claude
                                app.status_msg = Some("Starting session...".into());
                                terminal.draw(|f| ui::draw(f, &app))?;
                                restore_terminal();
                                let err = Command::new("claude")
                                    .arg("--dangerously-skip-permissions")
                                    .current_dir(&dir.path)
                                    .exec();
                                eprintln!("Failed to launch claude: {}", err);
                                std::process::exit(1);
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        app.dir_query.pop();
                        app.apply_dir_filter();
                    }
                    KeyCode::Up => app.move_selection(-1),
                    KeyCode::Down => app.move_selection(1),
                    KeyCode::Char(c) => {
                        app.dir_query.push(c);
                        app.apply_dir_filter();
                    }
                    _ => {}
                }
                continue;
            }

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
                KeyCode::Down => {
                    if app.view == View::Folders && app.folder_preview_sel.is_some() {
                        // Navigate within expanded folder preview
                        let sel = app.folder_preview_sel.unwrap();
                        if let Some(folder_sel) = app.folder_state.selected() {
                            if let Some(&proj_idx) = app.folder_filtered.get(folder_sel) {
                                let count = app.sessions.iter()
                                    .filter(|s| s.project == app.projects[proj_idx].path)
                                    .count();
                                if sel + 1 < count {
                                    app.folder_preview_sel = Some(sel + 1);
                                    if app.expand_lines < sel + 2 {
                                        app.expand_lines = sel + 2;
                                    }
                                    app.folder_preview_expand = 0;
                                }
                                // At bottom of previews — stay put
                            }
                        }
                    } else {
                        app.move_selection(1);
                    }
                }
                KeyCode::Up => {
                    if app.view == View::Folders && app.folder_preview_sel.is_some() {
                        let sel = app.folder_preview_sel.unwrap();
                        if sel > 0 {
                            // Move up within preview
                            app.folder_preview_sel = Some(sel - 1);
                            app.folder_preview_expand = 0;
                        } else {
                            // At top of preview — collapse back to folder nav
                            app.folder_preview_sel = None;
                            app.expand_lines = 0;
                            app.folder_preview_expand = 0;
                        }
                    } else {
                        app.move_selection(-1);
                    }
                }
                KeyCode::Right => {
                    if app.view == View::RemoteSessions {
                        if let Some(idx) = app.remote_session_state.selected() {
                            // Lazy-load messages on first expand
                            if app.remote_sessions[idx].messages.is_empty() {
                                let ssh_host = app.remote_selected_host.clone().unwrap_or_default();
                                let session_id = app.remote_sessions[idx].id.clone();
                                app.status_msg = Some("Loading messages...".into());
                                terminal.draw(|f| ui::draw(f, &app))?;
                                if let Ok(turns) = remote::fetch_remote_messages(&ssh_host, app.remote_selected_port, &session_id) {
                                    app.remote_sessions[idx].messages = turns;
                                }
                                app.status_msg = None;
                            }
                            let max = app.remote_sessions[idx].messages.len().saturating_sub(1);
                            if app.expand_lines < max {
                                app.expand_lines += 1;
                            }
                        }
                    } else if app.view == View::Folders {
                        // Expand session previews under selected folder and advance selection
                        if let Some(sel) = app.folder_state.selected() {
                            if let Some(&proj_idx) = app.folder_filtered.get(sel) {
                                let count = app.sessions.iter()
                                    .filter(|s| s.project == app.projects[proj_idx].path)
                                    .count();
                                if app.expand_lines < count {
                                    app.expand_lines += 1;
                                }
                                // Select the latest expanded session
                                app.folder_preview_sel = Some(app.expand_lines.saturating_sub(1));
                                app.folder_preview_expand = 0;
                            }
                        }
                    } else if app.view != View::RemoteHosts {
                        if let Some(session) = app.selected_session() {
                            let max = session.messages.len().saturating_sub(1);
                            if app.expand_lines < max {
                                app.expand_lines += 1;
                            }
                        }
                    }
                }
                KeyCode::Left => {
                    if app.view == View::Folders && app.expand_lines > 0 {
                        if let Some(sel) = app.folder_preview_sel {
                            if sel > 0 {
                                // Move selection up one, collapse the last preview
                                app.folder_preview_sel = Some(sel - 1);
                                app.expand_lines = app.expand_lines.saturating_sub(1);
                                app.folder_preview_expand = 0;
                            } else {
                                // At first session, collapse all
                                app.expand_lines = 0;
                                app.folder_preview_sel = None;
                                app.folder_preview_expand = 0;
                            }
                        } else {
                            app.expand_lines = 0;
                        }
                    } else if app.expand_lines > 0 {
                        app.expand_lines = app.expand_lines.saturating_sub(1);
                    }
                }
                KeyCode::Char(' ') => {
                    // Spacebar: expand more conversation lines for selected folder preview
                    if app.view == View::Folders {
                        if let Some(session) = app.selected_folder_preview_session() {
                            let max = session.messages.len().saturating_sub(2); // 2 already shown
                            if app.folder_preview_expand < max {
                                app.folder_preview_expand += 2;
                            }
                        }
                    }
                }
                KeyCode::Char('G') => app.jump_bottom(),
                KeyCode::Char('g') => app.jump_top(),
                KeyCode::Char('/') => {
                    app.filtering = true;
                }
                KeyCode::Char('f') => {
                    // Quick jump to Folders view
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
                KeyCode::Char('r') => {
                    // Quick jump to Remote view
                    app.enter_remote_hosts();
                }
                KeyCode::Char('n') => {
                    if matches!(app.view, View::RemoteHosts | View::RemoteSessions) {
                        app.enter_new_remote_session();
                    } else {
                        app.enter_new_session();
                    }
                }
                KeyCode::Char('k') => {
                    // Kill: ask for confirmation before killing selected session
                    let has_active = match app.view {
                        View::Folders => {
                            app.selected_folder_preview_session()
                                .map_or(false, |s| s.active.is_some())
                        }
                        View::FolderSessions | View::AllSessions => {
                            app.selected_session().map_or(false, |s| s.active.is_some())
                        }
                        View::RemoteSessions => {
                            app.selected_remote_session().map_or(false, |s| s.active_pid.is_some())
                        }
                        _ => false,
                    };
                    if has_active {
                        app.confirm_kill = true;
                        app.status_msg = Some("Kill this session? (y/n)".into());
                    } else {
                        app.status_msg = Some("No active session to kill".into());
                    }
                }
                KeyCode::Char('t') => {
                    app.toggle_tmux_mode();
                }
                KeyCode::Char('a') => {
                    // Quick jump to All Sessions view
                    app.enter_all_sessions();
                }
                KeyCode::PageDown => app.move_selection(20),
                KeyCode::PageUp => app.move_selection(-20),
                KeyCode::Tab => match app.view {
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
                    View::NewSession | View::NewRemoteSession => {
                        // Handled above
                    }
                },
                KeyCode::Esc => {
                    if app.expand_lines > 0 {
                        app.expand_lines = 0;
                    } else {
                        match app.view {
                            View::FolderSessions => app.back_to_folders(),
                            View::RemoteSessions => app.enter_remote_hosts(),
                            View::Folders | View::AllSessions | View::RemoteHosts
                            | View::NewSession | View::NewRemoteSession => break,
                        }
                    }
                }
                KeyCode::Enter => match app.view {
                    View::Folders => {
                        if let Some(session) = app.selected_folder_preview_session() {
                            // A preview session is selected — resume/focus it
                            let sid = session.id.clone();
                            let cwd = session.project.clone();
                            let is_active = session.active.is_some();
                            let session_in_tmux = session.active.as_ref().map_or(false, |a| a.in_tmux);
                            let tmux_sess = session.active.as_ref().and_then(|a| a.tmux_session.clone());
                            let has_window = session.active.as_ref().and_then(|a| a.window_address.as_ref()).is_some();

                            if is_active && session_in_tmux {
                                if has_window {
                                    focus_active_session(session);
                                    break;
                                } else {
                                    app.status_msg = Some("Attaching tmux...".into());
                                    terminal.draw(|f| ui::draw(f, &app))?;
                                    restore_terminal();
                                    let tmux_name = tmux_sess.unwrap_or_else(|| session::tmux_session_name(&sid));
                                    tmux_attach_detach(&tmux_name);
                                }
                            } else if is_active {
                                focus_active_session(session);
                                break;
                            }

                            app.status_msg = Some("Resuming session...".into());
                            terminal.draw(|f| ui::draw(f, &app))?;
                            restore_terminal();
                            if app.tmux_mode {
                                tmux_resume_local(&sid, &cwd);
                            } else {
                                let err = Command::new("claude")
                                    .arg("--dangerously-skip-permissions")
                                    .arg("--resume")
                                    .arg(&sid)
                                    .current_dir(&cwd)
                                    .exec();
                                eprintln!("Failed to launch claude: {}", err);
                                std::process::exit(1);
                            }
                        } else {
                            app.enter_folder();
                        }
                    }
                    View::RemoteHosts => {
                        if let Some(host) = app.start_remote_host_load() {
                            terminal.draw(|f| ui::draw(f, &app))?;
                            let result = remote::fetch_remote_sessions(&host);
                            app.finish_remote_host_load(result);
                        }
                    }
                    View::RemoteSessions => {
                        if let Some(session) = app.selected_remote_session() {
                            let ssh_host = app.remote_selected_host.clone().unwrap_or_default();
                            let host_name = app.remote_selected_host_name.clone().unwrap_or_default();
                            let active_pid = session.active_pid;
                            let session_id = session.id.clone();
                            let project = session.project.clone();
                            let _ = session;

                            if let Some(pid) = active_pid {
                                // Check if it's running inside a tmux session — if so,
                                // just attach instead of killing. Only kill if not in tmux.
                                let in_tmux = remote::is_in_tmux_session(&ssh_host, app.remote_selected_port, &session_id);
                                if !in_tmux {
                                    app.status_msg = Some(format!("Killing PID {} on {}...", pid, host_name));
                                    terminal.draw(|f| ui::draw(f, &app))?;

                                    if remote::is_remote_pid_alive(&ssh_host, app.remote_selected_port, pid) {
                                        let _ = remote::kill_remote_pid(&ssh_host, app.remote_selected_port, pid);
                                        std::thread::sleep(std::time::Duration::from_millis(500));
                                    }
                                }
                            }

                            app.status_msg = Some(format!("Connecting to {}...", host_name));
                            terminal.draw(|f| ui::draw(f, &app))?;

                            restore_terminal();
                            let status = remote::open_remote_session_by_id(&ssh_host, app.remote_selected_port, &session_id, &project);
                            std::process::exit(status.code().unwrap_or(0));
                        }
                    }
                    View::FolderSessions | View::AllSessions => {
                        if let Some(session) = app.selected_session() {
                            let sid = session.id.clone();
                            let cwd = session.project.clone();
                            let is_active = session.active.is_some();
                            let session_in_tmux = session.active.as_ref().map_or(false, |a| a.in_tmux);
                            let tmux_sess = session.active.as_ref().and_then(|a| a.tmux_session.clone());

                            if is_active && session_in_tmux {
                                // Session is in tmux — check if there's a local window to focus
                                if session.active.as_ref().and_then(|a| a.window_address.as_ref()).is_some() {
                                    // Tmux client is in a local terminal window — focus it
                                    focus_active_session(session);
                                    break;
                                } else {
                                    // No local window (SSH client or detached) — attach here
                                    app.status_msg = Some("Attaching tmux...".into());
                                    terminal.draw(|f| ui::draw(f, &app))?;
                                    restore_terminal();
                                    let tmux_name = tmux_sess.unwrap_or_else(|| session::tmux_session_name(&sid));
                                    tmux_attach_detach(&tmux_name);
                                }
                            } else if is_active {
                                // Not in tmux — focus window via WM
                                focus_active_session(session);
                                break;
                            }

                            // Inactive session — resume
                            app.status_msg = Some("Resuming session...".into());
                            terminal.draw(|f| ui::draw(f, &app))?;
                            restore_terminal();

                            if app.tmux_mode {
                                tmux_resume_local(&sid, &cwd);
                            } else {
                                let err = Command::new("claude")
                                    .arg("--dangerously-skip-permissions")
                                    .arg("--resume")
                                    .arg(&sid)
                                    .current_dir(&cwd)
                                    .exec();
                                eprintln!("Failed to launch claude: {}", err);
                                std::process::exit(1);
                            }
                        }
                    }
                    View::NewSession | View::NewRemoteSession => {
                        // Handled above in the NewSession input block
                    }
                },
                _ => {}
            }
        }
    }

    restore_terminal();
    Ok(())
}
