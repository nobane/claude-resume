use std::process::Command;

/// A window known to the window manager.
pub struct WmWindow {
    pub pid: u32,
    pub workspace: String,
    /// Opaque identifier used to focus this window (hyprland address or i3 con_id).
    pub id: String,
    /// Window title (i3 only, from the tree node name).
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wm {
    Hyprland,
    I3,
    Unknown,
}

/// Detect which window manager is running.
pub fn detect() -> Wm {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        return Wm::Hyprland;
    }
    if std::env::var("I3SOCK").is_ok() || std::env::var("SWAYSOCK").is_ok() {
        return Wm::I3;
    }
    // Fallback: try commands
    if Command::new("hyprctl")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Wm::Hyprland;
    }
    if Command::new("i3-msg")
        .args(["-t", "get_version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Wm::I3;
    }
    Wm::Unknown
}

/// List all windows with their PIDs, workspaces, and focusable IDs.
pub fn list_windows(wm: Wm) -> Vec<WmWindow> {
    match wm {
        Wm::Hyprland => list_hyprland(),
        Wm::I3 => list_i3(),
        Wm::Unknown => vec![],
    }
}

/// Focus a window by its opaque ID.
pub fn focus_window(wm: Wm, id: &str) {
    match wm {
        Wm::Hyprland => {
            let _ = Command::new("hyprctl")
                .args(["dispatch", "focuswindow", &format!("address:{}", id)])
                .output();
        }
        Wm::I3 => {
            let _ = Command::new("i3-msg")
                .args(&[&format!("[con_id={}] focus", id)])
                .output();
        }
        Wm::Unknown => {}
    }
}

// --- Hyprland ---

#[derive(serde::Deserialize)]
struct HyprClient {
    pid: i64,
    workspace: HyprWorkspace,
    address: String,
}

#[derive(serde::Deserialize)]
struct HyprWorkspace {
    name: String,
}

fn list_hyprland() -> Vec<WmWindow> {
    let output = match Command::new("hyprctl").args(["clients", "-j"]).output() {
        Ok(o) => o,
        Err(_) => return vec![],
    };
    let clients: Vec<HyprClient> = serde_json::from_slice(&output.stdout).unwrap_or_default();
    clients
        .into_iter()
        .filter(|c| c.pid > 0)
        .map(|c| WmWindow {
            pid: c.pid as u32,
            workspace: c.workspace.name,
            id: c.address,
            title: None,
        })
        .collect()
}

// --- i3 ---

#[derive(serde::Deserialize)]
struct I3Node {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "type", default)]
    node_type: Option<String>,
    /// X11 window ID (non-null for actual windows).
    #[serde(default)]
    window: Option<u64>,
    #[serde(default)]
    nodes: Vec<I3Node>,
    #[serde(default)]
    floating_nodes: Vec<I3Node>,
}

/// An i3 window before PID resolution.
struct I3Window {
    con_id: u64,
    x11_window: u64,
    workspace: String,
    title: String,
}

fn list_i3() -> Vec<WmWindow> {
    let output = match Command::new("i3-msg").args(["-t", "get_tree"]).output() {
        Ok(o) => o,
        Err(_) => return vec![],
    };
    let root: I3Node = match serde_json::from_slice(&output.stdout) {
        Ok(n) => n,
        Err(_) => return vec![],
    };
    let mut i3_windows = Vec::new();
    collect_i3_windows(&root, None, &mut i3_windows);

    // Resolve PIDs from X11 window IDs via xdotool
    i3_windows
        .into_iter()
        .filter_map(|w| {
            let pid = get_window_pid(w.x11_window)?;
            Some(WmWindow {
                pid,
                workspace: w.workspace,
                id: w.con_id.to_string(),
                title: Some(w.title),
            })
        })
        .collect()
}

/// Get the PID of a process owning an X11 window via xdotool.
fn get_window_pid(x11_window: u64) -> Option<u32> {
    let output = Command::new("xdotool")
        .args(["getwindowpid", &x11_window.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    s.trim().parse().ok()
}

fn collect_i3_windows(node: &I3Node, workspace: Option<&str>, windows: &mut Vec<I3Window>) {
    let ws = if node.node_type.as_deref() == Some("workspace") {
        node.name.as_deref()
    } else {
        workspace
    };

    // Node with a window ID = an X11 window
    if let Some(x11_win) = node.window {
        windows.push(I3Window {
            con_id: node.id,
            x11_window: x11_win,
            workspace: ws.unwrap_or("?").to_string(),
            title: node.name.clone().unwrap_or_default(),
        });
    }

    for child in &node.nodes {
        collect_i3_windows(child, ws, windows);
    }
    for child in &node.floating_nodes {
        collect_i3_windows(child, ws, windows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_returns_without_panic() {
        // Should never panic regardless of environment
        let wm = detect();
        // Just verify it returns a valid enum variant
        let _ = match wm {
            Wm::Hyprland => "hyprland",
            Wm::I3 => "i3",
            Wm::Unknown => "unknown",
        };
    }

    #[test]
    fn test_list_windows_unknown() {
        let windows = list_windows(Wm::Unknown);
        assert!(windows.is_empty());
    }

    #[test]
    fn test_focus_window_unknown_noop() {
        // Should not panic
        focus_window(Wm::Unknown, "some-id");
    }

    #[test]
    fn test_collect_i3_windows_empty_tree() {
        let root = I3Node {
            id: 0,
            name: Some("root".into()),
            node_type: Some("root".into()),
            window: None,
            nodes: vec![],
            floating_nodes: vec![],
        };
        let mut windows = Vec::new();
        collect_i3_windows(&root, None, &mut windows);
        assert!(windows.is_empty());
    }

    #[test]
    fn test_collect_i3_windows_with_workspace() {
        let root = I3Node {
            id: 1,
            name: Some("root".into()),
            node_type: Some("root".into()),
            window: None,
            nodes: vec![I3Node {
                id: 2,
                name: Some("1".into()),
                node_type: Some("workspace".into()),
                window: None,
                nodes: vec![I3Node {
                    id: 3,
                    name: Some("terminal".into()),
                    node_type: Some("con".into()),
                    window: Some(12345),
                    nodes: vec![],
                    floating_nodes: vec![],
                }],
                floating_nodes: vec![],
            }],
            floating_nodes: vec![],
        };
        let mut windows = Vec::new();
        collect_i3_windows(&root, None, &mut windows);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].workspace, "1");
        assert_eq!(windows[0].x11_window, 12345);
        assert_eq!(windows[0].con_id, 3);
        assert_eq!(windows[0].title, "terminal");
    }

    #[test]
    fn test_parse_hyprland_clients() {
        let json = r#"[
            {"pid": 1234, "workspace": {"name": "1"}, "address": "0x1234"},
            {"pid": -1, "workspace": {"name": "2"}, "address": "0x5678"},
            {"pid": 5678, "workspace": {"name": "3"}, "address": "0xabcd"}
        ]"#;
        let clients: Vec<HyprClient> = serde_json::from_str(json).unwrap();
        let windows: Vec<WmWindow> = clients
            .into_iter()
            .filter(|c| c.pid > 0)
            .map(|c| WmWindow {
                pid: c.pid as u32,
                workspace: c.workspace.name,
                id: c.address,
                title: None,
            })
            .collect();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].pid, 1234);
        assert_eq!(windows[1].pid, 5678);
    }
}
