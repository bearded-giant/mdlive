use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
};
use minijinja::context;
use minijinja::value::Value;
use serde::Deserialize;
use std::fs;
use std::time::UNIX_EPOCH;

use crate::state::{MarkdownState, SharedMarkdownState};
use crate::template::{template_env, TEMPLATE_NAME};
use crate::tree::build_file_tree;
use crate::util::{file_type_class, is_image_file, is_supported_file};

use super::static_files::serve_static_file_inner;

#[derive(Deserialize)]
pub(crate) struct NewFileQuery {
    #[serde(default)]
    dir: String,
}

pub(crate) async fn serve_html_root(
    State(state): State<SharedMarkdownState>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> impl IntoResponse {
    let mut state = state.lock().await;

    // ?picker forces the selector even when the daemon already holds a workspace,
    // so a freshly spawned window lands on the index regardless of shared state
    let force_picker = query.as_deref().is_some_and(|q| q.contains("picker"));

    if force_picker || (state.daemon_mode && !state.has_workspace()) {
        return render_workspace_picker(&state);
    }

    if state.is_directory_mode {
        return render_empty_landing(&state);
    }

    let filename = match state.get_sorted_filenames().into_iter().next() {
        Some(name) => name,
        None => {
            if state.daemon_mode {
                return render_workspace_picker(&state);
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("No files available to serve".to_string()),
            );
        }
    };

    let _ = state.refresh_file(&filename);

    render_file_with_default_flag(&state, &filename, true).await
}

fn render_empty_landing(state: &MarkdownState) -> (StatusCode, Html<String>) {
    let env = template_env();
    let template = match env.get_template(TEMPLATE_NAME) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!("Template error: {e}")),
            );
        }
    };

    let file_infos = state.get_file_infos();
    let dir_paths = crate::util::scan_directories(&state.base_dir);
    let tree = build_file_tree(&file_infos, &dir_paths);

    let saved_tabs = crate::handlers::api::read_workspace_tabs(&state.base_dir);

    let content = Value::from_safe_string(
        "<div class=\"landing-page\">\
         <h2>Pick a file to get started</h2>\
         <p>Select a file from the sidebar, or press <code>n</code> to create a new one.</p>\
         </div>"
            .to_string(),
    );

    match template.render(context! {
        content => content,
        file_type => "markdown",
        show_navigation => true,
        has_history => false,
        tree => tree,
        current_file => "",
        base_dir => state.base_dir.display().to_string(),
        daemon_mode => state.daemon_mode,
        is_default_file => true,
        is_landing => true,
        saved_tabs => serde_json::to_value(&saved_tabs).unwrap_or(serde_json::Value::Null),
    }) {
        Ok(r) => (StatusCode::OK, Html(r)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(format!("Rendering error: {e}")),
        ),
    }
}

pub(crate) async fn serve_file(
    AxumPath(filepath): AxumPath<String>,
    State(state): State<SharedMarkdownState>,
) -> axum::response::Response {
    if is_supported_file(std::path::Path::new(&filepath)) {
        let mut state = state.lock().await;

        if !state.tracked_files.contains_key(&filepath) {
            return render_not_found(&state, &filepath).into_response();
        }

        let _ = state.refresh_file(&filepath);

        let (status, html) = render_file_with_default_flag(&state, &filepath, false).await;
        (status, html).into_response()
    } else if is_image_file(&filepath) {
        serve_static_file_inner(filepath, state).await
    } else {
        let state = state.lock().await;
        render_not_found(&state, &filepath).into_response()
    }
}

fn render_workspace_picker(state: &MarkdownState) -> (StatusCode, Html<String>) {
    let env = template_env();
    let template = match env.get_template(TEMPLATE_NAME) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!("Template error: {e}")),
            );
        }
    };

    let recent: Vec<minijinja::value::Value> = state
        .config
        .as_ref()
        .map(|c| {
            c.recent
                .iter()
                .map(|r| {
                    minijinja::context! {
                        path => r.path.clone(),
                        mode => r.mode.clone(),
                        last_opened => r.last_opened,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    match template.render(context! {
        daemon_mode => true,
        no_workspace => true,
        show_navigation => false,
        recent_workspaces => recent,
        current_file => "",
        base_dir => "",
    }) {
        Ok(r) => (StatusCode::OK, Html(r)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(format!("Rendering error: {e}")),
        ),
    }
}

async fn render_file_with_default_flag(
    state: &MarkdownState,
    current_file: &str,
    is_default_file: bool,
) -> (StatusCode, Html<String>) {
    let env = template_env();
    let template = match env.get_template(TEMPLATE_NAME) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!("Template error: {e}")),
            );
        }
    };

    let file_type = file_type_class(current_file);

    let (content, has_mermaid, file_modified) =
        if let Some(tracked) = state.tracked_files.get(current_file) {
            let html = &tracked.html;
            let mermaid = matches!(file_type, "markdown" | "mermaid")
                && html.contains(r#"class="language-mermaid""#);
            let modified = tracked
                .last_modified
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (Value::from_safe_string(html.clone()), mermaid, modified)
        } else {
            return (StatusCode::NOT_FOUND, Html("File not found".to_string()));
        };

    let has_history = state.mdlive_dir.is_some();

    let saved_tabs = if state.show_navigation() {
        crate::handlers::api::read_workspace_tabs(&state.base_dir)
    } else {
        crate::handlers::api::WorkspaceTabsState::default()
    };
    let saved_tabs_value = serde_json::to_value(&saved_tabs).unwrap_or(serde_json::Value::Null);

    let rendered = if state.show_navigation() {
        let file_infos = state.get_file_infos();
        let dir_paths = crate::util::scan_directories(&state.base_dir);
        let tree = build_file_tree(&file_infos, &dir_paths);

        match template.render(context! {
            content => content,
            file_type => file_type,
            file_modified => file_modified,
            mermaid_enabled => has_mermaid,
            show_navigation => true,
            has_history => has_history,
            tree => tree,
            current_file => current_file,
            base_dir => state.base_dir.display().to_string(),
            daemon_mode => state.daemon_mode,
            is_default_file => is_default_file,
            saved_tabs => saved_tabs_value,
        }) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(format!("Rendering error: {e}")),
                );
            }
        }
    } else {
        match template.render(context! {
            content => content,
            file_type => file_type,
            file_modified => file_modified,
            mermaid_enabled => has_mermaid,
            show_navigation => false,
            has_history => has_history,
            current_file => current_file,
            base_dir => state.base_dir.display().to_string(),
            daemon_mode => state.daemon_mode,
            is_default_file => is_default_file,
        }) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(format!("Rendering error: {e}")),
                );
            }
        }
    };

    (StatusCode::OK, Html(rendered))
}

