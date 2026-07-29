use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Redirect,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::task::AbortHandle;

use crate::state::SharedMarkdownState;
use crate::util::{is_supported_file, scan_supported_files};
use crate::watcher::start_watcher;

#[derive(Deserialize)]
pub(crate) struct SwitchRequest {
    path: String,
}

#[derive(Serialize)]
pub(crate) struct WorkspaceResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_count: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct RecentEntry {
    path: String,
    mode: String,
    last_opened: u64,
}

#[derive(Serialize)]
pub(crate) struct RecentResponse {
    success: bool,
    recent: Vec<RecentEntry>,
}

fn expand_path(raw: &str) -> PathBuf {
    let expanded = if let Some(rest) = raw.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            home.join(rest.strip_prefix('/').unwrap_or(rest))
        } else {
            PathBuf::from(raw)
        }
    } else {
        PathBuf::from(raw)
    };
    expanded
        .canonicalize()
        .unwrap_or_else(|_| std::path::absolute(&expanded).unwrap_or(expanded))
}

pub(crate) async fn api_workspace_switch(
    State(state): State<SharedMarkdownState>,
    Json(body): Json<SwitchRequest>,
) -> Result<Json<WorkspaceResponse>, (StatusCode, Json<WorkspaceResponse>)> {
    let target = expand_path(&body.path);
    let original_path = target.display().to_string();

    let (base_dir, files, dir_mode, target_file) = if target.is_file() {
        let base = target
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        let base = base.canonicalize().unwrap_or(base);
        let canonical = target.canonicalize().unwrap_or(target);
        let rel = canonical
            .strip_prefix(&base)
            .unwrap_or(std::path::Path::new(""))
            .to_string_lossy()
            .to_string();
        (base, vec![canonical], false, Some(rel))
    } else if target.is_dir() {
        let target = target.canonicalize().unwrap_or(target);
        let files = scan_supported_files(&target).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(WorkspaceResponse {
                    success: false,
                    error: Some(format!("failed to scan directory: {e}")),
                    base_dir: None,
                    mode: None,
                    file_count: None,
                }),
            )
        })?;
        if files.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(WorkspaceResponse {
                    success: false,
                    error: Some(
                        "no supported files found (.md, .txt, .json, .csv, .yaml, .yml, .toml, .mmd, .graphql, .gql)"
                            .to_string(),
                    ),
                    base_dir: None,
                    mode: None,
                    file_count: None,
                }),
            ));
        }
        (target, files, true, None)
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(WorkspaceResponse {
                success: false,
                error: Some("path does not exist".to_string()),
                base_dir: None,
                mode: None,
                file_count: None,
            }),
        ));
    };

    let file_count = files.len();
    let mode = if dir_mode { "directory" } else { "file" };
    let base_dir_display = base_dir.display().to_string();

    {
        let mut guard = state.lock().await;
        guard
            .switch_workspace(
                base_dir.clone(),
                files,
                dir_mode,
                target_file,
                Some(original_path),
            )
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(WorkspaceResponse {
                        success: false,
                        error: Some(format!("failed to switch workspace: {e}")),
                        base_dir: None,
                        mode: None,
                        file_count: None,
                    }),
                )
            })?;
    }

    let abort_handle: AbortHandle = start_watcher(&base_dir, state.clone()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WorkspaceResponse {
                success: false,
                error: Some(format!("failed to start file watcher: {e}")),
                base_dir: None,
                mode: None,
                file_count: None,
            }),
        )
    })?;

    {
        let mut guard = state.lock().await;
        guard.watcher_abort = Some(abort_handle);
    }

    Ok(Json(WorkspaceResponse {
        success: true,
        error: None,
        base_dir: Some(base_dir_display),
        mode: Some(mode.to_string()),
        file_count: Some(file_count),
    }))
}

pub(crate) async fn api_workspace_current(
    State(state): State<SharedMarkdownState>,
) -> Json<WorkspaceResponse> {
    let guard = state.lock().await;
    if guard.has_workspace() {
        let mode = if guard.is_directory_mode {
            "directory"
        } else {
            "file"
        };
        Json(WorkspaceResponse {
            success: true,
            error: None,
            base_dir: Some(guard.base_dir.display().to_string()),
            mode: Some(mode.to_string()),
            file_count: Some(guard.tracked_files.len()),
        })
    } else {
        Json(WorkspaceResponse {
            success: true,
            error: None,
            base_dir: None,
            mode: None,
            file_count: None,
        })
    }
}

