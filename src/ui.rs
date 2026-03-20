use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::app::{App, View};
use crate::session::{format_time_ago, short_project, Turn};

pub fn draw(f: &mut Frame, app: &App) {
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
        View::NewSession => format!(
            "  New Session  {}/{} dirs",
            app.dir_filtered.len(),
            app.dir_list.len()
        ),
        View::NewRemoteSession => {
            let host_name = app.remote_selected_host_name.as_deref().unwrap_or("?");
            format!(
                "  New Remote Session @ {}  {}/{} dirs",
                host_name,
                app.dir_filtered.len(),
                app.dir_list.len()
            )
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
    let tab_folders = make_tab(
        "Folders",
        app.view == View::Folders || app.view == View::FolderSessions,
    );
    let tab_remote = make_tab(
        "Remote",
        app.view == View::RemoteHosts || app.view == View::RemoteSessions,
    );

    let tmux_indicator = if app.tmux_mode {
        Span::styled(
            " [tmux]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" [tmux]", Style::default().fg(Color::Rgb(60, 60, 70)))
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " Claude Resume",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        tmux_indicator,
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
        View::NewSession | View::NewRemoteSession => draw_new_session(f, app, chunks[1]),
    }

    let footer_text = if let Some(ref msg) = app.status_msg {
        Line::from(vec![
            Span::styled(" ● ", Style::default().fg(Color::Yellow)),
            Span::styled(msg.as_str(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ])
    } else if matches!(app.view, View::NewSession | View::NewRemoteSession) {
        Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.dir_query),
            Span::styled("█", Style::default().fg(Color::Yellow)),
            Span::styled("    enter", Style::default().fg(Color::Green)),
            Span::raw(" select  "),
            Span::styled("esc", Style::default().fg(Color::Green)),
            Span::raw(" back"),
        ])
    } else if app.filtering {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.filter),
            Span::styled("█", Style::default().fg(Color::Yellow)),
        ])
    } else {
        // Determine Enter action label based on selected session
        let enter_label = match app.view {
            View::Folders | View::RemoteHosts => " open  ",
            View::FolderSessions | View::AllSessions => {
                if let Some(session) = app.selected_session() {
                    match &session.active {
                        Some(info) if info.in_tmux => " attach  ",
                        Some(_) => " focus  ",
                        None => " resume  ",
                    }
                } else {
                    " resume  "
                }
            }
            _ => " resume/focus  ",
        };
        let mut hints = vec![
            Span::styled(" enter", Style::default().fg(Color::Green)),
            Span::raw(enter_label),
        ];
        if app.view == View::FolderSessions || app.view == View::RemoteSessions {
            hints.push(Span::styled("esc", Style::default().fg(Color::Green)));
            hints.push(Span::raw(" back  "));
        }
        if matches!(
            app.view,
            View::Folders | View::FolderSessions | View::AllSessions | View::RemoteSessions
        ) {
            hints.push(Span::styled("←/→", Style::default().fg(Color::Green)));
            hints.push(Span::raw(if app.view == View::Folders { " preview  " } else { " expand  " }));
        }
        hints.push(Span::styled("a", Style::default().fg(Color::Green)));
        hints.push(Span::raw("ll "));
        hints.push(Span::styled("f", Style::default().fg(Color::Green)));
        hints.push(Span::raw("olders "));
        hints.push(Span::styled("r", Style::default().fg(Color::Green)));
        hints.push(Span::raw("emote  "));
        hints.push(Span::styled("n", Style::default().fg(Color::Green)));
        hints.push(Span::raw("ew  "));
        hints.push(Span::styled("t", Style::default().fg(Color::Green)));
        hints.push(Span::raw("mux  "));
        // Show kill hint when selected session is active
        let show_kill = match app.view {
            View::Folders => {
                app.selected_folder_preview_session().map_or(false, |s| s.active.is_some())
            }
            View::FolderSessions | View::AllSessions => {
                app.selected_session().map_or(false, |s| s.active.is_some())
            }
            View::RemoteSessions => {
                app.selected_remote_session().map_or(false, |s| s.active_pid.is_some())
            }
            _ => false,
        };
        if show_kill {
            hints.push(Span::styled("k", Style::default().fg(Color::Red)));
            hints.push(Span::raw("ill  "));
        }
        if !matches!(app.view, View::RemoteHosts | View::RemoteSessions) {
            hints.push(Span::styled("/", Style::default().fg(Color::Green)));
            hints.push(Span::raw(" filter  "));
        }
        hints.push(Span::styled("q", Style::default().fg(Color::Green)));
        hints.push(Span::raw("uit"));
        Line::from(hints)
    };
    let footer = Paragraph::new(footer_text);
    f.render_widget(footer, chunks[2]);
}

