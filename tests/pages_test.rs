mod common;

use common::*;
use mdlive::{new_router, scan_supported_files};
use std::fs;
use tempfile::tempdir;

use axum_test::TestServer;

#[tokio::test]
async fn test_server_starts_and_serves_basic_markdown() {
    let (server, _, _dir) = create_test_server("# Hello World\n\nThis is **bold** text.").await;

    let response = server.get("/").await;

    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(body.contains("<h1>Hello World</h1>"));
    assert!(body.contains("<strong>bold</strong>"));
    assert!(body.contains("theme-toggle"));
    assert!(body.contains("openThemeModal"));
    assert!(body.contains("--bg-color"));
    assert!(body.contains("data-theme=\"dark\""));
}

#[tokio::test]
async fn test_server_handles_gfm_features() {
    let markdown_content = r#"# GFM Test

## Table
| Name | Age |
|------|-----|
| John | 30  |
| Jane | 25  |

## Strikethrough
~~deleted text~~

## Code block
```rust
fn main() {
    println!("Hello!");
}
```
"#;

    let (server, _, _dir) = create_test_server(markdown_content).await;

    let response = server.get("/").await;

    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(body.contains("<table>"));
    assert!(body.contains("<th>Name</th>"));
    assert!(body.contains("<td>John</td>"));
    assert!(body.contains("<del>deleted text</del>"));
    assert!(body.contains("<pre>"));
    assert!(body.contains("fn main()"));
}

#[tokio::test]
async fn test_404_for_unknown_routes() {
    let (server, _, _dir) = create_test_server("# 404 Test").await;

    let response = server.get("/unknown-route").await;

    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn test_html_tags_in_markdown_are_rendered() {
    let markdown_content = r#"# HTML Test

This markdown contains HTML tags:

<div class="highlight">
    <p>This should be rendered as HTML, not escaped</p>
    <span style="color: red;">Red text</span>
</div>

Regular **markdown** still works.
"#;

    let (server, _, _dir) = create_test_server(markdown_content).await;

    let response = server.get("/").await;

    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(body.contains(r#"<div class="highlight">"#));
    assert!(body.contains(r#"<span style="color: red;">"#));
    assert!(body.contains("<p>This should be rendered as HTML, not escaped</p>"));
    assert!(!body.contains("&lt;div"));
    assert!(!body.contains("&gt;"));
    assert!(body.contains("<strong>markdown</strong>"));
}

#[tokio::test]
async fn test_yaml_frontmatter_is_stripped() {
    let (server, _, _dir) = create_test_server(YAML_FRONTMATTER_CONTENT).await;

    let response = server.get("/").await;

    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(!body.contains("title: Test Post"));
    assert!(!body.contains("author: Name"));
    assert!(body.contains("<h1>Test Post</h1>"));
}

#[tokio::test]
async fn test_toml_frontmatter_is_stripped() {
    let (server, _, _dir) = create_test_server(TOML_FRONTMATTER_CONTENT).await;

    let response = server.get("/").await;

    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(!body.contains("title = \"Test Post\""));
    assert!(body.contains("<h1>Test Post</h1>"));
}

#[tokio::test]
async fn test_image_serving() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    let md_content =
        "# Test with Image\n\n![Test Image](test.png)\n\nThis markdown references an image.";
    let md_path = temp_dir.path().join("test.md");
    fs::write(&md_path, md_content).expect("Failed to write markdown file");

    let png_data = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0x0F, 0x00, 0x00, 0x01, 0x00, 0x01, 0x5C, 0xDD, 0x8D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let img_path = temp_dir.path().join("test.png");
    fs::write(&img_path, png_data).expect("Failed to write image file");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = vec![md_path];
    let router = new_router(base_dir, tracked_files, false).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    let response = server.get("/").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();
    assert!(body.contains("<img src=\"test.png\" alt=\"Test Image\""));

    let img_response = server.get("/test.png").await;
    assert_eq!(img_response.status_code(), 200);
    assert_eq!(img_response.header("content-type"), "image/png");
    assert!(!img_response.as_bytes().is_empty());
}

#[tokio::test]
async fn test_non_image_files_not_served() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    let md_path = temp_dir.path().join("test.md");
    fs::write(&md_path, "# Test").expect("Failed to write markdown file");

    let txt_path = temp_dir.path().join("secret.txt");
    fs::write(&txt_path, "secret content").expect("Failed to write txt file");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = vec![md_path];
    let router = new_router(base_dir, tracked_files, false).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    let response = server.get("/secret.txt").await;
    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn test_subdirectory_image_serving() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    let md_content = "# Test\n\n![Sub Image](subdir/test.png)\n![Deep Image](a/b/deep.png)";
    let md_path = temp_dir.path().join("test.md");
    fs::write(&md_path, md_content).expect("Failed to write markdown file");

    let png_data: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0x0F, 0x00, 0x00, 0x01, 0x00, 0x01, 0x5C, 0xDD, 0x8D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    let sub_dir = temp_dir.path().join("subdir");
    fs::create_dir(&sub_dir).expect("Failed to create subdir");
    fs::write(sub_dir.join("test.png"), &png_data).expect("Failed to write subdir image");

    let deep_dir = temp_dir.path().join("a").join("b");
    fs::create_dir_all(&deep_dir).expect("Failed to create deep dir");
    fs::write(deep_dir.join("deep.png"), &png_data).expect("Failed to write deep image");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = vec![md_path];
    let router = new_router(base_dir, tracked_files, false).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    let response = server.get("/subdir/test.png").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("content-type"), "image/png");

    let response = server.get("/a/b/deep.png").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("content-type"), "image/png");
}

#[tokio::test]
async fn test_directory_traversal_blocked() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    let md_path = temp_dir.path().join("test.md");
    fs::write(&md_path, "# Test").expect("Failed to write markdown file");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = vec![md_path];
    let router = new_router(base_dir, tracked_files, false).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    let response = server.get("/../../../etc/passwd").await;
    assert_ne!(response.status_code(), 200);
}

