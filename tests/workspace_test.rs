mod common;

use common::{create_daemon_server, scan_supported_files};
use std::fs;

#[tokio::test]
async fn test_daemon_root_shows_workspace_picker() {
    let (server, _cfg_dir) = create_daemon_server();
    let response = server.get("/").await;
    response.assert_status_ok();
    let body = response.text();
    assert!(
        body.contains("Open Workspace"),
        "should show workspace picker"
    );
    assert!(body.contains("pickerPageInput"), "should have path input");
}

#[tokio::test]
async fn test_workspace_current_no_workspace() {
    let (server, _cfg_dir) = create_daemon_server();
    let response = server.get("/api/workspace/current").await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json["success"], true);
    assert!(json["base_dir"].is_null(), "no workspace should be loaded");
}

#[tokio::test]
async fn test_workspace_recent_returns_valid_response() {
    let (server, _cfg_dir) = create_daemon_server();
    let response = server.get("/api/workspace/recent").await;
    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json["success"], true);
    assert!(json["recent"].is_array());
}

#[tokio::test]
async fn test_workspace_switch_to_directory() {
    let (server, _cfg_dir) = create_daemon_server();
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(temp_dir.path().join("hello.md"), "# Hello").unwrap();

    let response = server
        .post("/api/workspace/switch")
        .json(&serde_json::json!({
            "path": temp_dir.path().to_str().unwrap()
        }))
        .await;

    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json["success"], true);
    assert_eq!(json["mode"], "directory");
    assert_eq!(json["file_count"], 1);

    // verify current workspace updated
    let current = server.get("/api/workspace/current").await;
    let current_json: serde_json::Value = current.json();
    assert!(!current_json["base_dir"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn test_workspace_switch_to_file() {
    let (server, _cfg_dir) = create_daemon_server();
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test.md");
    fs::write(&file_path, "# Test file").unwrap();

    let response = server
        .post("/api/workspace/switch")
        .json(&serde_json::json!({
            "path": file_path.to_str().unwrap()
        }))
        .await;

    response.assert_status_ok();
    let json: serde_json::Value = response.json();
    assert_eq!(json["success"], true);
    assert_eq!(json["mode"], "file");
    assert_eq!(json["file_count"], 1);
}

#[tokio::test]
async fn test_workspace_switch_nonexistent_path() {
    let (server, _cfg_dir) = create_daemon_server();
    let response = server
        .post("/api/workspace/switch")
        .json(&serde_json::json!({
            "path": "/tmp/nonexistent_mdlive_test_path_12345"
        }))
        .await;

    response.assert_status_bad_request();
    let json: serde_json::Value = response.json();
    assert_eq!(json["success"], false);
    assert!(json["error"].as_str().unwrap().contains("does not exist"));
}

#[tokio::test]
async fn test_workspace_switch_empty_directory() {
    let (server, _cfg_dir) = create_daemon_server();
    let temp_dir = tempfile::tempdir().unwrap();

    let response = server
        .post("/api/workspace/switch")
        .json(&serde_json::json!({
            "path": temp_dir.path().to_str().unwrap()
        }))
        .await;

    response.assert_status_bad_request();
    let json: serde_json::Value = response.json();
    assert_eq!(json["success"], false);
    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("no supported files"));
}

#[tokio::test]
async fn test_workspace_switch_then_serves_files() {
    let (server, _cfg_dir) = create_daemon_server();
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(temp_dir.path().join("readme.md"), "# Project readme").unwrap();

    // switch workspace
    let switch = server
        .post("/api/workspace/switch")
        .json(&serde_json::json!({
            "path": temp_dir.path().to_str().unwrap()
        }))
        .await;
    switch.assert_status_ok();

    // root should render landing (tree shown, picker gone) without auto-opening a file
    let root = server.get("/").await;
    root.assert_status_ok();
    let body = root.text();
    assert!(
        body.contains("readme.md"),
        "tree should list the workspace file"
    );
    assert!(
        !body.contains("pickerPageInput"),
        "should not show picker when workspace is loaded"
    );
    assert!(
        body.contains("landing-page"),
        "directory mode should land on welcome state, not auto-open a file"
    );
}

#[tokio::test]
async fn test_workspace_switch_adds_to_recent() {
    let (server, _cfg_dir) = create_daemon_server();
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(temp_dir.path().join("test.md"), "# Test").unwrap();

    let canonical = temp_dir.path().canonicalize().unwrap();
    let canonical_str = canonical.display().to_string();

    server
        .post("/api/workspace/switch")
        .json(&serde_json::json!({
            "path": temp_dir.path().to_str().unwrap()
        }))
        .await
        .assert_status_ok();

    let recent = server.get("/api/workspace/recent").await;
    recent.assert_status_ok();
    let json: serde_json::Value = recent.json();
    let entries = json["recent"].as_array().unwrap();

    // the switched-to path should be in the recent list
    let found = entries
        .iter()
        .any(|e| e["path"].as_str().unwrap() == canonical_str);
    assert!(found, "switched workspace should appear in recent list");
}

#[tokio::test]
async fn test_workspace_double_switch() {
    let (server, _cfg_dir) = create_daemon_server();

    let dir1 = tempfile::tempdir().unwrap();
    fs::write(dir1.path().join("a.md"), "# Dir 1").unwrap();

    let dir2 = tempfile::tempdir().unwrap();
    fs::write(dir2.path().join("b.md"), "# Dir 2").unwrap();

    // switch to dir1
    server
        .post("/api/workspace/switch")
        .json(&serde_json::json!({
            "path": dir1.path().to_str().unwrap()
        }))
        .await
        .assert_status_ok();

    // switch to dir2
    server
        .post("/api/workspace/switch")
        .json(&serde_json::json!({
            "path": dir2.path().to_str().unwrap()
        }))
        .await
        .assert_status_ok();

    // root should render the dir2 landing page (tree lists b.md, not a.md)
    let root = server.get("/").await;
    let body = root.text();
    assert!(body.contains("b.md"));
    assert!(!body.contains("a.md"));
    assert!(body.contains("landing-page"));

    // current workspace should point to dir2
    let current = server.get("/api/workspace/current").await;
    let json: serde_json::Value = current.json();
    let base = json["base_dir"].as_str().unwrap();
    let dir2_canon = dir2.path().canonicalize().unwrap();
    assert_eq!(base, dir2_canon.display().to_string());
}

#[tokio::test]
async fn test_daemon_mode_has_open_button() {
    let (server, _cfg_dir) = create_daemon_server();
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(temp_dir.path().join("test.md"), "# Test").unwrap();

    server
        .post("/api/workspace/switch")
        .json(&serde_json::json!({
            "path": temp_dir.path().to_str().unwrap()
        }))
        .await
        .assert_status_ok();

    let root = server.get("/").await;
    let body = root.text();
    assert!(
        body.contains("id=\"workspaceModal\""),
        "daemon mode should have workspace modal"
    );
}

#[tokio::test]
async fn test_non_daemon_mode_no_workspace_modal() {
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(temp_dir.path().join("test.md"), "# Test").unwrap();
    let tracked = scan_supported_files(temp_dir.path()).unwrap();
    let router = mdlive::new_router(temp_dir.path().to_path_buf(), tracked, true).unwrap();
    let server = axum_test::TestServer::new(router).unwrap();

    let root = server.get("/").await;
    let body = root.text();
    assert!(
        !body.contains("id=\"workspaceModal\""),
        "non-daemon mode should not have workspace modal"
    );
}
