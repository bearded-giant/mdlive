use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use tokio::sync::{broadcast, Mutex};
use tokio::task::AbortHandle;

use crate::config::AppConfig;
use crate::util::is_markdown_file;

pub(crate) type SharedMarkdownState = Arc<Mutex<MarkdownState>>;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum ServerMessage {
    Reload,
    Pong,
    FileMoved {
        from: String,
        to: String,
    },
    WorkspaceChanged {
        base_dir: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
}

pub(crate) struct TrackedFile {
    pub(crate) path: PathBuf,
    pub(crate) last_modified: SystemTime,
    pub(crate) created: SystemTime,
    pub(crate) html: String,
}

pub(crate) struct FileInfo {
    pub(crate) name: String,
    pub(crate) modified: u64,
    pub(crate) created: u64,
}

pub(crate) struct MarkdownState {
    pub(crate) base_dir: PathBuf,
    pub(crate) tracked_files: HashMap<String, TrackedFile>,
    pub(crate) is_directory_mode: bool,
    pub(crate) change_tx: broadcast::Sender<ServerMessage>,
    pub(crate) mdlive_dir: Option<PathBuf>,
    pub(crate) daemon_mode: bool,
    pub(crate) watcher_abort: Option<AbortHandle>,
    pub(crate) config: Option<AppConfig>,
}

impl MarkdownState {
    pub(crate) fn new(
        base_dir: PathBuf,
        file_paths: Vec<PathBuf>,
        is_directory_mode: bool,
    ) -> Result<Self> {
        let (change_tx, _) = broadcast::channel::<ServerMessage>(16);

        let mut tracked_files = HashMap::new();
        for file_path in file_paths {
            let metadata = fs::metadata(&file_path)?;
            let last_modified = metadata.modified()?;
            let created = metadata.created().unwrap_or(last_modified);
            let content = fs::read_to_string(&file_path)?;
            let html = Self::render_file_to_html(&file_path, &content)?;

            let canonical = file_path.canonicalize().unwrap_or(file_path);
            let key = canonical
                .strip_prefix(&base_dir)
                .unwrap_or(&canonical)
                .to_string_lossy()
                .to_string();

            tracked_files.insert(
                key,
                TrackedFile {
                    path: canonical,
                    last_modified,
                    created,
                    html,
                },
            );
        }

        let dir = base_dir.join(".mdlive");
        let history = dir.join("history");
        let _ = fs::create_dir_all(&history);
        let mdlive_dir = Some(dir);

        Ok(MarkdownState {
            base_dir,
            tracked_files,
            is_directory_mode,
            change_tx,
            mdlive_dir,
            daemon_mode: false,
            watcher_abort: None,
            config: None,
        })
    }

    pub(crate) fn new_daemon() -> Self {
        let (change_tx, _) = broadcast::channel::<ServerMessage>(16);
        MarkdownState {
            base_dir: PathBuf::new(),
            tracked_files: HashMap::new(),
            is_directory_mode: false,
            change_tx,
            mdlive_dir: None,
            daemon_mode: true,
            watcher_abort: None,
            config: Some(AppConfig::load()),
        }
    }

    pub(crate) fn new_daemon_with_config(config: AppConfig) -> Self {
        let (change_tx, _) = broadcast::channel::<ServerMessage>(16);
        MarkdownState {
            base_dir: PathBuf::new(),
            tracked_files: HashMap::new(),
            is_directory_mode: false,
            change_tx,
            mdlive_dir: None,
            daemon_mode: true,
            watcher_abort: None,
            config: Some(config),
        }
    }

    pub(crate) fn has_workspace(&self) -> bool {
        !self.base_dir.as_os_str().is_empty()
    }

    pub(crate) fn switch_workspace(
        &mut self,
        new_dir: PathBuf,
        files: Vec<PathBuf>,
        dir_mode: bool,
        target_file: Option<String>,
        original_path: Option<String>,
    ) -> Result<()> {
        if let Some(handle) = self.watcher_abort.take() {
            handle.abort();
        }

        self.base_dir = new_dir;
        self.is_directory_mode = dir_mode;
        self.tracked_files.clear();

        for file_path in files {
            let metadata = fs::metadata(&file_path)?;
            let last_modified = metadata.modified()?;
            let created = metadata.created().unwrap_or(last_modified);
            let content = fs::read_to_string(&file_path)?;
            let html = Self::render_file_to_html(&file_path, &content)?;

            let canonical = file_path.canonicalize().unwrap_or(file_path);
            let key = canonical
                .strip_prefix(&self.base_dir)
                .unwrap_or(&canonical)
                .to_string_lossy()
                .to_string();

            self.tracked_files.insert(
                key,
                TrackedFile {
                    path: canonical,
                    last_modified,
                    created,
                    html,
                },
            );
        }

        let dir = self.base_dir.join(".mdlive");
        let history = dir.join("history");
        let _ = fs::create_dir_all(&history);
        self.mdlive_dir = Some(dir);

        let mode = if dir_mode { "directory" } else { "file" };
        let recent_path = if dir_mode {
            self.base_dir.display().to_string()
        } else {
            original_path.unwrap_or_else(|| self.base_dir.display().to_string())
        };
        if let Some(ref mut config) = self.config {
            config.add_recent(recent_path, mode.to_string());
            let _ = config.save();
        }

        let rx_count = self.change_tx.receiver_count();
        eprintln!(
            "[state] workspace switched to: {} ({rx_count} ws receivers)",
            self.base_dir.display()
        );
        let _ = self.change_tx.send(ServerMessage::WorkspaceChanged {
            base_dir: self.base_dir.display().to_string(),
            file: target_file,
        });

        Ok(())
    }

    pub(crate) fn show_navigation(&self) -> bool {
        self.is_directory_mode
    }

    pub(crate) fn get_sorted_filenames(&self) -> Vec<String> {
        let mut filenames: Vec<_> = self.tracked_files.keys().cloned().collect();
        filenames.sort();
        filenames
    }

    pub(crate) fn get_file_infos(&self) -> Vec<FileInfo> {
        fn to_epoch(t: SystemTime) -> u64 {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        }
        let mut infos: Vec<FileInfo> = self
            .tracked_files
            .iter()
            .map(|(name, tf)| {
                // restat disk so sidebar reflects edits the watcher may have missed
                // or that happened before this file was opened in mdlive
                let (modified, created) = match fs::metadata(&tf.path) {
                    Ok(meta) => {
                        let m = meta.modified().unwrap_or(tf.last_modified);
                        let c = meta.created().unwrap_or(tf.created);
                        (to_epoch(m), to_epoch(c))
                    }
                    Err(_) => (to_epoch(tf.last_modified), to_epoch(tf.created)),
                };
                FileInfo {
                    name: name.clone(),
                    modified,
                    created,
                }
            })
            .collect();
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        infos
    }

    pub(crate) fn refresh_file(&mut self, filename: &str) -> Result<()> {
        if let Some(tracked) = self.tracked_files.get_mut(filename) {
            let metadata = fs::metadata(&tracked.path)?;
            let current_modified = metadata.modified()?;

            if current_modified > tracked.last_modified {
                let content = fs::read_to_string(&tracked.path)?;
                tracked.html = Self::render_file_to_html(&tracked.path, &content)?;
                tracked.last_modified = current_modified;
            }
        }

        Ok(())
    }

    pub(crate) fn add_tracked_file(&mut self, file_path: PathBuf) -> Result<()> {
        let key = file_path
            .strip_prefix(&self.base_dir)
            .unwrap_or(&file_path)
            .to_string_lossy()
            .to_string();

        if self.tracked_files.contains_key(&key) {
            return Ok(());
        }

        let metadata = fs::metadata(&file_path)?;
        let last_modified = metadata.modified()?;
        let created = metadata.created().unwrap_or(last_modified);
        let content = fs::read_to_string(&file_path)?;

        self.tracked_files.insert(
            key,
            TrackedFile {
                path: file_path.clone(),
                last_modified,
                created,
                html: Self::render_file_to_html(&file_path, &content)?,
            },
        );

        Ok(())
    }

    pub(crate) fn remove_tracked_file(&mut self, key: &str) -> bool {
        self.tracked_files.remove(key).is_some()
    }

    /// Rewrite every tracked key that equals `from` or lives under `from/` to
    /// point at the equivalent location under `to`. Used when a directory is
    /// moved so every file inside stays tracked without rescanning.
    pub(crate) fn move_tracked_prefix(&mut self, from: &str, to: &str) {
        let from_prefix = format!("{from}/");
        let to_prefix = format!("{to}/");
        let keys: Vec<String> = self.tracked_files.keys().cloned().collect();
        for key in keys {
            let new_key = if key == from {
                to.to_string()
            } else if key.starts_with(&from_prefix) {
                format!("{to_prefix}{}", &key[from_prefix.len()..])
            } else {
                continue;
            };
            if let Some(mut tf) = self.tracked_files.remove(&key) {
                let new_path = self.base_dir.join(&new_key);
                tf.path = new_path.canonicalize().unwrap_or(new_path);
                self.tracked_files.insert(new_key, tf);
            }
        }
    }

    /// Drop every tracked entry that equals `prefix` or lives under `prefix/`.
    /// Used when a directory is deleted.
    pub(crate) fn remove_tracked_prefix(&mut self, prefix: &str) -> usize {
        let prefix_slash = format!("{prefix}/");
        let keys: Vec<String> = self
            .tracked_files
            .keys()
            .filter(|k| k.as_str() == prefix || k.starts_with(&prefix_slash))
            .cloned()
            .collect();
        let count = keys.len();
        for key in keys {
            self.tracked_files.remove(&key);
        }
        count
    }

    pub(crate) fn render_file_to_html(path: &Path, content: &str) -> Result<String> {
        if is_markdown_file(path) {
            Self::markdown_to_html(content)
        } else {
            Ok(Self::text_to_html(path, content))
        }
    }

    pub(crate) fn markdown_to_html(content: &str) -> Result<String> {
        let content = content.to_string();
        let result = std::panic::catch_unwind(|| {
            let mut options = markdown::Options::gfm();
            options.compile.allow_dangerous_html = true;
            options.parse.constructs.frontmatter = true;
            markdown::to_html_with_options(&content, &options)
                .unwrap_or_else(|_| "Error parsing markdown".to_string())
        });

        match result {
            Ok(html) => Ok(html),
            Err(_) => Ok("<p><em>Could not render this file (markdown parser error)</em></p>".into()),
        }
    }

    fn text_to_html(path: &Path, content: &str) -> String {
        let lang = if crate::util::is_json_file(path) {
            "json"
        } else {
            "plaintext"
        };
        let escaped = content
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        format!("<pre><code class=\"language-{lang}\">{escaped}</code></pre>")
    }
}
