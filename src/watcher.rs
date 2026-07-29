use anyhow::Result;
use notify::event::RemoveKind;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::state::{ServerMessage, SharedMarkdownState};
use crate::util::{is_image_file, is_supported_file, scan_supported_files};

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
                    if new_path.is_dir() {
                        handle_dir_move(&event.paths[0], new_path, state).await;
                    } else {
                        handle_file_change(new_path, state).await;
                    }
                }
                RenameMode::From => {
                    if let Some(path) = event.paths.first() {
                        schedule_deferred_remove(path.to_path_buf(), state.clone());
                    }
                }
                RenameMode::To => {
                    if let Some(path) = event.paths.first() {
                        if path.is_dir() {
                            handle_dir_add(path, state).await;
                        } else {
                            handle_file_change(path, state).await;
                        }
                    }
                }
                RenameMode::Any => {
                    if let Some(path) = event.paths.first() {
                        if path.is_dir() {
                            handle_dir_add(path, state).await;
                        } else if path.exists() {
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
                } else {
                    handle_dir_event(&event.kind, path, state).await;
                }
            }
        }
    }
}

fn rel_key(base_dir: &Path, path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical
        .strip_prefix(base_dir)
        .unwrap_or(&canonical)
        .to_string_lossy()
        .to_string()
}

// the sidebar tree is built from tracked files plus a live directory scan, so a
// directory appearing/vanishing changes the tree without any file event firing
async fn handle_dir_event(kind: &notify::EventKind, path: &Path, state: &SharedMarkdownState) {
    match kind {
        notify::EventKind::Create(_) if path.is_dir() => handle_dir_add(path, state).await,
        notify::EventKind::Remove(_) if !path.exists() => {
            let mut state_guard = state.lock().await;
            if !state_guard.is_directory_mode {
                return;
            }
            let key = rel_key(&state_guard.base_dir, path);
            let dropped = state_guard.remove_tracked_prefix(&key);
            let was_dir = matches!(kind, notify::EventKind::Remove(RemoveKind::Folder));
            if dropped > 0 || was_dir {
                let _ = state_guard.change_tx.send(ServerMessage::Reload);
            }
        }
        _ => {}
    }
}

async fn handle_dir_add(path: &Path, state: &SharedMarkdownState) {
    let mut state_guard = state.lock().await;
    if !state_guard.is_directory_mode {
        return;
    }
    // a directory can arrive already populated (git checkout, mv) without a
    // per-file event for each child
    if let Ok(files) = scan_supported_files(path) {
        for file in files {
            let canonical = file.canonicalize().unwrap_or(file);
            let _ = state_guard.add_tracked_file(canonical);
        }
    }
    let _ = state_guard.change_tx.send(ServerMessage::Reload);
}

async fn handle_dir_move(from: &Path, to: &Path, state: &SharedMarkdownState) {
    let mut state_guard = state.lock().await;
    if !state_guard.is_directory_mode {
        return;
    }
    let from_key = rel_key(&state_guard.base_dir, from);
    let to_key = rel_key(&state_guard.base_dir, to);
    state_guard.move_tracked_prefix(&from_key, &to_key);
    let _ = state_guard.change_tx.send(ServerMessage::Reload);
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
        // prefix removal covers a directory renamed or moved out of the workspace
        if state_guard.remove_tracked_file(&key) || state_guard.remove_tracked_prefix(&key) > 0 {
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
