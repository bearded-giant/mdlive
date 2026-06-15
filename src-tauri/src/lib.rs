use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use mdlive::AppConfig;
use tauri::menu::{AboutMetadata, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::Manager;

#[cfg(target_os = "macos")]
fn pick_file_or_folder(
    extensions: &[&str],
    start_dir: Option<&std::path::Path>,
) -> Option<PathBuf> {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSOpenPanel;
    use objc2_foundation::{NSString, NSURL};

    let mtm = unsafe { MainThreadMarker::new_unchecked() };

    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(false);
    panel.setResolvesAliases(true);
    panel.setShowsHiddenFiles(true);

    if let Some(dir) = start_dir {
        if dir.is_dir() {
            let path_str = NSString::from_str(&dir.display().to_string());
            let url = NSURL::fileURLWithPath(&path_str);
            panel.setDirectoryURL(Some(&url));
        }
    }

    let ext_strings: Vec<Retained<NSString>> =
        extensions.iter().map(|e| NSString::from_str(e)).collect();
    let refs: Vec<&NSString> = ext_strings.iter().map(|s| &**s).collect();
    let ns_array = objc2_foundation::NSArray::from_slice(&refs);
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&ns_array));

    let result = panel.runModal();
    if result == objc2_app_kit::NSModalResponseOK {
        panel
            .URL()
            .and_then(|url| url.path().map(|p| PathBuf::from(p.to_string())))
    } else {
        None
    }
}

#[cfg(not(target_os = "macos"))]
fn pick_file_or_folder(
    _extensions: &[&str],
    start_dir: Option<&std::path::Path>,
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(dir) = start_dir {
        if dir.is_dir() {
            dialog = dialog.set_directory(dir);
        }
    }
    dialog.pick_folder()
}

static SERVER_PORT: OnceLock<u16> = OnceLock::new();
static RT_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

// tracks which workspace path each window is serving (label → canonical path)
static WINDOW_PATHS: OnceLock<std::sync::Mutex<HashMap<String, String>>> = OnceLock::new();
static APP_QUITTING: AtomicBool = AtomicBool::new(false);

fn register_window_path(label: &str, path: &std::path::Path) {
    if let Some(m) = WINDOW_PATHS.get() {
        if let Ok(mut map) = m.lock() {
            map.insert(
                label.to_string(),
                path.canonicalize()
                    .unwrap_or_else(|_| path.to_path_buf())
                    .display()
                    .to_string(),
            );
        }
    }
    std::thread::spawn(persist_workspaces);
}

fn unregister_window_path(label: &str) {
    if let Some(m) = WINDOW_PATHS.get() {
        if let Ok(mut map) = m.lock() {
            map.remove(label);
        }
    }
    // persist does blocking self-HTTP + file I/O; never run it on the main thread
    std::thread::spawn(persist_workspaces);
}

fn persist_workspaces() {
    let mut workspaces: Vec<String> = Vec::new();

    if let Some(m) = WINDOW_PATHS.get() {
        if let Ok(map) = m.lock() {
            workspaces = map.values().cloned().collect();
        }
    }

    // the daemon's "main" window can switch workspaces via the web UI without
    // creating a new window, so it's never in WINDOW_PATHS — query it directly
    if let Some(&port) = SERVER_PORT.get() {
        if let Some(ws) = query_daemon_workspace(port) {
            if !ws.is_empty() && !workspaces.contains(&ws) {
                workspaces.push(ws);
            }
        }
    }

    let mut config = AppConfig::load();
    config.last_workspaces = workspaces;
    let _ = config.save();
}

fn query_daemon_workspace(port: u16) -> Option<String> {
    use std::io::{Read as _, Write as _};
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut stream =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200)).ok()?;
    // bound the read/write -- without these a stuck daemon would block the caller
    // forever (the close path runs this on the main thread -> beach ball)
    let to = std::time::Duration::from_millis(300);
    stream.set_read_timeout(Some(to)).ok()?;
    stream.set_write_timeout(Some(to)).ok()?;
    let req = format!(
        "GET /api/workspace/current HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).ok()?;
    stream.flush().ok()?;
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).ok()?;
    let resp = String::from_utf8_lossy(&buf[..n]);
    let body = resp.split("\r\n\r\n").nth(1)?;
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json.get("base_dir")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn check_mdlive_server(port: u16) -> bool {
    use std::io::{Read as _, Write as _};
    let request = format!(
        "GET /api/workspace/current HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        std::time::Duration::from_millis(300),
    ) else {
        return false;
    };
    let to = std::time::Duration::from_millis(300);
    let _ = stream.set_read_timeout(Some(to));
    let _ = stream.set_write_timeout(Some(to));
    let _ = stream.write_all(request.as_bytes());
    let _ = stream.flush();
    let mut buf = vec![0u8; 512];
    let _ = stream.read(&mut buf);
    String::from_utf8_lossy(&buf).contains("\"success\"")
}