pub(crate) async fn api_workspace_recent(
    State(state): State<SharedMarkdownState>,
) -> Json<RecentResponse> {
    let guard = state.lock().await;
    // reload off disk: other windows (each its own server) append recents too
    let recent = guard
        .config
        .as_ref()
        .map(|c| c.reload())
        .map(|c| {
            c.recent
                .iter()
                .map(|r| RecentEntry {
                    path: r.path.clone(),
                    mode: r.mode.clone(),
                    last_opened: r.last_opened,
                })
                .collect()
        })
        .unwrap_or_default();

    Json(RecentResponse {
        success: true,
        recent,
    })
}

#[derive(Deserialize)]
pub(crate) struct BrowseQuery {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    show_hidden: Option<bool>,
    #[serde(default)]
    sort: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct BrowseEntry {
    name: String,
    path: String,
    is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    modified: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct BrowseResponse {
    success: bool,
    path: String,
    parent: Option<String>,
    entries: Vec<BrowseEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub(crate) async fn api_workspace_browse(
    Query(params): Query<BrowseQuery>,
) -> Json<BrowseResponse> {
    let config = crate::AppConfig::load();
    let target = params
        .path
        .filter(|p| !p.trim().is_empty())
        .map(|p| expand_path(&p))
        .unwrap_or_else(|| {
            config
                .last_browse_dir
                .as_ref()
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
        });

    let target = if target.is_file() {
        target.parent().unwrap_or(&target).to_path_buf()
    } else {
        target
    };

    let display = target.display().to_string();
    let parent = target.parent().map(|p| p.display().to_string());

    {
        let mut config = crate::AppConfig::load();
        config.last_browse_dir = Some(display.clone());
        let _ = config.save();
    }

    let show_hidden = params.show_hidden.unwrap_or(false);
    let sort_mode = params.sort.as_deref().unwrap_or("name");

    let entries = match std::fs::read_dir(&target) {
        Ok(rd) => {
            let mut items: Vec<BrowseEntry> = rd
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if !show_hidden && name.starts_with('.') {
                        return None;
                    }
                    let ft = e.file_type().ok()?;
                    if ft.is_dir() {
                        // directories always shown
                    } else if ft.is_file() {
                        if !is_supported_file(&e.path()) {
                            return None;
                        }
                    } else {
                        return None;
                    }
                    let modified = e
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs());
                    Some(BrowseEntry {
                        path: e.path().display().to_string(),
                        is_dir: ft.is_dir(),
                        name,
                        modified,
                    })
                })
                .collect();
            match sort_mode {
                "modified" => {
                    items.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => b.modified.unwrap_or(0).cmp(&a.modified.unwrap_or(0)),
                    });
                }
                _ => {
                    items.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    });
                }
            }
            items
        }
        Err(e) => {
            return Json(BrowseResponse {
                success: false,
                path: display,
                parent,
                entries: vec![],
                error: Some(format!("cannot read directory: {e}")),
            });
        }
    };

    Json(BrowseResponse {
        success: true,
        path: display,
        parent,
        entries,
        error: None,
    })
}

#[derive(Deserialize)]
pub(crate) struct OpenQuery {
    path: String,
}

pub(crate) async fn open_and_redirect(
    Query(params): Query<OpenQuery>,
    State(state): State<SharedMarkdownState>,
) -> Redirect {
    let target = expand_path(&params.path);
    let original_path = target.display().to_string();

    let result = if target.is_file() {
        let base = target
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        let base = base.canonicalize().unwrap_or(base);
        let canonical = target.canonicalize().unwrap_or(target);
        let rel = canonical
            .strip_prefix(&base)
            .unwrap_or(std::path::Path::new(""))
            .to_string_lossy()
            .to_string();
        Some((base, vec![canonical], false, Some(rel)))
    } else if target.is_dir() {
        let target_canon = target.canonicalize().unwrap_or(target);
        scan_supported_files(&target_canon).ok().and_then(|files| {
            if files.is_empty() {
                None
            } else {
                Some((target_canon, files, true, None))
            }
        })
    } else {
        None
    };

    if let Some((base_dir, files, dir_mode, target_file)) = result {
        let switch_ok = {
            let mut guard = state.lock().await;
            guard
                .switch_workspace(
                    base_dir.clone(),
                    files,
                    dir_mode,
                    target_file,
                    Some(original_path),
                )
                .is_ok()
        };
        if switch_ok {
            if let Ok(abort_handle) = start_watcher(&base_dir, state.clone()) {
                let mut guard = state.lock().await;
                guard.watcher_abort = Some(abort_handle);
            }
        }
    }

    Redirect::to("/")
}
