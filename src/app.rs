use ratatui::widgets::ListState;
use std::collections::HashMap;

use crate::config;
use crate::remote;
use crate::session::{DirEntry, Project, Session};

#[derive(PartialEq)]
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
    pub remote_gpu: bool,
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
    // Path editing for resume
    pub editing_path: bool,
    pub edit_path_buf: String,
    pub edit_session_id: Option<String>,
}

impl App {
    pub fn new(mut sessions: Vec<Session>) -> Self {
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

        let remote_hosts = config::load_hosts();
        let mut remote_host_state = ListState::default();
        if !remote_hosts.is_empty() {
            remote_host_state.select(Some(0));
        }

        let recent_dirs = config::load_recent_dirs();

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
            remote_gpu: false,
            remote_error: None,
            remote_loading: false,
            dir_list: Vec::new(),
            dir_filtered: Vec::new(),
            dir_state: ListState::default(),
            dir_query: String::new(),
            recent_dirs,
            prev_view: None,
            status_msg: None,
            editing_path: false,
            edit_path_buf: String::new(),
            edit_session_id: None,
        }
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
                        s.first_msg.to_lowercase().contains(&q)
                            || s.last_msg.to_lowercase().contains(&q)
                            || s.project.to_lowercase().contains(&q)
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
        let ssh_host = match &self.remote_selected_host {
            Some(h) => h.clone(),
            None => return,
        };
        match remote::fetch_remote_dirs(&ssh_host) {
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
        self.remote_gpu = host.gpu;
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
    }

    pub fn jump_top(&mut self) {
        if self.current_list_len() > 0 {
            self.current_list_state_mut().select(Some(0));
            self.expand_lines = 0;
        }
    }

    pub fn jump_bottom(&mut self) {
        let len = self.current_list_len();
        if len > 0 {
            self.current_list_state_mut().select(Some(len - 1));
            self.expand_lines = 0;
        }
    }
}