fn find_existing_server() -> Option<u16> {
    if check_mdlive_server(mdlive::DEFAULT_PORT) {
        return Some(mdlive::DEFAULT_PORT);
    }
    if let Some(port) = mdlive::read_daemon_port() {
        if port != mdlive::DEFAULT_PORT && check_mdlive_server(port) {
            return Some(port);
        }
        // stale port file — previous instance crashed without cleanup
        mdlive::delete_daemon_port();
    }
    None
}

fn start_server_for_path(path: &std::path::Path) -> Result<u16, String> {
    let rt = RT_HANDLE.get().ok_or("runtime not initialized")?;
    let _guard = rt.enter();

    let path = path.canonicalize().map_err(|e| e.to_string())?;
    eprintln!("[mdlive] opening workspace: {}", path.display());

    let (base_dir, tracked_files, is_dir_mode) = if path.is_file() {
        let base = path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        (base, vec![path], false)
    } else if path.is_dir() {
        let files = mdlive::scan_supported_files(&path).map_err(|e| e.to_string())?;
        if files.is_empty() {
            return Err("no supported files in directory".into());
        }
        (path, files, true)
    } else {
        return Err("path is not a file or directory".into());
    };

    eprintln!(
        "[mdlive] building router ({} files)...",
        tracked_files.len()
    );
    let router =
        mdlive::new_router_with_config(base_dir, tracked_files, is_dir_mode, AppConfig::load())
            .map_err(|e| e.to_string())?;

    eprintln!("[mdlive] binding port...");
    rt.block_on(async {
        let (listener, port) = mdlive::bind_with_port_increment("127.0.0.1", mdlive::DEFAULT_PORT)
            .await
            .map_err(|e| e.to_string())?;
        eprintln!("[mdlive] workspace server on port {port}");
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router).await {
                eprintln!("[mdlive] workspace server error: {e}");
            }
        });
        Ok(port)
    })
}

fn create_window(app_handle: &tauri::AppHandle, port: u16, path: &std::path::Path) {
    let url = format!("http://127.0.0.1:{port}");
    let label = format!("win-{port}");
    let title = format!(
        "mdlive - {}",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
    );
    if tauri::WebviewWindowBuilder::new(
        app_handle,
        &label,
        tauri::WebviewUrl::External(url.parse().unwrap()),
    )
    .title(&title)
    .inner_size(1500.0, 1000.0)
    .min_inner_size(600.0, 400.0)
    .build()
    .is_ok()
    {
        register_window_path(&label, path);
    }
}

// fresh window on the daemon picker (?picker forces the selector regardless of
// the daemon's current workspace), independent of any open project windows
fn create_picker_window(app_handle: &tauri::AppHandle, port: u16) {
    let url = format!("http://127.0.0.1:{port}/?picker=1");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let label = format!("picker-{stamp}");
    let _ = tauri::WebviewWindowBuilder::new(
        app_handle,
        &label,
        tauri::WebviewUrl::External(url.parse().unwrap()),
    )
    .title("mdlive")
    .inner_size(1500.0, 1000.0)
    .min_inner_size(600.0, 400.0)
    .build();
}

// start server on a background thread, then create window on main thread
fn open_path_in_window(app_handle: &tauri::AppHandle, path: PathBuf) {
    let handle = app_handle.clone();
    std::thread::spawn(move || match start_server_for_path(&path) {
        Ok(port) => {
            let inner = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                create_window(&inner, port, &path);
            });
        }
        Err(e) => eprintln!("failed to open {}: {e}", path.display()),
    });
}

#[derive(Clone)]
struct WindowOpenTx(std::sync::mpsc::Sender<PathBuf>);

