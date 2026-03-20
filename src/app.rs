use ratatui::widgets::ListState;
use std::collections::HashMap;

use crate::config;
use crate::remote;
use crate::session::{DirEntry, Project, Session};

#[derive(Debug, PartialEq)]
pub enum View {
    Folders,
    FolderSessions,
    AllSessions,
    RemoteHosts,
    RemoteSessions,
    NewSession,
    NewRemoteSession,
}

pub struct App {
    pub sessions: Vec<Session>,
    pub projects: Vec<Project>,
    pub view: View,
    pub folder_state: ListState,
    pub folder_filtered: Vec<usize>,
    pub session_state: ListState,
    pub session_filtered: Vec<usize>,
    pub selected_project: Option<String>,
    pub filter: String,
    pub filtering: bool,
    pub expand_lines: usize,
    // Remote
    pub remote_hosts: Vec<config::HostConfig>,
    pub remote_host_state: ListState,
    pub remote_sessions: Vec<remote::RemoteSession>,
    pub remote_session_state: ListState,
    pub remote_selected_host: Option<String>,
    pub remote_selected_host_name: Option<String>,
    pub remote_selected_port: Option<u16>,
    pub remote_gpu: bool,
    pub remote_selected_config: Option<config::HostConfig>,
    pub remote_error: Option<String>,
    pub remote_loading: bool,
    // New session directory picker
    pub dir_list: Vec<DirEntry>,
    pub dir_filtered: Vec<usize>,
    pub dir_state: ListState,
    pub dir_query: String,
    pub recent_dirs: Vec<String>,
    pub prev_view: Option<Box<View>>,
    pub status_msg: Option<String>,
    pub status_msg_time: Option<std::time::Instant>,
    pub tmux_mode: bool,
    pub confirm_kill: bool,
    /// Which previewed session is selected in folder view (0 = most recent)
    pub folder_preview_sel: Option<usize>,
    /// How many extra conversation lines to show for the selected folder preview session
    pub folder_preview_expand: usize,
    /// Whether initial session load is still in progress
    pub loading: bool,
}

impl App {
    /// Create an empty app that shows a loading state.
    pub fn empty() -> Self {
        let remote_hosts = config::load_hosts();
        let mut remote_host_state = ListState::default();
        if !remote_hosts.is_empty() {
            remote_host_state.select(Some(0));
        }
        let recent_dirs = config::load_recent_dirs();
        App {
            sessions: vec![],
            projects: vec![],
            view: View::AllSessions,
            folder_state: ListState::default(),
            folder_filtered: vec![],
            session_state: ListState::default(),
            session_filtered: vec![],
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
            remote_selected_port: None,
            remote_gpu: false,
            remote_selected_config: None,
            remote_error: None,
            remote_loading: false,
            dir_list: Vec::new(),
            dir_filtered: Vec::new(),
            dir_state: ListState::default(),
            dir_query: String::new(),
            recent_dirs,
            prev_view: None,
            status_msg: Some("Loading sessions...".into()),
            status_msg_time: None,
            tmux_mode: false,
            confirm_kill: false,
            folder_preview_sel: None,
            folder_preview_expand: 0,
            loading: true,
        }
    }

