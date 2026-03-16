use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, View};
use crate::session::{format_time_ago, short_project};

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
        View::NewSession | View::NewRemoteSession => draw_new_session(f, app, chunks[1]),
    }

    let footer_text = if matches!(app.view, View::NewSession | View::NewRemoteSession) {
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
        if matches!(
            app.view,
            View::FolderSessions | View::AllSessions | View::RemoteSessions
        ) {
            hints.push(Span::styled("l/h", Style::default().fg(Color::Green)));
            hints.push(Span::raw(" expand  "));
        }
        hints.push(Span::styled("a", Style::default().fg(Color::Green)));
        hints.push(Span::raw("ll "));
        hints.push(Span::styled("f", Style::default().fg(Color::Green)));
        hints.push(Span::raw("olders "));
        hints.push(Span::styled("r", Style::default().fg(Color::Green)));
        hints.push(Span::raw("emote  "));
        hints.push(Span::styled("n", Style::default().fg(Color::Green)));
        hints.push(Span::raw("ew  "));
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

pub fn draw_sessions(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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
                Span::styled(format!(" ●{}", ws), Style::default().fg(Color::Green))
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
                    let msg_lines = wrap_text(
                        msg,
                        w.saturating_sub(2),
                        label_len,
                        Style::default().fg(Color::Rgb(150, 150, 165)),
                    );
                    if let Some(_first) = msg_lines.first() {
                        let mut first_spans = vec![Span::styled(
                            label.clone(),
                            Style::default().fg(Color::Rgb(70, 70, 90)),
                        )];
                        let text_part: String =
                            msg.chars().take(w.saturating_sub(label_len + 2)).collect();
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
        .highlight_style(Style::default().bg(Color::Rgb(50, 50, 70)))
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

            let cwd_display = s.last_cwd.as_deref().unwrap_or(&s.project);

            let active_marker = if let Some(pid) = s.active_pid {
                Span::styled(format!(" ●{}", pid), Style::default().fg(Color::Green))
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
                        let mut first_spans = vec![Span::styled(
                            label.clone(),
                            Style::default().fg(Color::Rgb(70, 70, 90)),
                        )];
                        let text_part: String =
                            msg.chars().take(w.saturating_sub(label_len + 2)).collect();
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
        .highlight_style(Style::default().bg(Color::Rgb(50, 50, 70)))
        .highlight_symbol("▸ ");

    f.render_stateful_widget(list, area, &mut app.remote_session_state.clone());
}

fn draw_new_session(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.dir_filtered.is_empty() {
        let msg = Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                if app.dir_query.is_empty() {
                    "No directories found"
                } else {
                    "No matching directories"
                },
                Style::default().fg(Color::DarkGray),
            ),
        ]))
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
