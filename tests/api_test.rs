mod common;

use common::*;
use std::fs;
use std::thread;
use std::time::Duration;

#[tokio::test]
async fn test_api_raw_content() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/api/raw_content?path=test1.md").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("content-type"), "text/plain; charset=utf-8");
    let body = response.text();
    assert_eq!(body, TEST_FILE_1_CONTENT);
}

#[tokio::test]
async fn test_api_raw_content_not_found() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/api/raw_content?path=nonexistent.md").await;
    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn test_api_raw_content_traversal_blocked() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .get("/api/raw_content?path=../../../etc/passwd")
        .await;
    assert_eq!(response.status_code(), 403);
}

#[tokio::test]
async fn test_api_delete_file() {
    let (server, temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/delete_file")
        .json(&serde_json::json!({"path": "test1.md"}))
        .await;
    assert_eq!(response.status_code(), 200);
    let body: serde_json::Value = response.json();
    assert_eq!(body["success"], true);

    assert!(!temp_dir.path().join("test1.md").exists());

    let response = server.get("/test1.md").await;
    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn test_api_delete_file_not_found() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/delete_file")
        .json(&serde_json::json!({"path": "nonexistent.md"}))
        .await;
    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn test_api_delete_file_traversal_blocked() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/delete_file")
        .json(&serde_json::json!({"path": "../../../etc/passwd"}))
        .await;
    assert_eq!(response.status_code(), 403);
}

#[tokio::test]
async fn test_api_move_file() {
    let (server, temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/move_file")
        .json(&serde_json::json!({"path": "test1.md", "target": "renamed.md"}))
        .await;
    assert_eq!(response.status_code(), 200);
    let body: serde_json::Value = response.json();
    assert_eq!(body["success"], true);
    assert_eq!(body["path"], "renamed.md");

    assert!(!temp_dir.path().join("test1.md").exists());
    assert!(temp_dir.path().join("renamed.md").exists());

    let response = server.get("/renamed.md").await;
    assert_eq!(response.status_code(), 200);
    let html = response.text();
    assert!(html.contains("Test 1"));
}

#[tokio::test]
async fn test_api_move_file_to_subdirectory() {
    let (server, temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/move_file")
        .json(&serde_json::json!({"path": "test1.md", "target": "archive/test1.md"}))
        .await;
    assert_eq!(response.status_code(), 200);

    assert!(temp_dir.path().join("archive/test1.md").exists());
}

#[tokio::test]
async fn test_api_move_file_target_exists() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/move_file")
        .json(&serde_json::json!({"path": "test1.md", "target": "test3.md"}))
        .await;
    assert_eq!(response.status_code(), 409);
}

#[tokio::test]
async fn test_api_move_file_non_md_target() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/move_file")
        .json(&serde_json::json!({"path": "test1.md", "target": "test1.sh"}))
        .await;
    assert_eq!(response.status_code(), 400);
}

#[tokio::test]
async fn test_api_create_file() {
    let (server, temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/create_file")
        .json(&serde_json::json!({"path": "new_file.md"}))
        .await;
    assert_eq!(response.status_code(), 201);
    let body: serde_json::Value = response.json();
    assert_eq!(body["success"], true);
    assert_eq!(body["path"], "new_file.md");

    let content = fs::read_to_string(temp_dir.path().join("new_file.md")).unwrap();
    assert!(content.contains("# new_file"));

    let response = server.get("/new_file.md").await;
    assert_eq!(response.status_code(), 200);
}

#[tokio::test]
async fn test_api_create_file_with_content() {
    let (server, temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/create_file")
        .json(&serde_json::json!({"path": "custom.md", "content": "# Custom\n\nHello world"}))
        .await;
    assert_eq!(response.status_code(), 201);

    let content = fs::read_to_string(temp_dir.path().join("custom.md")).unwrap();
    assert_eq!(content, "# Custom\n\nHello world");
}

#[tokio::test]
async fn test_api_create_file_in_new_subdirectory() {
    let (server, temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/create_file")
        .json(&serde_json::json!({"path": "deep/nested/dir/new.md"}))
        .await;
    assert_eq!(response.status_code(), 201);

    assert!(temp_dir.path().join("deep/nested/dir/new.md").exists());
}

#[tokio::test]
async fn test_api_create_file_already_exists() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/create_file")
        .json(&serde_json::json!({"path": "test1.md"}))
        .await;
    assert_eq!(response.status_code(), 409);
}

#[tokio::test]
async fn test_api_create_file_non_md_extension() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/create_file")
        .json(&serde_json::json!({"path": "script.sh"}))
        .await;
    assert_eq!(response.status_code(), 400);
}

// save_file tests

#[tokio::test]
async fn test_api_save_file() {
    let (server, temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/save_file")
        .json(&serde_json::json!({"path": "test1.md", "content": "# Updated\n\nNew content"}))
        .await;
    assert_eq!(response.status_code(), 200);
    let body: serde_json::Value = response.json();
    assert_eq!(body["success"], true);

    let content = fs::read_to_string(temp_dir.path().join("test1.md")).unwrap();
    assert_eq!(content, "# Updated\n\nNew content");
}

#[tokio::test]
async fn test_api_save_file_not_tracked() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/save_file")
        .json(&serde_json::json!({"path": "nonexistent.md", "content": "test"}))
        .await;
    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn test_api_save_file_traversal_blocked() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/save_file")
        .json(&serde_json::json!({"path": "../../../etc/passwd", "content": "test"}))
        .await;
    assert_eq!(response.status_code(), 403);
}

#[tokio::test]
async fn test_api_save_file_creates_history_snapshot() {
    let (server, temp_dir) = create_directory_server().await;

    let response = server
        .post("/api/save_file")
        .json(&serde_json::json!({"path": "test1.md", "content": "# Updated content"}))
        .await;
    assert_eq!(response.status_code(), 200);

    let history_dir = temp_dir.path().join(".mdlive/history/test1.md");
    assert!(history_dir.exists(), "history directory should exist");
    let entries: Vec<_> = fs::read_dir(&history_dir).unwrap().collect();
    assert_eq!(entries.len(), 1, "should have one snapshot");

    let snapshot = entries[0].as_ref().unwrap();
    let snapshot_content = fs::read_to_string(snapshot.path()).unwrap();
    assert_eq!(snapshot_content, TEST_FILE_1_CONTENT);
}

#[tokio::test]
async fn test_api_file_history() {
    let (server, _temp_dir) = create_directory_server().await;

    server
        .post("/api/save_file")
        .json(&serde_json::json!({"path": "test1.md", "content": "# Version 2"}))
        .await;
    thread::sleep(Duration::from_millis(1100));
    server
        .post("/api/save_file")
        .json(&serde_json::json!({"path": "test1.md", "content": "# Version 3"}))
        .await;

    let response = server.get("/api/file_history?path=test1.md").await;
    assert_eq!(response.status_code(), 200);
    let body: serde_json::Value = response.json();
    assert_eq!(body["success"], true);

    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "should have two history entries");

    let ts0: u64 = entries[0]["timestamp"].as_str().unwrap().parse().unwrap();
    let ts1: u64 = entries[1]["timestamp"].as_str().unwrap().parse().unwrap();
    assert!(ts0 >= ts1, "entries should be newest first");
}

#[tokio::test]
async fn test_api_restore_version() {
    let (server, _temp_dir) = create_directory_server().await;

    server
        .post("/api/save_file")
        .json(&serde_json::json!({"path": "test1.md", "content": "# New content"}))
        .await;

    let history_response = server.get("/api/file_history?path=test1.md").await;
    let history: serde_json::Value = history_response.json();
    let timestamp = history["entries"][0]["timestamp"].as_str().unwrap();

    let response = server
        .post("/api/restore_version")
        .json(&serde_json::json!({"path": "test1.md", "timestamp": timestamp}))
        .await;
    assert_eq!(response.status_code(), 200);
    let body: serde_json::Value = response.json();
    assert_eq!(body["success"], true);
    assert_eq!(body["content"], TEST_FILE_1_CONTENT);
}

#[tokio::test]
async fn test_api_delete_history_entry() {
    let (server, temp_dir) = create_directory_server().await;

    // create a snapshot by saving
    server
        .post("/api/save_file")
        .json(&serde_json::json!({"path": "test1.md", "content": "# New content"}))
        .await;

    // get the timestamp
    let history_response = server.get("/api/file_history?path=test1.md").await;
    let history: serde_json::Value = history_response.json();
    let timestamp = history["entries"][0]["timestamp"].as_str().unwrap();

    // verify snapshot file exists
    let history_dir = temp_dir.path().join(".mdlive/history/test1.md");
    let snapshot_path = history_dir.join(format!("{timestamp}.md"));
    assert!(
        snapshot_path.exists(),
        "snapshot should exist before delete"
    );

    // delete it
    let response = server
        .delete("/api/delete_history_entry")
        .json(&serde_json::json!({"path": "test1.md", "timestamp": timestamp}))
        .await;
    assert_eq!(response.status_code(), 200);
    let body: serde_json::Value = response.json();
    assert_eq!(body["success"], true);

    // verify snapshot is gone
    assert!(!snapshot_path.exists(), "snapshot should be deleted");

    // verify history is now empty
    let history_response = server.get("/api/file_history?path=test1.md").await;
    let history: serde_json::Value = history_response.json();
    assert_eq!(
        history["entries"].as_array().unwrap().len(),
        0,
        "history should be empty after delete"
    );
}

#[tokio::test]
async fn test_api_delete_history_entry_not_found() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .delete("/api/delete_history_entry")
        .json(&serde_json::json!({"path": "test1.md", "timestamp": "9999999999"}))
        .await;
    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn test_api_delete_history_entry_traversal_blocked() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server
        .delete("/api/delete_history_entry")
        .json(&serde_json::json!({"path": "test1.md", "timestamp": "../../etc/passwd"}))
        .await;
    assert_eq!(response.status_code(), 403);
    let body: serde_json::Value = response.json();
    assert_eq!(body["success"], false);
}