async fn handle_window_new(
    axum::Extension(tx): axum::Extension<WindowOpenTx>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    let path_str = body["path"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return axum::Json(serde_json::json!({"success": false, "error": "path required"}));
    }

    let expanded = if let Some(rest) = path_str.strip_prefix("~/") {
        dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(path_str))
    } else {
        PathBuf::from(path_str)
    };

    if !expanded.exists() {
        return axum::Json(serde_json::json!({"success": false, "error": "path not found"}));
    }

    match tx.0.send(expanded) {
        Ok(_) => axum::Json(serde_json::json!({"success": true})),
        Err(e) => axum::Json(serde_json::json!({"success": false, "error": e.to_string()})),
    }
}

async fn handle_persist_session() -> axum::Json<serde_json::Value> {
    // persist_workspaces makes a blocking self-HTTP call; running it directly on
    // the async worker can starve the runtime (the self-call needs a free worker)
    let _ = tokio::task::spawn_blocking(persist_workspaces).await;
    axum::Json(serde_json::json!({"success": true}))
}

async fn start_server(window_tx: std::sync::mpsc::Sender<PathBuf>) -> Result<u16, String> {
    if let Some(port) = find_existing_server() {
        eprintln!("Reusing existing mdlive server on port {port}");
        return Ok(port);
    }

    let base_router = mdlive::new_daemon_router_with_config(AppConfig::load());
    let router = base_router
        .route("/api/window/new", axum::routing::post(handle_window_new))
        .route(
            "/api/session/persist",
            axum::routing::post(handle_persist_session),
        )
        .layer(axum::Extension(WindowOpenTx(window_tx)));

    let (listener, port) = mdlive::bind_with_port_increment("127.0.0.1", mdlive::DEFAULT_PORT)
        .await
        .map_err(|e| format!("failed to bind server: {e}"))?;

    mdlive::write_daemon_port(port);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("server error: {e}");
        }
    });

    Ok(port)
}

#[tauri::command]
fn get_server_url() -> String {
    let port = SERVER_PORT.get().copied().unwrap_or(mdlive::DEFAULT_PORT);
    format!("http://127.0.0.1:{port}")
}

fn home_prefix() -> String {
    dirs::home_dir()
        .map(|h| h.display().to_string())
        .unwrap_or_default()
}

fn shorten_path(path: &str) -> String {
    let home = home_prefix();
    if !home.is_empty() && path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    }
}

fn format_relative_time(epoch_secs: u64) -> String {
    if epoch_secs == 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let diff = now.saturating_sub(epoch_secs);
    if diff < 60 {
        "now".to_string()
    } else if diff < 3600 {
        format!("{}m", diff / 60)
    } else if diff < 86_400 {
        format!("{}h", diff / 3600)
    } else if diff < 7 * 86_400 {
        format!("{}d", diff / 86_400)
    } else if diff < 30 * 86_400 {
        format!("{}w", diff / (7 * 86_400))
    } else if diff < 365 * 86_400 {
        format!("{}mo", diff / (30 * 86_400))
    } else {
        format!("{}y", diff / (365 * 86_400))
    }
}