#[tokio::test]
async fn test_same_dir_image_still_works_with_wildcard_route() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    let md_path = temp_dir.path().join("test.md");
    fs::write(&md_path, "# Test\n\n![img](photo.jpg)").expect("Failed to write md");

    let jpg_data = vec![0xFF, 0xD8, 0xFF, 0xE0];
    fs::write(temp_dir.path().join("photo.jpg"), &jpg_data).expect("Failed to write jpg");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = vec![md_path];
    let router = new_router(base_dir, tracked_files, false).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    let response = server.get("/photo.jpg").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("content-type"), "image/jpeg");
}

#[tokio::test]
async fn test_single_file_mode_no_navigation_sidebar() {
    let (server, _, _dir) = create_test_server("# Single File Test").await;

    let response = server.get("/").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(!body.contains(r#"<nav class="sidebar">"#));
    assert!(!body.contains("<h3>Files</h3>"));
    assert!(!body.contains(r#"<ul class="file-tree">"#));
}

// --- directory mode tests ---

#[tokio::test]
async fn test_directory_mode_serves_multiple_files() {
    let (server, _temp_dir) = create_directory_server().await;

    let response1 = server.get("/test1.md").await;
    assert_eq!(response1.status_code(), 200);
    let body1 = response1.text();
    assert!(body1.contains("<h1>Test 1</h1>"));
    assert!(body1.contains("Content of test1"));

    let response2 = server.get("/test2.markdown").await;
    assert_eq!(response2.status_code(), 200);
    let body2 = response2.text();
    assert!(body2.contains("<h1>Test 2</h1>"));
    assert!(body2.contains("Content of test2"));

    let response3 = server.get("/test3.md").await;
    assert_eq!(response3.status_code(), 200);
    let body3 = response3.text();
    assert!(body3.contains("<h1>Test 3</h1>"));
    assert!(body3.contains("Content of test3"));
}

#[tokio::test]
async fn test_directory_mode_file_not_found() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/nonexistent.md").await;
    assert_eq!(response.status_code(), 404);
}

#[tokio::test]
async fn test_directory_mode_has_navigation_sidebar() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/test1.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(body.contains(r#"<nav class="sidebar">"#));
    assert!(body.contains(r#"class="file-tree"#));
    assert!(body.contains("test1.md"));
    assert!(body.contains("test2.markdown"));
    assert!(body.contains("test3.md"));
}

#[tokio::test]
async fn test_directory_mode_active_file_highlighting() {
    let (server, _temp_dir) = create_directory_server().await;

    let response1 = server.get("/test1.md").await;
    assert_eq!(response1.status_code(), 200);
    let body1 = response1.text();

    assert!(
        body1.contains(r#"href="/test1.md""#) && body1.contains(r#"class="active""#),
        "test1.md link should have href and active class"
    );

    let active_link_count = body1.matches(r#"class="active""#).count();
    assert_eq!(active_link_count, 1, "Should have exactly one active link");

    let response2 = server.get("/test2.markdown").await;
    assert_eq!(response2.status_code(), 200);
    let body2 = response2.text();

    assert!(
        body2.contains(r#"href="/test2.markdown""#) && body2.contains(r#"class="active""#),
        "test2.markdown link should have href and active class"
    );
}

#[tokio::test]
async fn test_directory_mode_file_order() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/test1.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    let test1_pos = body.find("test1.md").expect("test1.md not found");
    let test2_pos = body
        .find("test2.markdown")
        .expect("test2.markdown not found");
    let test3_pos = body.find("test3.md").expect("test3.md not found");

    assert!(
        test1_pos < test2_pos,
        "test1.md should appear before test2.markdown"
    );
    assert!(
        test2_pos < test3_pos,
        "test2.markdown should appear before test3.md"
    );
}

#[tokio::test]
async fn test_directory_mode_serves_nested_files() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    fs::write(temp_dir.path().join("root.md"), "# Root File").expect("Failed to write");

    let docs_dir = temp_dir.path().join("docs");
    fs::create_dir(&docs_dir).expect("Failed to create docs dir");
    fs::write(docs_dir.join("guide.md"), "# Guide\n\nGuide content").expect("Failed to write");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_supported_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    let response = server.get("/docs/guide.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();
    assert!(body.contains("<h1>Guide</h1>"));
    assert!(body.contains("Guide content"));
}

#[tokio::test]
async fn test_directory_mode_tree_sidebar() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    fs::write(temp_dir.path().join("readme.md"), "# Readme").expect("Failed to write");

    let docs_dir = temp_dir.path().join("docs");
    fs::create_dir(&docs_dir).expect("Failed to create docs dir");
    fs::write(docs_dir.join("guide.md"), "# Guide").expect("Failed to write");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_supported_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    let response = server.get("/readme.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(body.contains(r#"class="file-tree"#));
    assert!(body.contains("<details"));
    assert!(body.contains("<summary>"));
    assert!(body.contains("docs"));
    assert!(body.contains("guide.md"));
}

#[tokio::test]
async fn test_tree_links_carry_modified_timestamps() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("readme.md"), "# Readme").expect("Failed to write");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_supported_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    let response = server.get("/readme.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    let needle = r#"href="/readme.md" data-modified=""#;
    let idx = body
        .find(needle)
        .expect("readme.md link should expose data-modified");
    let after = &body[idx + needle.len()..];
    let end = after.find('"').expect("data-modified should be quoted");
    let value: u64 = after[..end]
        .parse()
        .expect("data-modified should be a unix epoch integer");
    assert!(value > 0, "data-modified must reflect file mtime");
}

#[tokio::test]
async fn test_tree_modified_reflects_fresh_disk_mtime() {
    use std::thread::sleep;
    use std::time::Duration;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let file_path = temp_dir.path().join("readme.md");
    fs::write(&file_path, "# Readme").expect("Failed to write");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_supported_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    fn extract_modified(body: &str) -> u64 {
        let needle = r#"href="/readme.md" data-modified=""#;
        let idx = body.find(needle).expect("link with data-modified");
        let after = &body[idx + needle.len()..];
        let end = after.find('"').expect("quoted attr");
        after[..end].parse().expect("epoch int")
    }

    let initial = extract_modified(&server.get("/readme.md").await.text());

    // simulate an external edit the watcher might miss. sleep past the 1s
    // mtime resolution so the new write produces a distinguishable epoch.
    sleep(Duration::from_millis(1100));
    fs::write(&file_path, "# Readme updated").expect("Failed to write");

    let refreshed = extract_modified(&server.get("/readme.md").await.text());
    assert!(
        refreshed > initial,
        "tree should reflect fresh disk mtime: initial={initial}, refreshed={refreshed}"
    );
}

#[tokio::test]
async fn test_nested_file_active_highlighting() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    fs::write(temp_dir.path().join("root.md"), "# Root").expect("Failed to write");

    let docs_dir = temp_dir.path().join("docs");
    fs::create_dir(&docs_dir).expect("Failed to create docs dir");
    fs::write(docs_dir.join("guide.md"), "# Guide").expect("Failed to write");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_supported_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    let response = server.get("/docs/guide.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        body.contains(r#"href="/docs/guide.md""#) && body.contains(r#"class="active""#),
        "nested file should have active class"
    );

    let active_count = body.matches(r#"class="active""#).count();
    assert_eq!(active_count, 1, "Should have exactly one active link");
}

#[tokio::test]
async fn test_context_menu_elements_in_directory_mode() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/test1.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(body.contains(r#"id="contextMenu""#));
    assert!(body.contains(r#"id="fileDialog""#));
    assert!(body.contains("initContextMenu"));
    assert!(body.contains("var currentFile"));
    assert!(body.contains("data-dir-path"));
}

#[tokio::test]
async fn test_context_menu_in_single_file_mode() {
    let (server, _, _dir) = create_test_server("# Single").await;

    let response = server.get("/").await;
    let body = response.text();

    // context menu is available in single-file mode for edit and copy operations
    assert!(body.contains(r#"id="contextMenu""#));
    assert!(body.contains(r#"id="fileDialog""#));
}

// --- mermaid tests ---

#[tokio::test]
async fn test_mermaid_diagram_detection_and_script_injection() {
    let markdown_content = r#"# Mermaid Test

Regular content here.

```mermaid
graph TD
    A[Start] --> B{Decision}
    B -->|Yes| C[End]
    B -->|No| D[Continue]
```

More regular content.

```javascript
// This is a regular code block, not mermaid
console.log("Hello World");
```
"#;

    let (server, _, _dir) = create_test_server(markdown_content).await;

    let response = server.get("/").await;

    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(body.contains(r#"class="language-mermaid""#));
    assert!(body.contains("graph TD"));

    let has_raw_content = body.contains("A[Start] --> B{Decision}");
    let has_encoded_content = body.contains("A[Start] --&gt; B{Decision}");
    assert!(
        has_raw_content || has_encoded_content,
        "Expected mermaid content not found in body"
    );

    assert!(body.contains(r#"<script src="/mermaid.min.js"></script>"#));
    assert!(body.contains("function initMermaid()"));
    assert!(body.contains("function transformMermaidCodeBlocks()"));
    assert!(body.contains("function getMermaidTheme()"));
    assert!(body.contains(r#"class="language-javascript""#));
    assert!(body.contains("console.log"));
}

#[tokio::test]
async fn test_no_mermaid_script_injection_without_mermaid_blocks() {
    let markdown_content = r#"# No Mermaid Test

This content has no mermaid diagrams.

```javascript
console.log("Hello World");
```

```bash
echo "Regular code block"
```

Just regular markdown content.
"#;

    let (server, _, _dir) = create_test_server(markdown_content).await;

    let response = server.get("/").await;

    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(!body.contains(r#"<script src="https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.min.js"></script>"#));
    assert!(body.contains("function initMermaid()"));
    assert!(body.contains(r#"class="language-javascript""#));
    assert!(body.contains(r#"class="language-bash""#));
}

#[tokio::test]
async fn test_multiple_mermaid_diagrams() {
    let markdown_content = r#"# Multiple Mermaid Diagrams

## Flowchart
```mermaid
graph LR
    A --> B
```

## Sequence Diagram
```mermaid
sequenceDiagram
    Alice->>Bob: Hello
    Bob-->>Alice: Hi
```

## Class Diagram
```mermaid
classDiagram
    Animal <|-- Duck
```
"#;

    let (server, _, _dir) = create_test_server(markdown_content).await;

    let response = server.get("/").await;

    assert_eq!(response.status_code(), 200);
    let body = response.text();

    let mermaid_occurrences = body.matches(r#"class="language-mermaid""#).count();
    assert_eq!(mermaid_occurrences, 3);

    assert!(body.contains("graph LR"));
    assert!(body.contains("sequenceDiagram"));
    assert!(body.contains("classDiagram"));

    assert!(body.contains("A --&gt; B") || body.contains("A --> B"));
    assert!(body.contains("Alice-&gt;&gt;Bob") || body.contains("Alice->>Bob"));
    assert!(body.contains("Animal &lt;|-- Duck") || body.contains("Animal <|-- Duck"));

    let script_occurrences = body
        .matches(r#"<script src="/mermaid.min.js"></script>"#)
        .count();
    assert_eq!(script_occurrences, 1);
}

#[tokio::test]
async fn test_mermaid_js_etag_caching() {
    let (server, _, _dir) = create_test_server("# Test").await;

    let response = server.get("/mermaid.min.js").await;
    assert_eq!(response.status_code(), 200);

    let etag = response.header("etag");
    assert!(!etag.is_empty(), "ETag header should be present");

    let cache_control = response.header("cache-control");
    let cache_control_str = cache_control.to_str().unwrap();
    assert!(cache_control_str.contains("public"));
    assert!(cache_control_str.contains("no-cache"));

    let content_type = response.header("content-type");
    assert_eq!(content_type, "application/javascript");

    assert!(!response.as_bytes().is_empty());

    let response_304 = server
        .get("/mermaid.min.js")
        .add_header(
            axum::http::header::IF_NONE_MATCH,
            axum::http::HeaderValue::from_str(etag.to_str().unwrap()).unwrap(),
        )
        .await;

    assert_eq!(response_304.status_code(), 304);
    assert_eq!(response_304.header("etag"), etag);
    assert!(response_304.as_bytes().is_empty());

    let response_200 = server
        .get("/mermaid.min.js")
        .add_header(
            axum::http::header::IF_NONE_MATCH,
            axum::http::HeaderValue::from_static("\"different-etag\""),
        )
        .await;

    assert_eq!(response_200.status_code(), 200);
    assert!(!response_200.as_bytes().is_empty());
}

// --- content width tests ---

#[tokio::test]
async fn test_content_width_css_presets_present() {
    let (server, _, _dir) = create_test_server("# Width CSS Test").await;

    let response = server.get("/").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        body.contains(r#"[data-width="narrow"]"#),
        "narrow data-width selector should be present"
    );
    assert!(
        body.contains("--content-max-width: 680px"),
        "narrow CSS variable value should be 680px"
    );
    assert!(
        body.contains(r#"[data-width="wide"]"#),
        "wide data-width selector should be present"
    );
    assert!(
        body.contains("--content-max-width: 1200px"),
        "wide CSS variable value should be 1200px"
    );
    assert!(
        body.contains(r#"[data-width="full"]"#),
        "full data-width selector should be present"
    );
    assert!(
        body.contains("--content-max-width: 100%"),
        "full CSS variable value should be 100%"
    );
}

#[tokio::test]
async fn test_content_width_modal_ui() {
    let (server, _, _dir) = create_test_server("# Width Modal Test").await;

    let response = server.get("/").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        body.contains("Content Width"),
        "Appearance modal should have a Content Width heading"
    );

    // Width card labels
    assert!(body.contains("Narrow"), "Narrow label should be present");
    assert!(body.contains("Default"), "Default label should be present");
    assert!(body.contains("Wide"), "Wide label should be present");
    assert!(body.contains("Full"), "Full label should be present");

    // onclick handlers on width cards
    assert!(
        body.contains("onclick=\"setContentWidth('narrow')\""),
        "narrow onclick should be present"
    );
    assert!(
        body.contains("onclick=\"setContentWidth('default')\""),
        "default onclick should be present"
    );
    assert!(
        body.contains("onclick=\"setContentWidth('wide')\""),
        "wide onclick should be present"
    );
    assert!(
        body.contains("onclick=\"setContentWidth('full')\""),
        "full onclick should be present"
    );

    // Pixel / percentage values shown in cards
    assert!(
        body.contains("680px"),
        "680px value should be shown in width card"
    );
    assert!(
        body.contains("900px"),
        "900px value should be shown in width card"
    );
    assert!(
        body.contains("1200px"),
        "1200px value should be shown in width card"
    );
    assert!(
        body.contains("100%"),
        "100% value should be shown in width card"
    );
    assert!(
        body.contains("grid-template-columns: repeat(4, 1fr)"),
        "width grid should use 4-column layout"
    );
}

#[tokio::test]
async fn test_content_width_js_functions() {
    let (server, _, _dir) = create_test_server("# Width JS Test").await;

    let response = server.get("/").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        body.contains("function setContentWidth"),
        "setContentWidth function should be present"
    );
    assert!(
        body.contains("function cycleContentWidth"),
        "cycleContentWidth function should be present"
    );
    assert!(
        body.contains("function updateWidthSelection"),
        "updateWidthSelection function should be present"
    );
    assert!(
        body.contains("WIDTH_PRESETS"),
        "WIDTH_PRESETS array should be present"
    );
}

#[tokio::test]
async fn test_content_width_early_script_fouc_prevention() {
    let (server, _, _dir) = create_test_server("# FOUC Prevention Test").await;

    let response = server.get("/").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        body.contains(r#"localStorage.getItem("content-width")"#),
        "early script should read content-width from localStorage"
    );
    assert!(
        body.contains("validWidths"),
        "early script should have a validWidths whitelist"
    );
    assert!(
        body.contains("'full'"),
        "early script validWidths should contain 'full'"
    );
    assert!(
        body.contains(r#"setAttribute("data-width""#),
        "early script should set data-width attribute"
    );
}

#[tokio::test]
async fn test_content_width_keyboard_shortcut() {
    let (server, _, _dir) = create_test_server("# Keyboard Shortcut Test").await;

    let response = server.get("/").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        body.contains("Shift+W"),
        "Shift+W shortcut key label should be in the shortcuts modal"
    );
    assert!(
        body.contains("Cycle content width"),
        "Cycle content width description should be in the shortcuts modal"
    );
}

#[tokio::test]
async fn test_content_width_works_in_directory_mode() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/test1.md").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        body.contains(r#"[data-width="narrow"]"#),
        "narrow CSS preset should be present in directory mode"
    );
    assert!(
        body.contains("--content-max-width: 680px"),
        "narrow width value should be present in directory mode"
    );
    assert!(
        body.contains(r#"[data-width="wide"]"#),
        "wide CSS preset should be present in directory mode"
    );
    assert!(
        body.contains("--content-max-width: 1200px"),
        "wide width value should be present in directory mode"
    );
    assert!(
        body.contains(r#"[data-width="full"]"#),
        "full CSS preset should be present in directory mode"
    );
    assert!(
        body.contains("Content Width"),
        "Content Width modal heading should be present in directory mode"
    );
    assert!(
        body.contains("onclick=\"setContentWidth('narrow')\""),
        "width card onclick should be present in directory mode"
    );
}

#[tokio::test]
async fn test_content_width_single_file_uses_css_variable() {
    let (server, _, _dir) = create_test_server("# CSS Variable Test").await;

    let response = server.get("/").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    assert!(
        body.contains("max-width: var(--content-max-width)"),
        ".page-container should use var(--content-max-width) for max-width"
    );
    // Ensure the old hardcoded percentage width is not used on .page-container
    assert!(
        !body.contains("width: 75%"),
        ".page-container should not use a hardcoded width: 75%"
    );
}