pub fn draw_folders(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let selected_idx = app.folder_state.selected();
    let w = area.width as usize;

    let items: Vec<ListItem> = app
        .folder_filtered
        .iter()
        .enumerate()
        .map(|(list_pos, &idx)| {
            let p = &app.projects[idx];
            let time_ago = format_time_ago(p.last_ts);
            let path = short_project(&p.path);

            let folder_line = Line::from(vec![
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
            ]);

            let is_selected = selected_idx == Some(list_pos);
            if is_selected && app.expand_lines > 0 {
                let mut lines = vec![folder_line];

                // Get sessions for this folder, sorted by timestamp desc
                let mut folder_sessions: Vec<&crate::session::Session> = app.sessions.iter()
                    .filter(|s| s.project == p.path)
                    .collect();
                folder_sessions.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));

                let show = app.expand_lines.min(folder_sessions.len());
                let preview_sel = app.folder_preview_sel;
                for (si, s) in folder_sessions.iter().take(show).enumerate() {
                    let is_preview_selected = preview_sel == Some(si);
                    let s_time = format_time_ago(s.last_ts);
                    let active = if s.active.is_some() {
                        Span::styled(" ● ", Style::default().fg(Color::Green))
                    } else {
                        Span::styled("   ", Style::default())
                    };
                    let sel_marker = if is_preview_selected { "  ▹ " } else { "    " };
                    let header_bg = if is_preview_selected {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::Rgb(80, 80, 100))
                    };

                    // Session header line
                    lines.push(Line::from(vec![
                        Span::styled(sel_marker, Style::default().fg(Color::Cyan)),
                        active,
                        Span::styled(format!("{:>8}", s_time), header_bg),
                        Span::styled(format!("  {}m  ", s.msg_count), header_bg),
                    ]));

                    // Messages to show: last 2 by default + extra from spacebar expand
                    let msg_width = w.saturating_sub(10);
                    let extra = if is_preview_selected { app.folder_preview_expand } else { 0 };
                    let default_show = 2;
                    let total_show = (default_show + extra).min(s.messages.len());

                    if total_show > 0 && !s.messages.is_empty() {
                        let start = s.messages.len().saturating_sub(total_show);
                        for turn in s.messages[start..].iter() {
                            let is_asst = turn.role == "assistant";
                            let prefix = if is_asst { "      ▸ " } else { "      » " };
                            let color = if is_asst {
                                Color::Rgb(140, 140, 160)
                            } else {
                                Color::Rgb(170, 170, 185)
                            };
                            let prefix_color = if is_asst {
                                Color::Rgb(180, 130, 255)
                            } else {
                                Color::Rgb(100, 180, 100)
                            };
                            let text: String = turn.text.chars().take(msg_width).collect();
                            lines.push(Line::from(vec![
                                Span::styled(prefix, Style::default().fg(prefix_color)),
                                Span::styled(text, Style::default().fg(color)),
                            ]));
                        }
                    } else if !s.last_msg.is_empty() {
                        let text: String = s.last_msg.chars().take(msg_width).collect();
                        lines.push(Line::from(vec![
                            Span::raw("      "),
                            Span::styled(text, Style::default().fg(Color::Rgb(170, 170, 185))),
                        ]));
                    }
                }
                lines.push(Line::raw(""));
                ListItem::new(lines)
            } else {
                ListItem::new(folder_line)
            }
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
pub fn wrap_text(text: &str, width: usize, indent: usize, style: Style) -> Vec<Line<'static>> {
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
        let spans = vec![Span::raw(prefix.clone()), Span::styled(chunk, style)];
        lines.push(Line::from(spans));
        pos = end;
    }
    lines
}

/// Data needed to render one session row (local or remote).
struct SessionRow<'a> {
    last_ts: u64,
    project_display: String,
    cwd_display: Option<String>,
    active_marker: String,
    last_msg: &'a str,
    msg_count: usize,
    messages: &'a [Turn],
}

