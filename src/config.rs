use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MAX_RECENT: usize = 25;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecentWorkspace {
    pub path: String,
    pub mode: String,
    pub last_opened: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppConfig {
    #[serde(skip)]
    pub file_path: PathBuf,
    #[serde(default)]
    pub recent: Vec<RecentWorkspace>,
    #[serde(default)]
    pub last_workspaces: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            file_path: default_config_path(),
            recent: Vec::new(),
            last_workspaces: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        Self::load_from(default_config_path())
    }

    pub fn load_from(file_path: PathBuf) -> Self {
        let mut config = if let Ok(content) = fs::read_to_string(&file_path) {
            toml::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        };
        config.file_path = file_path;
        config
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(&self.file_path, content)?;
        Ok(())
    }

    pub fn add_recent(&mut self, path: String, mode: String) {
        self.recent.retain(|r| r.path != path);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.recent.insert(
            0,
            RecentWorkspace {
                path,
                mode,
                last_opened: now,
            },
        );

        self.recent.truncate(MAX_RECENT);
    }
}

fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/mdlive/config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert!(config.recent.is_empty());
    }

    #[test]
    fn test_add_recent_basic() {
        let mut config = AppConfig::default();
        config.add_recent("/tmp/foo".into(), "directory".into());
        assert_eq!(config.recent.len(), 1);
        assert_eq!(config.recent[0].path, "/tmp/foo");
        assert_eq!(config.recent[0].mode, "directory");
    }

    #[test]
    fn test_add_recent_deduplicates() {
        let mut config = AppConfig::default();
        config.add_recent("/tmp/foo".into(), "directory".into());
        config.add_recent("/tmp/bar".into(), "file".into());
        config.add_recent("/tmp/foo".into(), "directory".into());
        assert_eq!(config.recent.len(), 2);
        assert_eq!(config.recent[0].path, "/tmp/foo");
        assert_eq!(config.recent[1].path, "/tmp/bar");
    }

    #[test]
    fn test_add_recent_fifo_max() {
        let mut config = AppConfig::default();
        for i in 0..30 {
            config.add_recent(format!("/tmp/dir{i}"), "directory".into());
        }
        assert_eq!(config.recent.len(), MAX_RECENT);
        assert_eq!(config.recent[0].path, "/tmp/dir29");
        assert_eq!(config.recent[MAX_RECENT - 1].path, "/tmp/dir5");
    }

    #[test]
    fn test_save_and_load() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mdlive/config.toml");

        let mut config = AppConfig::default();
        config.add_recent("/tmp/test".into(), "directory".into());

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = toml::to_string_pretty(&config).unwrap();
        fs::write(&path, &content).unwrap();

        let loaded: AppConfig = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.recent.len(), 1);
        assert_eq!(loaded.recent[0].path, "/tmp/test");
    }

    #[test]
    fn test_roundtrip_toml() {
        let mut config = AppConfig::default();
        config.add_recent("/Users/bryan/dev".into(), "directory".into());
        config.add_recent("/Users/bryan/notes.md".into(), "file".into());

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: AppConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.recent.len(), 2);
        assert_eq!(deserialized.recent[0].path, "/Users/bryan/notes.md");
        assert_eq!(deserialized.recent[1].path, "/Users/bryan/dev");
    }
}
