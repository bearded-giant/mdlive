use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;

use crate::handlers;
use crate::state::MarkdownState;
use crate::watcher::start_watcher;

pub fn new_router(
    base_dir: PathBuf,
    tracked_files: Vec<PathBuf>,
    is_directory_mode: bool,
) -> Result<Router> {
    let base_dir = base_dir.canonicalize()?;

    let state = Arc::new(Mutex::new(MarkdownState::new(
        base_dir.clone(),
        tracked_files,
        is_directory_mode,
    )?));

    let abort_handle = start_watcher(&base_dir, state.clone())?;
    state.try_lock().unwrap().watcher_abort = Some(abort_handle);

    Ok(build_routes(state))
}

pub fn new_daemon_router() -> Router {
    let state = Arc::new(Mutex::new(MarkdownState::new_daemon()));
    build_routes(state)
}

pub fn new_daemon_router_with_config(config: crate::config::AppConfig) -> Router {
    let state = Arc::new(Mutex::new(MarkdownState::new_daemon_with_config(config)));
    build_routes(state)
}

fn build_routes(state: Arc<Mutex<MarkdownState>>) -> Router {
    Router::new()
        .route("/", get(handlers::pages::serve_html_root))
        .route("/ws", get(handlers::websocket::websocket_handler))
        .route("/new", get(handlers::pages::serve_new_file_editor))
        .route(
            "/mermaid.min.js",
            get(handlers::static_files::serve_mermaid_js),
        )
        .route(
            "/highlight.min.js",
            get(handlers::static_files::serve_highlight_js),
        )
        .route(
            "/marked.min.js",
            get(handlers::static_files::serve_marked_js),
        )
        .route("/static/md.png", get(handlers::static_files::serve_md_icon))
        .route(
            "/static/favicon.png",
            get(handlers::static_files::serve_favicon),
        )
        .route(
            "/static/mdlive.png",
            get(handlers::static_files::serve_mdlive_logo),
        )
        .route("/api/raw_content", get(handlers::api::api_raw_content))
        .route("/api/delete_file", post(handlers::api::api_delete_file))
        .route("/api/move_file", post(handlers::api::api_move_file))
        .route("/api/create_file", post(handlers::api::api_create_file))
        .route(
            "/api/create_directory",
            post(handlers::api::api_create_directory),
        )
        .route(
            "/api/delete_directory",
            post(handlers::api::api_delete_directory),
        )
        .route("/api/save_file", post(handlers::api::api_save_file))
        .route("/api/file_history", get(handlers::api::api_file_history))
        .route(
            "/api/restore_version",
            post(handlers::api::api_restore_version),
        )
        .route(
            "/api/delete_history_entry",
            axum::routing::delete(handlers::api::api_delete_history_entry),
        )
        .route(
            "/api/workspace/switch",
            post(handlers::workspace::api_workspace_switch),
        )
        .route(
            "/api/workspace/current",
            get(handlers::workspace::api_workspace_current),
        )
        .route(
            "/api/workspace/recent",
            get(handlers::workspace::api_workspace_recent),
        )
        .route(
            "/api/workspace/browse",
            get(handlers::workspace::api_workspace_browse),
        )
        .route("/open", get(handlers::workspace::open_and_redirect))
        .route("/edit/*filepath", get(handlers::pages::serve_editor))
        .route("/*filepath", get(handlers::pages::serve_file))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