fn draw_session_list<'a>(
    f: &mut Frame,
    area: ratatui::layout::Rect,
    rows: Vec<SessionRow<'a>>,
    selected_idx: Option<usize>,
    expand_lines: usize,
    list_state: &mut ListState,
) {
    let w = area.width as usize;

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(list_pos, s)| {
            let time_ago = format_time_ago(s.last_ts);

            let active_marker = if s.active_marker.is_empty() {
                Span::styled("  ", Style::default())
            } else {
                Span::styled(format!(" ●{}", s.active_marker), Style::default().fg(Color::Green))
            };

            let mut line1_spans = vec![
                active_marker,
                Span::styled(
                    format!(" {:>8}", time_ago),
                    Style::default().fg(Color::Rgb(100, 100, 120)),
                ),
                Span::raw("  "),
                Span::styled(
                    s.project_display.clone(),
                    Style::default().fg(Color::Rgb(100, 160, 220)),
                ),
            ];
            if let Some(ref cwd) = s.cwd_display {
                if cwd != &s.project_display {
                    line1_spans.push(Span::styled(
                        format!("  cwd:{}", cwd),
                        Style::default().fg(Color::Rgb(80, 120, 80)),
                    ));
                }
            }
            line1_spans.push(Span::styled(
                format!("  {}m", s.msg_count),
                Style::default().fg(Color::Rgb(80, 80, 100)),
            ));
            let line1 = Line::from(line1_spans);

            // Default preview: show last 2 messages (user + assistant) from conversation
            let msg_width = w.saturating_sub(5);
            let default_preview_count = if s.messages.len() >= 2 { 2 } else { 0 };
            let preview_lines: Vec<Line> = if default_preview_count > 0 {
                // Show last N messages (most recent at bottom)
                let start = s.messages.len() - default_preview_count;
                s.messages[start..].iter().map(|turn| {
                    let is_assistant = turn.role == "assistant";
                    let role_label = if is_assistant { "  ▸ " } else { "  » " };
                    let role_color = if is_assistant {
                        Color::Rgb(180, 130, 255)
                    } else {
                        Color::Rgb(100, 180, 100)
                    };
                    let text_color = if is_assistant {
                        Color::Rgb(140, 140, 160)
                    } else {
                        Color::Rgb(170, 170, 185)
                    };
                    let text: String = turn.text.chars().take(msg_width.saturating_sub(4)).collect();
                    Line::from(vec![
                        Span::styled(role_label, Style::default().fg(role_color)),
                        Span::styled(text, Style::default().fg(text_color)),
                    ])
                }).collect()
            } else {
                // Fallback: use last_msg from history
                let preview: String = s.last_msg.chars().take(msg_width).collect();
                vec![Line::from(vec![
                    Span::raw("     "),
                    Span::styled(preview, Style::default().fg(Color::Rgb(180, 180, 190))),
                ])]
            };

            let is_selected = selected_idx == Some(list_pos);
            if is_selected && expand_lines > 0 {
                let mut lines = vec![line1];

                // Show expanded messages in chronological order, then the preview at the bottom
                let msg_count = s.messages.len();
                let skip = default_preview_count.max(1);
                let available = msg_count.saturating_sub(skip);
                let show = expand_lines.min(available);
                let start = msg_count.saturating_sub(skip + show);
                let end = msg_count.saturating_sub(skip);

                for i in start..end {
                    let turn = &s.messages[i];
                    let is_assistant = turn.role == "assistant";
                    let role_label = if is_assistant { "  claude " } else { "  you    " };
                    let role_color = if is_assistant {
                        Color::Rgb(180, 130, 255) // purple for assistant
                    } else {
                        Color::Rgb(100, 180, 100) // green for user
                    };
                    let text_color = if is_assistant {
                        Color::Rgb(140, 140, 160)
                    } else {
                        Color::Rgb(170, 170, 185)
                    };

                    let label_len = role_label.len();
                    let msg_lines = wrap_text(
                        &turn.text,
                        w.saturating_sub(2),
                        label_len,
                        Style::default().fg(text_color),
                    );
                    if let Some(_first) = msg_lines.first() {
                        let mut first_spans = vec![Span::styled(
                            role_label.to_string(),
                            Style::default().fg(role_color),
                        )];
                        let text_part: String =
                            turn.text.chars().take(w.saturating_sub(label_len + 2)).collect();
                        first_spans.push(Span::styled(
                            text_part,
                            Style::default().fg(text_color),
                        ));
                        lines.push(Line::from(first_spans));
                        for wrap_line in msg_lines.iter().skip(1) {
                            lines.push(wrap_line.clone());
                        }
                    }
                }

                // Preview (latest messages) at the bottom
                lines.extend(preview_lines);
                lines.push(Line::raw(""));
                ListItem::new(lines)
            } else {
                let mut lines = vec![line1];
                lines.extend(preview_lines);
                lines.push(Line::raw(""));
                ListItem::new(lines)
            }
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().style(Style::default().bg(Color::Rgb(30, 30, 40))))
        .highlight_style(Style::default().bg(Color::Rgb(50, 50, 70)))
        .highlight_symbol("▸ ");

    f.render_stateful_widget(list, area, list_state);
}