// editor page tests

#[tokio::test]
async fn test_editor_page_for_existing_file() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/edit/test1.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();
    assert!(
        body.contains("editorTextarea"),
        "should contain editor textarea"
    );
    assert!(body.contains("marked.min.js"), "should include marked.js");
    assert!(body.contains("Test 1"), "should contain raw content");
}

#[tokio::test]
async fn test_editor_page_not_found() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/edit/nonexistent.md").await;
    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn test_new_file_editor_page() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/new?dir=").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();
    assert!(
        body.contains("editorFilename"),
        "should have filename input"
    );
    assert!(
        body.contains("editorTextarea"),
        "should contain editor textarea"
    );
}

#[tokio::test]
async fn test_new_file_editor_with_dir() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/new?dir=subdir").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();
    // minijinja auto-escapes / as &#x2f; in attribute values
    assert!(
        body.contains("subdir/new.md") || body.contains("subdir&#x2f;new.md"),
        "should have default path with dir prefix"
    );
}

#[tokio::test]
async fn test_editor_context_menu_has_edit() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/test1.md").await;
    let body = response.text();
    assert!(
        body.contains("\"Edit\"") || body.contains("'Edit'"),
        "context menu JS should include Edit option"
    );
}

// --- workspace tabs persistence (server-side store survives port shifts) ---

