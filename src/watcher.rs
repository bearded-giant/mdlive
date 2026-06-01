use anyhow::Result;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::state::{ServerMessage, SharedMarkdownState};
use crate::util::{is_image_file, is_supported_file};

pub(crate) fn start_watcher(base_dir: &Path, state: SharedMarkdownState) -> Result<AbortHandle> {
    let (tx, mut rx) = mpsc::channel(100);

    let mut watcher = RecommendedWatcher::new(
        move |res: std::result::Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        },
        Config::default(),
    )?;

    watcher.watch(base_dir, RecursiveMode::Recursive)?;

    let handle = tokio::spawn(async move {
        let _watcher = watcher;
        while let Some(event) = rx.recv().await {
            handle_file_event(event, &state).await;
        }
    });

    Ok(handle.abort_handle())
}

fn is_mdlive_path(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == ".mdlive")
}

pub(crate) async fn handle_file_event(event: Event, state: &SharedMarkdownState) {
    if event.paths.iter().any(|p| is_mdlive_path(p)) {
        return;
    }
    match event.kind {
        notify::EventKind::Modify(notify::event::ModifyKind::Name(rename_mode)) => {
            use notify::event::RenameMode;
            match rename_mode {
                RenameMode::Both if event.paths.len() == 2 => {
                    let new_path = &event.paths[1];
                    handle_file_change(new_path, state).await;
                }
                RenameMode::From => {
                    if let Some(path) = event.paths.first() {
                        schedule_deferred_remove(path.to_path_buf(), state.clone());
                    }
                }
                RenameMode::To => {
                    if let Some(path) = event.paths.first() {
                        handle_file_change(path, state).await;
                    }
                }
                RenameMode::Any => {
                    if let Some(path) = event.paths.first() {
                        if path.exists() {
                            handle_file_change(path, state).await;
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {
            for path in &event.paths {
                if is_supported_file(path) {
                    match event.kind {
                        notify::EventKind::Create(_)
                        | notify::EventKind::Modify(notify::event::ModifyKind::Data(_)) => {
                            handle_file_change(path, state).await;
                        }
                        notify::EventKind::Remove(_) => {
                            // editors like neovim save by renaming to a backup then creating
                            // a new file -- defer removal so atomic-save patterns don't
                            // untrack a still-live file.
                            schedule_deferred_remove(path.to_path_buf(), state.clone());
                        }
                        _ => {}
                    }
                } else if path.is_file() && is_image_file(path.to_str().unwrap_or("")) {
                    match event.kind {
                        notify::EventKind::Modify(_)
                        | notify::EventKind::Create(_)
                        | notify::EventKind::Remove(_) => {
                            let state_guard = state.lock().await;
                            let _ = state_guard.change_tx.send(ServerMessage::Reload);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn schedule_deferred_remove(path: std::path::PathBuf, state: SharedMarkdownState) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if path.exists() {
            return;
        }
        let mut state_guard = state.lock().await;
        let key = path
            .strip_prefix(&state_guard.base_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        if state_guard.remove_tracked_file(&key) {
            let _ = state_guard.change_tx.send(ServerMessage::Reload);
        }
    });
}

async fn handle_file_change(path: &Path, state: &SharedMarkdownState) {
    if !is_supported_file(path) {
        return;
    }

    let mut state_guard = state.lock().await;

    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let key = canonical
        .strip_prefix(&state_guard.base_dir)
        .unwrap_or(&canonical)
        .to_string_lossy()
        .to_string();

    if state_guard.tracked_files.contains_key(&key) {
        if state_guard.refresh_file(&key).is_ok() {
            let _ = state_guard.change_tx.send(ServerMessage::Reload);
        }
    } else if state_guard.is_directory_mode && state_guard.add_tracked_file(canonical).is_ok() {
        let _ = state_guard.change_tx.send(ServerMessage::Reload);
    }
}