pub fn draw_sessions(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let rows: Vec<SessionRow> = app
        .session_filtered
        .iter()
        .map(|&idx| {
            let s = &app.sessions[idx];
            let project_display = short_project(&s.project);
            let cwd_display = s.last_cwd.as_deref().map(short_project);
            let active_marker = match &s.active {
                Some(info) => {
                    let ws = info.workspace.as_deref().unwrap_or("");
                    if info.in_tmux {
                        if ws.is_empty() { "T".to_string() } else { format!("T{}", ws) }
                    } else if ws.is_empty() {
                        "?".to_string()
                    } else {
                        ws.to_string()
                    }
                }
                None => String::new(),
            };
            SessionRow {
                last_ts: s.last_ts,
                project_display,
                cwd_display,
                active_marker,
                last_msg: &s.last_msg,
                msg_count: s.msg_count,
                messages: &s.messages,
            }
        })
        .collect();

    draw_session_list(
        f,
        area,
        rows,
        app.session_state.selected(),
        app.expand_lines,
        &mut app.session_state.clone(),
    );
}

fn draw_remote_hosts(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut items: Vec<ListItem> = app
        .remote_hosts
        .iter()
        .map(|h| {
            let mut spans = vec![
                Span::styled("  ", Style::default().fg(Color::Magenta)),
                Span::styled(
                    format!("{:<20} ", h.name),
                    Style::default().fg(Color::Magenta),
                ),
                Span::styled(
                    format!("(ssh {})", h.ssh),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if h.gpu {
                spans.push(Span::styled("  GPU", Style::default().fg(Color::Green)));
            }
            ListItem::new(Line::from(spans))
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

    let rows: Vec<SessionRow> = app
        .remote_sessions
        .iter()
        .map(|s| {
            let project_display = s.project.clone();
            let cwd_display = s.last_cwd.clone();
            let active_marker = match s.active_pid {
                Some(_) => if s.in_tmux { "T".to_string() } else { "N".to_string() },
                None => String::new(),
            };
            SessionRow {
                last_ts: s.last_ts,
                project_display,
                cwd_display,
                active_marker,
                last_msg: &s.last_msg,
                msg_count: s.msg_count,
                messages: &s.messages,
            }
        })
        .collect();

    draw_session_list(
        f,
        area,
        rows,
        app.remote_session_state.selected(),
        app.expand_lines,
        &mut app.remote_session_state.clone(),
    );
}

fn draw_new_session(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.dir_filtered.is_empty() {
        let text = if let Some(ref err) = app.remote_error {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("Error: {}", err), Style::default().fg(Color::Red)),
            ])
        } else {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if app.dir_query.is_empty() {
                        "No directories found"
                    } else {
                        "No matching directories"
                    },
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        };
        let msg = Paragraph::new(text)
            .block(Block::default().style(Style::default().bg(Color::Rgb(30, 30, 40))));
        f.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .dir_filtered
        .iter()
        .map(|&idx| {
            let d = &app.dir_list[idx];
            let icon = if d.has_claude_md {
                Span::styled("  ", Style::default().fg(Color::Cyan))
            } else if d.has_git {
                Span::styled("  ", Style::default().fg(Color::Yellow))
            } else {
                Span::styled("  ", Style::default().fg(Color::DarkGray))
            };

            let path_color = if d.has_claude_md {
                Color::Cyan
            } else {
                Color::Rgb(100, 160, 220)
            };

            ListItem::new(Line::from(vec![
                icon,
                Span::styled(&d.display, Style::default().fg(path_color)),
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

    f.render_stateful_widget(list, area, &mut app.dir_state.clone());
}
