mod app;
mod config;
mod remote;
mod session;
mod ui;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers, DisableMouseCapture},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::Terminal;
use serde::Serialize;
use std::{io::stdout, process::Command};

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
    messages: Vec<session::Turn>,
}

#[derive(Serialize)]
struct JsonOutput {
    sessions: Vec<JsonSession>,
}

/// Focus an active session's terminal window via hyprctl
fn focus_active_session(session: &Session) {
    if let Some(ref info) = session.active {
        if let Some(ref addr) = info.window_address {
            let _ = Command::new("hyprctl")
                .args(["dispatch", "focuswindow", &format!("address:{}", addr)])
                .output();
        }
    }
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

    // --json flag: dump session data as JSON for remote consumption
    if std::env::args().any(|a| a == "--json") {
        let sessions = session::load_sessions();
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

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        if let Event::Key(key) = event::read()? {
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
                                restore_terminal();
                                let status = remote::open_new_remote_session(&ssh_host, &dir.path);
                                std::process::exit(status.code().unwrap_or(0));
                            } else {
                                // Local: replace TUI process with claude
                                restore_terminal();
                                let status = Command::new("claude")
                                    .arg("--dangerously-skip-permissions")
                                    .current_dir(&dir.path)
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
                    }
                    KeyCode::Backspace => {
                        app.dir_query.pop();
                        app.apply_dir_filter();
                    }
                    KeyCode::Up => app.move_selection(-1),
                    KeyCode::Down => app.move_selection(1),
                    KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.move_selection(-1);
                    }
                    KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.move_selection(1);
                    }
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
                    View::Folders => app.enter_folder(),
                    View::RemoteHosts => app.enter_remote_host(),
                    View::RemoteSessions => {
                        if let Some(session) = app.selected_remote_session() {
                            let ssh_host = app.remote_selected_host.clone().unwrap_or_default();

                            if let Some(pid) = session.active_pid {
                                // Verify PID is actually still alive before killing
                                if remote::is_remote_pid_alive(&ssh_host, pid) {
                                    let _ = remote::kill_remote_pid(&ssh_host, pid);
                                    std::thread::sleep(std::time::Duration::from_millis(500));
                                }
                            }

                            restore_terminal();
                            let status = remote::open_remote_session(&ssh_host, session);
                            std::process::exit(status.code().unwrap_or(0));
                        }
                    }
                    View::FolderSessions | View::AllSessions => {
                        if let Some(session) = app.selected_session() {
                            if session.active.is_some() {
                                focus_active_session(session);
                                break;
                            }

                            let sid = session.id.clone();
                            let cwd = session.project.clone();

                            restore_terminal();

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