    /// Populate app with loaded sessions (called after background load completes).
    pub fn populate(&mut self, sessions: Vec<Session>) {
        self.loading = false;
        self.status_msg = None;
        self.status_msg_time = None;

        // Re-use the same logic as new()
        let mut sessions = sessions;
        sessions.sort_by(|a, b| {
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

        self.folder_filtered = (0..projects.len()).collect();
        if !self.folder_filtered.is_empty() {
            self.folder_state.select(Some(0));
        }

        self.session_filtered = (0..sessions.len()).collect();
        if !self.session_filtered.is_empty() {
            self.session_state.select(Some(0));
        }

        self.sessions = sessions;
        self.projects = projects;
    }

    pub fn enter_folder(&mut self) {
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

    pub fn enter_all_sessions(&mut self) {
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

    pub fn back_to_folders(&mut self) {
        self.view = View::Folders;
        self.filter.clear();
        self.filtering = false;
        self.expand_lines = 0;
        self.apply_filter();
    }

    pub fn apply_filter(&mut self) {
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
                if let Some(sel) = self.folder_state.selected() {
                    if sel >= self.folder_filtered.len() {
                        self.folder_state.select(if self.folder_filtered.is_empty() {
                            None
                        } else {
                            Some(0)
                        });
                    }
                }
            }
            View::FolderSessions | View::AllSessions => {
                let project_filter = self.selected_project.clone();
                self.session_filtered = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        if let Some(ref pf) = project_filter {
                            if s.project != *pf {
                                return false;
                            }
                        }
                        if q.is_empty() {
                            return true;
                        }
                        s.project.to_lowercase().contains(&q)
                            || s.first_msg.to_lowercase().contains(&q)
                            || s.last_msg.to_lowercase().contains(&q)
                            || s.messages.iter().any(|m| m.text.to_lowercase().contains(&q))
                    })
                    .map(|(i, _)| i)
                    .collect();
                if let Some(sel) = self.session_state.selected() {
                    if sel >= self.session_filtered.len() {
                        self.session_state.select(if self.session_filtered.is_empty() {
                            None
                        } else {
                            Some(0)
                        });
                    }
                }
            }
            View::RemoteHosts | View::RemoteSessions => {
                // No filtering for remote views currently
            }
            View::NewSession | View::NewRemoteSession => {
                // Dir filtering uses apply_dir_filter instead
            }
        }
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.session_state
            .selected()
            .and_then(|i| self.session_filtered.get(i))
            .map(|&idx| &self.sessions[idx])
    }

    pub fn current_list_state_mut(&mut self) -> &mut ListState {
        match self.view {
            View::Folders => &mut self.folder_state,
            View::FolderSessions | View::AllSessions => &mut self.session_state,
            View::RemoteHosts => &mut self.remote_host_state,
            View::RemoteSessions => &mut self.remote_session_state,
            View::NewSession | View::NewRemoteSession => &mut self.dir_state,
        }
    }

    pub fn current_list_len(&self) -> usize {
        match self.view {
            View::Folders => self.folder_filtered.len(),
            View::FolderSessions | View::AllSessions => self.session_filtered.len(),
            View::RemoteHosts => self.remote_hosts.len(),
            View::RemoteSessions => self.remote_sessions.len(),
            View::NewSession | View::NewRemoteSession => self.dir_filtered.len(),
        }
    }

    pub fn enter_new_session(&mut self) {
        self.dir_list = crate::session::discover_dirs(&self.recent_dirs);
        self.dir_query.clear();
        self.dir_filtered = (0..self.dir_list.len()).collect();
        self.dir_state = ListState::default();
        if !self.dir_filtered.is_empty() {
            self.dir_state.select(Some(0));
        }
        self.prev_view = Some(Box::new(std::mem::replace(&mut self.view, View::NewSession)));
    }

    pub fn enter_new_remote_session(&mut self) {
        // For remote, we need to fetch dirs from the remote host
        let host = match &self.remote_selected_config {
            Some(h) => h.clone(),
            None => return,
        };
        match remote::fetch_remote_dirs(&host) {
            Ok(dirs) => {
                self.dir_list = dirs;
                // Re-score with local recent_dirs knowledge
                for entry in &mut self.dir_list {
                    if self.recent_dirs.contains(&entry.path) {
                        entry.score += 50;
                    }
                }
                self.dir_list.sort_by(|a, b| b.score.cmp(&a.score).then(a.display.cmp(&b.display)));
            }
            Err(e) => {
                self.dir_list = Vec::new();
                self.remote_error = Some(e);
            }
        }
        self.dir_query.clear();
        self.dir_filtered = (0..self.dir_list.len()).collect();
        self.dir_state = ListState::default();
        if !self.dir_filtered.is_empty() {
            self.dir_state.select(Some(0));
        }
        self.prev_view = Some(Box::new(std::mem::replace(&mut self.view, View::NewRemoteSession)));
    }

