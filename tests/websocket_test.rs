mod common;

use common::*;
use mdlive::ServerMessage;
use std::fs;
use std::time::Duration;

#[tokio::test]
async fn test_websocket_connection() {
    let (server, _, _dir) = create_test_server_with_http("# WebSocket Test").await;

    let response = server.get_websocket("/ws").await;
    response.assert_status_switching_protocols();
}

#[tokio::test]
async fn test_file_modification_updates_via_websocket() {
    let (server, file_path, _dir) = create_test_server_with_http("# Original Content").await;

    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    fs::write(&file_path, "# Modified Content").expect("Failed to modify file");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(update_message) => {
            if let ServerMessage::Reload = update_message {
                // success
            } else {
                panic!("Expected Reload message after file modification");
            }
        }
        Err(_) => {
            panic!("Timeout waiting for WebSocket update after file modification");
        }
    }
}

#[tokio::test]
async fn test_directory_mode_websocket_file_modification() {
    let (server, temp_dir) = create_directory_server_with_http().await;

    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    let test_file = temp_dir.path().join("test1.md");
    fs::write(&test_file, "# Modified Test 1\n\nContent has changed")
        .expect("Failed to modify file");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(update_message) => {
            if let ServerMessage::Reload = update_message {
                // success
            } else {
                panic!("Expected Reload message after file modification");
            }
        }
        Err(_) => {
            panic!("Timeout waiting for WebSocket update after file modification");
        }
    }
}

#[tokio::test]
async fn test_directory_mode_new_file_triggers_reload() {
    let (server, temp_dir) = create_directory_server_with_http().await;

    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    let new_file = temp_dir.path().join("test4.md");
    fs::write(&new_file, "# Test 4\n\nThis is a new file").expect("Failed to create new file");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(update_message) => {
            if let ServerMessage::Reload = update_message {
                // success
            } else {
                panic!("Expected Reload message after new file creation");
            }
        }
        Err(_) => {
            panic!("Timeout waiting for WebSocket update after new file creation");
        }
    }

    let response = server.get("/test1.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        body.contains("test4.md"),
        "New file should appear in navigation"
    );

    let new_file_response = server.get("/test4.md").await;
    assert_eq!(new_file_response.status_code(), 200);
    let new_file_body = new_file_response.text();
    assert!(new_file_body.contains("<h1>Test 4</h1>"));
    assert!(new_file_body.contains("This is a new file"));
}

// --- editor save simulation tests ---

#[tokio::test]
async fn test_editor_save_simulation_single_file_mode() {
    let (server, file_path, _dir) =
        create_test_server_with_http("# Original\n\nOriginal content").await;

    let backup_path = file_path.with_extension("md~");

    let initial_response = server.get("/").await;
    assert_eq!(initial_response.status_code(), 200);
    assert!(initial_response.text().contains("Original content"));

    fs::rename(&file_path, &backup_path).expect("Failed to rename to backup");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let during_save_response = server.get("/").await;
    assert_eq!(
        during_save_response.status_code(),
        200,
        "File should not return 404 during editor save"
    );

    fs::write(&file_path, "# Updated\n\nUpdated content").expect("Failed to write new file");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let final_response = server.get("/").await;
    assert_eq!(final_response.status_code(), 200);
    let final_body = final_response.text();
    assert!(
        final_body.contains("Updated content"),
        "Should serve updated content after save"
    );
    assert!(
        !final_body.contains("Original content"),
        "Should not serve old content"
    );

    let _ = fs::remove_file(&backup_path);
}

#[tokio::test]
async fn test_editor_save_simulation_directory_mode() {
    let (server, temp_dir) = create_directory_server_with_http().await;

    let file_path = temp_dir.path().join("test1.md");
    let backup_path = temp_dir.path().join("test1.md~");

    let initial_response = server.get("/test1.md").await;
    assert_eq!(initial_response.status_code(), 200);
    assert!(initial_response.text().contains("Content of test1"));

    fs::rename(&file_path, &backup_path).expect("Failed to rename to backup");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let during_save_response = server.get("/test1.md").await;
    assert_eq!(
        during_save_response.status_code(),
        200,
        "File should not return 404 during editor save in directory mode"
    );

    fs::write(&file_path, "# Test 1 Updated\n\nUpdated content").expect("Failed to write new file");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let final_response = server.get("/test1.md").await;
    assert_eq!(final_response.status_code(), 200);
    let final_body = final_response.text();
    assert!(
        final_body.contains("Updated content"),
        "Should serve updated content after save"
    );

    let _ = fs::remove_file(&backup_path);
}

#[tokio::test]
async fn test_no_404_during_editor_save_sequence() {
    let (server, temp_dir) = create_directory_server_with_http().await;
    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    let file_path = temp_dir.path().join("test1.md");
    let backup_path = temp_dir.path().join("test1.md~");

    fs::rename(&file_path, &backup_path).expect("Failed to rename to backup");
    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let response_after_rename = server.get("/test1.md").await;
    assert_eq!(
        response_after_rename.status_code(),
        200,
        "Should not get 404 after rename to backup"
    );

    fs::write(&file_path, "# Test 1 Updated\n\nNew content").expect("Failed to write new file");
    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let response_after_create = server.get("/test1.md").await;
    assert_eq!(
        response_after_create.status_code(),
        200,
        "Should successfully serve after new file created"
    );
    assert!(response_after_create.text().contains("New content"));

    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    assert!(update_result.is_ok(), "Should receive reload after save");

    let _ = fs::remove_file(&backup_path);
}