pub(crate) async fn serve_editor(
    AxumPath(filepath): AxumPath<String>,
    State(state): State<SharedMarkdownState>,
) -> impl IntoResponse {
    let state = state.lock().await;

    if !state.tracked_files.contains_key(&filepath) {
        return render_not_found(&state, &filepath);
    }

    let tracked = &state.tracked_files[&filepath];
    let raw_content = fs::read_to_string(&tracked.path).unwrap_or_default();

    render_editor(&state, &filepath, &raw_content, false)
}

pub(crate) async fn serve_new_file_editor(
    Query(params): Query<NewFileQuery>,
    State(state): State<SharedMarkdownState>,
) -> impl IntoResponse {
    let state = state.lock().await;

    if !state.is_directory_mode {
        return (
            StatusCode::BAD_REQUEST,
            Html("New file only available in directory mode".to_string()),
        );
    }

    let default_name = if params.dir.is_empty() {
        "new.md".to_string()
    } else {
        format!("{}/new.md", params.dir)
    };

    render_editor(&state, &default_name, "", true)
}

fn render_editor(
    state: &MarkdownState,
    current_file: &str,
    raw_content: &str,
    new_file_mode: bool,
) -> (StatusCode, Html<String>) {
    let env = template_env();
    let template = match env.get_template(TEMPLATE_NAME) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!("Template error: {e}")),
            );
        }
    };

    let file_type = file_type_class(current_file);
    let has_history = state.mdlive_dir.is_some() && !new_file_mode;
    let file_modified = if !new_file_mode {
        state
            .tracked_files
            .get(current_file)
            .and_then(|tf| tf.last_modified.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    } else {
        0
    };

    let saved_tabs_value = if state.show_navigation() {
        serde_json::to_value(crate::handlers::api::read_workspace_tabs(&state.base_dir))
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };

    let rendered = if state.show_navigation() {
        let file_infos = state.get_file_infos();
        let dir_paths = crate::util::scan_directories(&state.base_dir);
        let tree = build_file_tree(&file_infos, &dir_paths);

        match template.render(context! {
            editor_mode => true,
            new_file_mode => new_file_mode,
            file_type => file_type,
            file_modified => file_modified,
            raw_content => raw_content,
            current_file => current_file,
            has_history => has_history,
            show_navigation => true,
            tree => tree,
            base_dir => state.base_dir.display().to_string(),
            daemon_mode => state.daemon_mode,
            saved_tabs => saved_tabs_value,
        }) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(format!("Rendering error: {e}")),
                );
            }
        }
    } else {
        match template.render(context! {
            editor_mode => true,
            new_file_mode => new_file_mode,
            file_type => file_type,
            file_modified => file_modified,
            raw_content => raw_content,
            current_file => current_file,
            has_history => has_history,
            show_navigation => false,
            base_dir => state.base_dir.display().to_string(),
            daemon_mode => state.daemon_mode,
        }) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(format!("Rendering error: {e}")),
                );
            }
        }
    };

    (StatusCode::OK, Html(rendered))
}

fn render_not_found(state: &MarkdownState, path: &str) -> (StatusCode, Html<String>) {
    let env = template_env();
    let template = match env.get_template(TEMPLATE_NAME) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!("Template error: {e}")),
            );
        }
    };

    let escaped = path
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let hint = if state.show_navigation() {
        "Pick a file from the sidebar, or \
         <a href=\"/\">open the workspace default</a>."
    } else {
        "<a href=\"/\">Reload the workspace</a>."
    };
    let content = Value::from_safe_string(format!(
        "<div class=\"not-found-page\"><h2>File unavailable</h2>\
         <p><code>{escaped}</code> was moved, renamed, or deleted.</p>\
         <p>{hint}</p></div>"
    ));

    let rendered = if state.show_navigation() {
        let file_infos = state.get_file_infos();
        let dir_paths = crate::util::scan_directories(&state.base_dir);
        let tree = build_file_tree(&file_infos, &dir_paths);
        template.render(context! {
            content => content,
            file_type => "markdown",
            show_navigation => true,
            tree => tree,
            current_file => "",
            base_dir => state.base_dir.display().to_string(),
            daemon_mode => state.daemon_mode,
            unavailable_path => path,
        })
    } else {
        template.render(context! {
            content => content,
            file_type => "markdown",
            show_navigation => false,
            current_file => "",
            base_dir => state.base_dir.display().to_string(),
            daemon_mode => state.daemon_mode,
            unavailable_path => path,
        })
    };

    match rendered {
        Ok(r) => (StatusCode::NOT_FOUND, Html(r)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(format!("Rendering error: {e}")),
        ),
    }
}
