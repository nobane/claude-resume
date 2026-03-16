use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct HostConfig {
    pub name: String,
    pub ssh: String,
    #[serde(default)]
    pub gpu: bool,
}

#[derive(Deserialize)]
struct HostsFile {
    #[serde(default)]
    host: Vec<HostConfig>,
}

pub fn load_hosts() -> Vec<HostConfig> {
    let path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/claude-resume/hosts.toml");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    parse_hosts_toml(&content)
}

pub fn parse_hosts_toml(content: &str) -> Vec<HostConfig> {
    match toml::from_str::<HostsFile>(content) {
        Ok(f) => f.host,
        Err(_) => vec![],
    }
}

pub fn recent_dirs_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/claude-resume/recent-dirs.json")
}

pub fn load_recent_dirs() -> Vec<String> {
    let path = recent_dirs_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_recent_dirs(dirs: &[String]) {
    let path = recent_dirs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let capped: Vec<&String> = dirs.iter().take(50).collect();
    if let Ok(json) = serde_json::to_string(&capped) {
        let _ = std::fs::write(&path, json);
    }
}

pub fn add_recent_dir(dir: &str) {
    let mut dirs = load_recent_dirs();
    dirs.retain(|d| d != dir);
    dirs.insert(0, dir.to_string());
    dirs.truncate(50);
    save_recent_dirs(&dirs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_hosts_empty() {
        // When the config file doesn't exist, we get an empty vec
        let hosts = load_hosts();
        // This may or may not be empty depending on whether the user has a config file,
        // but it should not panic
        let _ = hosts;
    }

    #[test]
    fn test_parse_hosts_toml() {
        let toml = r#"
[[host]]
name = "server1"
ssh = "user@server1.example.com"

[[host]]
name = "server2"
ssh = "admin@10.0.0.1"
"#;
        let hosts = parse_hosts_toml(toml);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].name, "server1");
        assert_eq!(hosts[0].ssh, "user@server1.example.com");
        assert_eq!(hosts[1].name, "server2");
        assert_eq!(hosts[1].ssh, "admin@10.0.0.1");
    }

    #[test]
    fn test_parse_hosts_toml_empty() {
        let hosts = parse_hosts_toml("");
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_parse_hosts_toml_invalid() {
        let hosts = parse_hosts_toml("not valid toml {{{{");
        assert!(hosts.is_empty());
    }
}