    pub fn apply_dir_filter(&mut self) {
        let q = self.dir_query.to_lowercase();
        self.dir_filtered = self
            .dir_list
            .iter()
            .enumerate()
            .filter(|(_, d)| q.is_empty() || d.display.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        if let Some(sel) = self.dir_state.selected() {
            if sel >= self.dir_filtered.len() {
                self.dir_state.select(if self.dir_filtered.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
        } else if !self.dir_filtered.is_empty() {
            self.dir_state.select(Some(0));
        }
    }

    pub fn selected_dir(&self) -> Option<&DirEntry> {
        self.dir_state
            .selected()
            .and_then(|i| self.dir_filtered.get(i))
            .map(|&idx| &self.dir_list[idx])
    }

    pub fn enter_remote_hosts(&mut self) {
        self.view = View::RemoteHosts;
        self.filter.clear();
        self.filtering = false;
        self.expand_lines = 0;
        self.remote_error = None;
        if !self.remote_hosts.is_empty() && self.remote_host_state.selected().is_none() {
            self.remote_host_state.select(Some(0));
        }
    }

    /// Prepare to load a remote host (sets loading state). Returns the host config
    /// for the caller to fetch sessions and call `finish_remote_host_load`.
    pub fn start_remote_host_load(&mut self) -> Option<config::HostConfig> {
        let idx = match self.remote_host_state.selected() {
            Some(i) => i,
            None => return None,
        };
        let host = match self.remote_hosts.get(idx) {
            Some(h) => h.clone(),
            None => return None,
        };

        self.remote_loading = true;
        self.remote_selected_host = Some(host.ssh.clone());
        self.remote_selected_host_name = Some(host.name.clone());
        self.remote_selected_port = host.port;
        self.remote_gpu = host.gpu;
        self.remote_selected_config = Some(host.clone());
        self.status_msg = Some(format!("Connecting to {}...", host.name));
        Some(host)
    }

    /// Finish loading remote sessions after the fetch completes.
    pub fn finish_remote_host_load(&mut self, result: Result<Vec<remote::RemoteSession>, String>) {
        match result {
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
        self.status_msg = None;
        self.expand_lines = 0;
    }

    pub fn selected_remote_session(&self) -> Option<&remote::RemoteSession> {
        self.remote_session_state
            .selected()
            .and_then(|i| self.remote_sessions.get(i))
    }

    /// Get the session selected in folder preview mode, if any.
    pub fn selected_folder_preview_session(&self) -> Option<&Session> {
        let sel = self.folder_preview_sel?;
        let folder_idx = *self.folder_filtered.get(self.folder_state.selected()?)?;
        let project_path = &self.projects[folder_idx].path;
        let mut folder_sessions: Vec<&Session> = self.sessions.iter()
            .filter(|s| s.project == *project_path)
            .collect();
        folder_sessions.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
        folder_sessions.get(sel).copied()
    }

    /// Set a status message that auto-expires after the given duration.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = Some(msg.into());
        self.status_msg_time = Some(std::time::Instant::now());
    }

    /// Clear status message if it has expired (10 seconds).
    pub fn clear_expired_status(&mut self) {
        if let Some(t) = self.status_msg_time {
            if t.elapsed() >= std::time::Duration::from_secs(10) {
                self.status_msg = None;
                self.status_msg_time = None;
            }
        }
    }

    pub fn toggle_tmux_mode(&mut self) {
        self.tmux_mode = !self.tmux_mode;
    }

    pub fn move_selection(&mut self, delta: i32) {
        let len = self.current_list_len() as i32;
        if len == 0 {
            return;
        }
        let state = self.current_list_state_mut();
        let current = state.selected().unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, len - 1) as usize;
        state.select(Some(next));
        self.expand_lines = 0;
        self.folder_preview_sel = None;
        self.folder_preview_expand = 0;
    }

    pub fn jump_top(&mut self) {
        if self.current_list_len() > 0 {
            self.current_list_state_mut().select(Some(0));
            self.expand_lines = 0;
            self.folder_preview_sel = None;
            self.folder_preview_expand = 0;
        }
    }

    pub fn jump_bottom(&mut self) {
        let len = self.current_list_len();
        if len > 0 {
            self.current_list_state_mut().select(Some(len - 1));
            self.expand_lines = 0;
            self.folder_preview_sel = None;
            self.folder_preview_expand = 0;
        }
    }

    /// Reload sessions from disk, preserving current view and selection state.
    pub fn refresh(&mut self) {
        // Remember what's selected so we can restore position
        let selected_session_id = match self.view {
            View::FolderSessions | View::AllSessions => {
                self.selected_session().map(|s| s.id.clone())
            }
            _ => None,
        };
        let selected_folder_path = match self.view {
            View::Folders | View::FolderSessions => {
                self.folder_state.selected()
                    .and_then(|i| self.folder_filtered.get(i))
                    .map(|&idx| self.projects[idx].path.clone())
            }
            _ => None,
        };

        // Reload
        let mut sessions = crate::session::load_sessions();
        sessions.sort_by(|a, b| {
            let a_active = a.active.is_some();
            let b_active = b.active.is_some();
            b_active.cmp(&a_active).then(b.last_ts.cmp(&a.last_ts))
        });

        // Rebuild projects
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

        self.sessions = sessions;
        self.projects = projects;

        // Rebuild folder filter
        self.folder_filtered = (0..self.projects.len()).collect();

        // Restore folder selection
        if let Some(ref path) = selected_folder_path {
            let pos = self.folder_filtered.iter().position(|&idx| self.projects[idx].path == *path);
            self.folder_state.select(pos.or(Some(0)));
        } else if self.folder_state.selected().map_or(false, |i| i >= self.folder_filtered.len()) {
            self.folder_state.select(if self.folder_filtered.is_empty() { None } else { Some(0) });
        }

        // Rebuild session filter based on current view
        match self.view {
            View::FolderSessions => {
                if let Some(ref proj) = self.selected_project {
                    self.session_filtered = self.sessions.iter().enumerate()
                        .filter(|(_, s)| s.project == *proj)
                        .map(|(i, _)| i)
                        .collect();
                }
            }
            View::AllSessions => {
                self.session_filtered = (0..self.sessions.len()).collect();
            }
            _ => {}
        }
        self.apply_filter();

        // Restore session selection
        if let Some(ref sid) = selected_session_id {
            let pos = self.session_filtered.iter().position(|&idx| self.sessions[idx].id == *sid);
            self.session_state.select(pos.or(Some(0)));
        } else if self.session_state.selected().map_or(false, |i| i >= self.session_filtered.len()) {
            self.session_state.select(if self.session_filtered.is_empty() { None } else { Some(0) });
        }

        // Reset expand state since the data changed
        self.expand_lines = 0;
        self.folder_preview_sel = None;
        self.folder_preview_expand = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Session, Turn};

    fn make_session(id: &str, project: &str, ts: u64) -> Session {
        Session {
            id: id.to_string(),
            project: project.to_string(),
            last_ts: ts,
            msg_count: 1,
            first_msg: format!("msg from {}", id),
            last_msg: format!("last from {}", id),
            last_cwd: None,
            active: None,
            messages: vec![
                Turn { role: "user".into(), text: format!("hello {}", id) },
                Turn { role: "assistant".into(), text: format!("hi from {}", id) },
            ],
        }
    }

    #[test]
    fn test_empty_app() {
        let app = App::empty();
        assert!(app.loading);
        assert!(app.sessions.is_empty());
        assert!(app.projects.is_empty());
        assert_eq!(app.status_msg.as_deref(), Some("Loading sessions..."));
    }

    #[test]
    fn test_populate() {
        let mut app = App::empty();
        let sessions = vec![
            make_session("s1", "/proj/a", 1000),
            make_session("s2", "/proj/a", 2000),
            make_session("s3", "/proj/b", 3000),
        ];
        app.populate(sessions);

        assert!(!app.loading);
        assert_eq!(app.sessions.len(), 3);
        assert_eq!(app.projects.len(), 2);
        assert_eq!(app.session_filtered.len(), 3);
        assert_eq!(app.session_state.selected(), Some(0));
        assert!(app.status_msg.is_none());
    }

    #[test]
    fn test_populate_sorts_by_timestamp() {
        let mut app = App::empty();
        let sessions = vec![
            make_session("old", "/proj", 1000),
            make_session("new", "/proj", 3000),
            make_session("mid", "/proj", 2000),
        ];
        app.populate(sessions);

        // Should be sorted newest first
        assert_eq!(app.sessions[0].id, "new");
        assert_eq!(app.sessions[1].id, "mid");
        assert_eq!(app.sessions[2].id, "old");
    }

    #[test]
    fn test_move_selection() {
        let mut app = App::empty();
        app.populate(vec![
            make_session("s1", "/a", 1000),
            make_session("s2", "/b", 2000),
            make_session("s3", "/c", 3000),
        ]);

        assert_eq!(app.session_state.selected(), Some(0));
        app.move_selection(1);
        assert_eq!(app.session_state.selected(), Some(1));
        app.move_selection(1);
        assert_eq!(app.session_state.selected(), Some(2));
        // Can't go past end
        app.move_selection(1);
        assert_eq!(app.session_state.selected(), Some(2));
        // Can go back
        app.move_selection(-1);
        assert_eq!(app.session_state.selected(), Some(1));
    }

    #[test]
    fn test_jump_top_bottom() {
        let mut app = App::empty();
        app.populate(vec![
            make_session("s1", "/a", 1000),
            make_session("s2", "/b", 2000),
            make_session("s3", "/c", 3000),
        ]);

        app.jump_bottom();
        assert_eq!(app.session_state.selected(), Some(2));
        app.jump_top();
        assert_eq!(app.session_state.selected(), Some(0));
    }

    #[test]
    fn test_filter_sessions() {
        let mut app = App::empty();
        app.populate(vec![
            make_session("s1", "/proj/alpha", 1000),
            make_session("s2", "/proj/beta", 2000),
            make_session("s3", "/proj/gamma", 3000),
        ]);

        app.filter = "beta".into();
        app.apply_filter();
        assert_eq!(app.session_filtered.len(), 1);
        assert_eq!(app.sessions[app.session_filtered[0]].project, "/proj/beta");
    }

    #[test]
    fn test_filter_searches_messages() {
        let mut app = App::empty();
        let mut s = make_session("s1", "/proj", 1000);
        s.messages = vec![
            Turn { role: "user".into(), text: "find the needle".into() },
        ];
        app.populate(vec![
            s,
            make_session("s2", "/proj", 2000),
        ]);

        app.filter = "needle".into();
        app.apply_filter();
        assert_eq!(app.session_filtered.len(), 1);
    }

    #[test]
    fn test_filter_empty_shows_all() {
        let mut app = App::empty();
        app.populate(vec![
            make_session("s1", "/a", 1000),
            make_session("s2", "/b", 2000),
        ]);

        app.filter = "".into();
        app.apply_filter();
        assert_eq!(app.session_filtered.len(), 2);
    }

    #[test]
    fn test_enter_folder() {
        let mut app = App::empty();
        app.populate(vec![
            make_session("s1", "/proj/a", 1000),
            make_session("s2", "/proj/a", 2000),
            make_session("s3", "/proj/b", 3000),
        ]);

        app.view = View::Folders;
        app.folder_state.select(Some(0));
        app.enter_folder();

        assert_eq!(app.view, View::FolderSessions);
        // Should only show sessions from the selected project
        assert!(app.session_filtered.len() <= 2);
    }

    #[test]
    fn test_toggle_tmux() {
        let mut app = App::empty();
        assert!(!app.tmux_mode);
        app.toggle_tmux_mode();
        assert!(app.tmux_mode);
        app.toggle_tmux_mode();
        assert!(!app.tmux_mode);
    }

    #[test]
    fn test_set_status_and_expire() {
        let mut app = App::empty();
        app.set_status("test message");
        assert_eq!(app.status_msg.as_deref(), Some("test message"));
        assert!(app.status_msg_time.is_some());

        // Shouldn't expire immediately
        app.clear_expired_status();
        assert!(app.status_msg.is_some());
    }

    #[test]
    fn test_selected_session() {
        let mut app = App::empty();
        app.populate(vec![
            make_session("s1", "/a", 3000),
            make_session("s2", "/b", 2000),
        ]);

        let s = app.selected_session().unwrap();
        assert_eq!(s.id, "s1"); // Newest first
    }

    #[test]
    fn test_move_selection_empty() {
        let mut app = App::empty();
        // Shouldn't panic on empty list
        app.move_selection(1);
        app.move_selection(-1);
        app.jump_top();
        app.jump_bottom();
    }

    #[test]
    fn test_back_to_folders() {
        let mut app = App::empty();
        app.populate(vec![make_session("s1", "/a", 1000)]);
        app.view = View::FolderSessions;
        app.filter = "something".into();
        app.expand_lines = 5;

        app.back_to_folders();
        assert_eq!(app.view, View::Folders);
        assert!(app.filter.is_empty());
        assert_eq!(app.expand_lines, 0);
    }
}