fn build_menu(app: &tauri::App) -> tauri::Result<()> {
    let config = AppConfig::load();

    let new_window = MenuItemBuilder::new("New Window")
        .id("new_window")
        .accelerator("CmdOrCtrl+N")
        .build(app)?;

    let open = MenuItemBuilder::new("Open...")
        .id("open")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;

    // recent submenu -- chronological (most recent first), mode glyph + relative timestamp
    let mut recent_builder = SubmenuBuilder::new(app, "Open Recent");

    for entry in &config.recent {
        let glyph = if entry.mode == "directory" {
            "📁"
        } else {
            "📄"
        };
        let path = shorten_path(&entry.path);
        let ts = format_relative_time(entry.last_opened);
        let label = if ts.is_empty() {
            format!("{glyph}  {path}")
        } else {
            format!("{glyph}  {path}  —  {ts}")
        };
        let item = MenuItemBuilder::new(&label)
            .id(format!("recent:{}", entry.path))
            .build(app)?;
        recent_builder = recent_builder.item(&item);
    }

    if !config.recent.is_empty() {
        recent_builder = recent_builder.separator();
        let clear = MenuItemBuilder::new("Clear Recent")
            .id("clear_recent")
            .build(app)?;
        recent_builder = recent_builder.item(&clear);
    }

    let recent_menu = recent_builder.build()?;

    let check_update = MenuItemBuilder::new("Check for Updates...")
        .id("check_update")
        .build(app)?;

    let close_tab = MenuItemBuilder::new("Close Tab")
        .id("close_tab")
        .accelerator("CmdOrCtrl+W")
        .build(app)?;

    let close_window = MenuItemBuilder::new("Close Window")
        .id("close_window_custom")
        .accelerator("Shift+CmdOrCtrl+W")
        .build(app)?;

    let app_menu = SubmenuBuilder::new(app, "mdlive")
        .about(Some(AboutMetadata {
            version: Some(env!("CARGO_PKG_VERSION").into()),
            website: Some("https://github.com/bearded-giant/mdlive".into()),
            website_label: Some("GitHub".into()),
            ..Default::default()
        }))
        .item(&check_update)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&new_window)
        .item(&open)
        .separator()
        .item(&recent_menu)
        .separator()
        .item(&close_tab)
        .item(&close_window)
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "View").fullscreen().build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .build()?;

    let menu = MenuBuilder::new(app)
        .item(&app_menu)
        .item(&file_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&window_menu)
        .build()?;

    app.set_menu(menu)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    RT_HANDLE
        .set(rt.handle().clone())
        .expect("runtime handle already set");

    WINDOW_PATHS
        .set(std::sync::Mutex::new(HashMap::new()))
        .expect("window paths already set");

    let (window_tx, window_rx) = std::sync::mpsc::channel::<PathBuf>();

    let port = match rt.block_on(start_server(window_tx)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("fatal: {e}");
            rfd::MessageDialog::new()
                .set_title("mdlive")
                .set_description(&format!("Could not start server:\n{e}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
            std::process::exit(1);
        }
    };
    SERVER_PORT.set(port).expect("port already set");

    // check for workspaces to restore from last session
    let config = AppConfig::load();
    let mut restore_paths: Vec<PathBuf> = config
        .last_workspaces
        .iter()
        .filter_map(|p| {
            let pb = PathBuf::from(p);
            if pb.exists() {
                Some(pb)
            } else {
                None
            }
        })
        .collect();

    // fallback: reopen the most recently used workspace
    if restore_paths.is_empty() {
        if let Some(recent) = config.recent.first() {
            let pb = PathBuf::from(&recent.path);
            if pb.exists() {
                restore_paths.push(pb);
            }
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .invoke_handler(tauri::generate_handler![get_server_url])
        .setup(move |app| {
            if restore_paths.is_empty() {
                // no previous session — show daemon picker
                let url = format!("http://127.0.0.1:{port}");
                let _ = tauri::WebviewWindowBuilder::new(
                    app,
                    "main",
                    tauri::WebviewUrl::External(url.parse().unwrap()),
                )
                .title("mdlive")
                .inner_size(1500.0, 1000.0)
                .min_inner_size(600.0, 400.0)
                .build()?;
            } else {
                // restore previous workspace windows
                for path in &restore_paths {
                    open_path_in_window(app.handle(), path.clone());
                }
            }

            build_menu(app)?;

            app.on_menu_event(|app_handle, event| {
                let id = event.id().as_ref().to_string();

                if id == "new_window" {
                    let port = SERVER_PORT.get().copied().unwrap_or(mdlive::DEFAULT_PORT);
                    create_picker_window(app_handle, port);
                } else if id == "open" {
                    let start = AppConfig::load()
                        .last_browse_dir
                        .map(PathBuf::from);
                    let picked = pick_file_or_folder(
                        &["md", "markdown", "txt", "json"],
                        start.as_deref(),
                    );
                    if let Some(ref path) = picked {
                        let mut config = AppConfig::load();
                        let dir = if path.is_dir() {
                            path.display().to_string()
                        } else {
                            path.parent()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default()
                        };
                        config.last_browse_dir = Some(dir);
                        let _ = config.save();
                    }
                    if let Some(path) = picked {
                        open_path_in_window(app_handle, path);
                    }
                } else if let Some(path) = id.strip_prefix("recent:") {
                    open_path_in_window(app_handle, PathBuf::from(path));
                } else if id == "clear_recent" {
                    let mut config = AppConfig::load();
                    config.recent.clear();
                    let _ = config.save();
                } else if id == "check_update" {
                    std::thread::spawn(check_for_updates);
                } else if id == "close_tab" {
                    for (_, win) in app_handle.webview_windows() {
                        if win.is_focused().unwrap_or(false) {
                            let _ = win.eval(
                                "if (window.mdliveCloseActiveTab) { window.mdliveCloseActiveTab(); } else { window.close(); }",
                            );
                            break;
                        }
                    }
                } else if id == "close_window_custom" {
                    for (_, win) in app_handle.webview_windows() {
                        if win.is_focused().unwrap_or(false) {
                            let _ = win.close();
                            break;
                        }
                    }
                }
            });

            // handle window open requests from CLI via /api/window/new
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                while let Ok(path) = window_rx.recv() {
                    // start server on background thread (needs tokio runtime context)
                    match start_server_for_path(&path) {
                        Ok(port) => {
                            let handle = app_handle.clone();
                            let _ = app_handle.run_on_main_thread(move || {
                                create_window(&handle, port, &path);
                            });
                        }
                        Err(e) => eprintln!("failed to open {}: {e}", path.display()),
                    }
                }
            });

            // keep tokio runtime alive for the lifetime of the app
            std::mem::forget(rt);

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            tauri::RunEvent::ExitRequested {
                api,
                code,
                ..
            } => {
                if code.is_none() {
                    // last window closed — keep app alive (macOS convention)
                    api.prevent_exit();
                } else {
                    // explicit quit (Cmd+Q or process signal)
                    APP_QUITTING.store(true, Ordering::SeqCst);
                    persist_workspaces();
                }
            }
            tauri::RunEvent::WindowEvent { label, event, .. } => {
                if matches!(event, tauri::WindowEvent::Destroyed)
                    && !APP_QUITTING.load(Ordering::SeqCst)
                {
                    unregister_window_path(&label);
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } => {
                if !has_visible_windows {
                    let port = SERVER_PORT.get().copied().unwrap_or(mdlive::DEFAULT_PORT);
                    let url = format!("http://127.0.0.1:{port}");
                    let _ = tauri::WebviewWindowBuilder::new(
                        app_handle,
                        format!("main-{}", std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis()),
                        tauri::WebviewUrl::External(url.parse().unwrap()),
                    )
                    .title("mdlive")
                    .inner_size(1500.0, 1000.0)
                    .min_inner_size(600.0, 400.0)
                    .build();
                }
            }
            tauri::RunEvent::Opened { urls } => {
                for url in urls {
                    if url.scheme() == "file" {
                        if let Ok(path) = url.to_file_path() {
                            open_path_in_window(app_handle, path);
                        }
                    }
                }
            }
            _ => {}
        });
}

fn parse_version(v: &str) -> (u32, u32, u32) {
    let parts: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
}

fn check_for_updates() {
    let current = env!("CARGO_PKG_VERSION");

    let output = match std::process::Command::new("curl")
        .args([
            "-s",
            "-H",
            "User-Agent: mdlive-update-check",
            "https://api.github.com/repos/bearded-giant/mdlive/releases/latest",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => {
            rfd::MessageDialog::new()
                .set_title("Update Check")
                .set_description("Could not reach GitHub.")
                .set_level(rfd::MessageLevel::Error)
                .show();
            return;
        }
    };

    let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => {
            rfd::MessageDialog::new()
                .set_title("Update Check")
                .set_description("Could not parse response.")
                .set_level(rfd::MessageLevel::Error)
                .show();
            return;
        }
    };

    let latest_tag = json["tag_name"].as_str().unwrap_or("");
    let latest = latest_tag.strip_prefix('v').unwrap_or(latest_tag);

    if parse_version(latest) > parse_version(current) {
        let release_url = json["html_url"].as_str().unwrap_or("").to_string();
        let result = rfd::MessageDialog::new()
            .set_title("Update Available")
            .set_description(&format!(
                "v{latest} is available (you have v{current}).\n\n\
                 Update with:\n  brew upgrade --cask bearded-giant/tap/mdlive-app"
            ))
            .set_level(rfd::MessageLevel::Info)
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show();
        if result == rfd::MessageDialogResult::Ok && !release_url.is_empty() {
            let _ = std::process::Command::new("open").arg(&release_url).spawn();
        }
    } else {
        rfd::MessageDialog::new()
            .set_title("Up to Date")
            .set_description(&format!("You're running the latest version (v{current})."))
            .set_level(rfd::MessageLevel::Info)
            .show();
    }
}
