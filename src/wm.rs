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