#[tokio::test]
async fn test_temp_file_rename_triggers_reload_single_file_mode() {
    let (server, file_path, _dir) =
        create_test_server_with_http("# Original\n\nOriginal content").await;

    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    let temp_write_path = file_path.with_extension("md.tmp.12345");

    let initial_response = server.get("/").await;
    assert_eq!(initial_response.status_code(), 200);
    assert!(initial_response.text().contains("Original content"));

    fs::write(
        &temp_write_path,
        "# Updated\n\nUpdated content via temp file",
    )
    .expect("Failed to write temp file");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    fs::rename(&temp_write_path, &file_path).expect("Failed to rename temp file");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(update_message) => {
            if let ServerMessage::Reload = update_message {
                // success
            } else {
                panic!("Expected Reload message after temp file rename");
            }
        }
        Err(_) => {
            panic!("Timeout waiting for WebSocket update after temp file rename");
        }
    }

    let final_response = server.get("/").await;
    assert_eq!(final_response.status_code(), 200);
    let final_body = final_response.text();
    assert!(final_body.contains("Updated content via temp file"));
    assert!(!final_body.contains("Original content"));
}

#[tokio::test]
async fn test_temp_file_rename_triggers_reload_directory_mode() {
    let (server, temp_dir) = create_directory_server_with_http().await;

    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    let file_path = temp_dir.path().join("test1.md");
    let temp_write_path = temp_dir.path().join("test1.md.tmp.67890");

    let initial_response = server.get("/test1.md").await;
    assert_eq!(initial_response.status_code(), 200);
    assert!(initial_response.text().contains("Content of test1"));

    fs::write(
        &temp_write_path,
        "# Test 1 Updated\n\nUpdated via temp file rename",
    )
    .expect("Failed to write temp file");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    fs::rename(&temp_write_path, &file_path).expect("Failed to rename temp file");

    tokio::time::sleep(Duration::from_millis(FILE_WATCH_DELAY_MS)).await;

    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(update_message) => {
            if let ServerMessage::Reload = update_message {
                // success
            } else {
                panic!("Expected Reload message after temp file rename in directory mode");
            }
        }
        Err(_) => {
            panic!("Timeout waiting for WebSocket update after temp file rename in directory mode");
        }
    }

    let final_response = server.get("/test1.md").await;
    assert_eq!(final_response.status_code(), 200);
    let final_body = final_response.text();
    assert!(final_body.contains("Updated via temp file rename"));
    assert!(!final_body.contains("Content of test1"));
}

// --- deferred-remove watcher tests ---

const DEFERRED_REMOVE_WAIT_MS: u64 = 800; // > 500ms grace period

#[tokio::test]
async fn test_external_delete_untracks_file_after_grace_period() {
    let (server, temp_dir) = create_directory_server_with_http().await;

    let _ws = server.get_websocket("/ws").await.into_websocket().await;

    let target = temp_dir.path().join("test1.md");
    fs::remove_file(&target).expect("failed to delete test1.md");

    // wait past the watcher's deferred-remove grace period
    tokio::time::sleep(Duration::from_millis(DEFERRED_REMOVE_WAIT_MS)).await;

    // tree (rendered on any page) should no longer list the deleted file
    let response = server.get("/test2.markdown").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        !body.contains("\"/test1.md\""),
        "deleted file should be removed from tree links after grace period"
    );
    assert!(
        body.contains("test2.markdown"),
        "remaining files should still be tracked"
    );

    // direct file URL should now 404 because tracked entry was dropped
    let gone = server.get("/test1.md").await;
    assert_eq!(
        gone.status_code(),
        404,
        "deleted file should 404 after grace period"
    );
}

#[tokio::test]
async fn test_atomic_save_does_not_untrack_file() {
    // editors like neovim save by deleting then recreating. the deferred-remove
    // grace period must not untrack a file that gets recreated quickly.
    let (server, temp_dir) = create_directory_server_with_http().await;

    let _ws = server.get_websocket("/ws").await.into_websocket().await;

    let target = temp_dir.path().join("test1.md");
    fs::remove_file(&target).expect("failed to delete test1.md");

    // recreate well within the 500ms grace period
    tokio::time::sleep(Duration::from_millis(50)).await;
    fs::write(&target, "# Test 1 (rewritten)\n\nNew body").expect("failed to recreate test1.md");

    // wait long enough that the deferred-remove would have fired had the file stayed gone
    tokio::time::sleep(Duration::from_millis(DEFERRED_REMOVE_WAIT_MS)).await;

    let response = server.get("/test1.md").await;
    assert_eq!(
        response.status_code(),
        200,
        "atomic-save pattern should leave file tracked"
    );
    let body = response.text();
    assert!(
        body.contains("rewritten") || body.contains("New body"),
        "served content should reflect the recreated file"
    );
}