#[tokio::test]
async fn test_get_workspace_tabs_empty_when_no_state() {
    let (server, _dir) = create_directory_server().await;

    let response = server.get("/api/workspace/tabs").await;
    assert_eq!(response.status_code(), 200);
    let json: serde_json::Value = response.json();
    assert_eq!(json["tabs"].as_array().unwrap().len(), 0);
    assert!(json["active"].is_null());
}

#[tokio::test]
async fn test_save_then_get_workspace_tabs() {
    let (server, _dir) = create_directory_server().await;

    let payload = serde_json::json!({
        "tabs": [
            { "path": "test1.md", "mode": "view" },
            { "path": "test2.markdown", "mode": "edit" }
        ],
        "active": "test2.markdown"
    });

    let post = server.post("/api/workspace/tabs").json(&payload).await;
    assert_eq!(post.status_code(), 200);

    let get = server.get("/api/workspace/tabs").await;
    assert_eq!(get.status_code(), 200);
    let json: serde_json::Value = get.json();
    let tabs = json["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0]["path"], "test1.md");
    assert_eq!(tabs[1]["path"], "test2.markdown");
    assert_eq!(tabs[1]["mode"], "edit");
    assert_eq!(json["active"], "test2.markdown");
}

#[tokio::test]
async fn test_workspace_tabs_persist_to_mdlive_dir() {
    let (server, temp_dir) = create_directory_server().await;

    let payload = serde_json::json!({
        "tabs": [{ "path": "test1.md", "mode": "view" }],
        "active": "test1.md"
    });
    server
        .post("/api/workspace/tabs")
        .json(&payload)
        .await
        .assert_status_ok();

    let tabs_file = temp_dir.path().join(".mdlive").join("tabs.json");
    assert!(tabs_file.exists(), ".mdlive/tabs.json should be written");
    let written = fs::read_to_string(&tabs_file).unwrap();
    assert!(written.contains("test1.md"));
    assert!(written.contains("\"active\""));
}

#[tokio::test]
async fn test_landing_renders_server_tabs_for_resume() {
    let (server, _dir) = create_directory_server().await;

    // seed tab state
    let payload = serde_json::json!({
        "tabs": [{ "path": "test2.markdown", "mode": "view" }],
        "active": "test2.markdown"
    });
    server
        .post("/api/workspace/tabs")
        .json(&payload)
        .await
        .assert_status_ok();

    // landing should embed the server tab state for the resume script
    let response = server.get("/").await;
    let body = response.text();
    assert!(
        body.contains("__serverTabs"),
        "landing should expose __serverTabs JS var for resume"
    );
    assert!(
        body.contains("test2.markdown"),
        "embedded tab payload should contain saved path"
    );
}

#[tokio::test]
async fn test_api_create_file_csv_yaml_toml() {
    let (server, _temp_dir) = create_directory_server().await;

    for ext in &["csv", "yaml", "yml", "toml", "mmd"] {
        let path = format!("new.{ext}");
        let response = server
            .post("/api/create_file")
            .json(&serde_json::json!({"path": path}))
            .await;
        assert_eq!(
            response.status_code(),
            201,
            "create_file should accept .{ext}"
        );
    }
}

#[tokio::test]
async fn test_api_save_round_trip_csv_yaml_toml() {
    let (server, temp_dir) = create_directory_server().await;

    let cases = [
        ("data.csv", "a,b,c\n1,2,3\n"),
        ("conf.yaml", "key: value\nlist:\n  - one\n"),
        ("settings.toml", "[section]\nkey = \"v\"\n"),
        ("flow.mmd", "graph TD\n  A --> B\n"),
    ];

    for (path, content) in &cases {
        let create = server
            .post("/api/create_file")
            .json(&serde_json::json!({"path": path, "content": ""}))
            .await;
        assert_eq!(create.status_code(), 201, "create {path}");

        let save = server
            .post("/api/save_file")
            .json(&serde_json::json!({"path": path, "content": content}))
            .await;
        assert_eq!(save.status_code(), 200, "save {path}");

        let on_disk = fs::read_to_string(temp_dir.path().join(path)).unwrap();
        assert_eq!(on_disk, *content, "round-trip {path}");
    }
}
