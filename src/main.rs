use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use mdlive::{
    delete_daemon_port, read_daemon_port, scan_supported_files, serve_daemon, serve_markdown,
};

#[derive(Parser)]
#[command(name = "mdlive")]
#[command(about = "Markdown workspace server for AI coding agents")]
#[command(version = env!("MDLIVE_VERSION"))]
struct Cli {
    /// Path to markdown file or directory to serve (omit for daemon mode)
    path: Option<PathBuf>,

    /// Hostname (domain or IP address) to listen on
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    hostname: String,

    /// Port to serve on
    #[arg(short, long, default_value_t = mdlive::DEFAULT_PORT)]
    port: u16,

    /// Don't open the preview in the default browser
    #[arg(long)]
    no_open: bool,

    /// Open in browser instead of the desktop app
    #[arg(short, long)]
    browser: bool,

    /// Hand off to a running instance instead of starting a new one
    #[arg(short, long)]
    join: bool,

    /// Verbose logging for debugging handoff and server startup
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Manage the mdlive LaunchAgent service
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Install LaunchAgent for auto-start on login
    Install,
    /// Remove LaunchAgent
    Uninstall,
    /// Start the daemon
    Start,
    /// Stop the running daemon
    Stop,
    /// Check if the service is running
    Status,
}

const LAUNCHD_LABEL: &str = "com.beardedgiant.mdlive";

fn get_uid() -> String {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "501".to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(Command::Service { action }) = cli.command {
        return handle_service(action, &cli.hostname, cli.port);
    }

    let verbose = cli.verbose;

    match cli.path {
        Some(path) => {
            let absolute_path = path.canonicalize().unwrap_or(path);

            if cli.join {
                if verbose {
                    eprintln!("[verbose] --join flag set, trying handoff");
                    eprintln!("[verbose] trying handoff to port {}", cli.port);
                }

                if try_handoff(&absolute_path, cli.port, verbose) {
                    return Ok(());
                }

                if verbose {
                    eprintln!("[verbose] handoff failed, trying app launch");
                }

                if try_launch_app_handoff(&absolute_path, cli.port, verbose) {
                    return Ok(());
                }

                if verbose {
                    eprintln!("[verbose] no running instance found, starting standalone");
                }
            } else if !cli.browser {
                // default: try opening in desktop app
                if verbose {
                    eprintln!("[verbose] trying to open in app window");
                }

                if try_window_open(&absolute_path, cli.port, verbose) {
                    eprintln!("Opened in mdlive: {}", absolute_path.display());
                    return Ok(());
                }

                if verbose {
                    eprintln!("[verbose] no running app, trying to launch");
                }

                if try_launch_app_window(&absolute_path, cli.port, verbose) {
                    return Ok(());
                }

                if verbose {
                    eprintln!("[verbose] no app found, falling back to browser");
                }
            } else if verbose {
                eprintln!("[verbose] --browser flag, starting standalone server");
            }

            let (base_dir, tracked_files, is_directory_mode) = if absolute_path.is_file() {
                let base_dir = absolute_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .to_path_buf();
                let tracked_files = vec![absolute_path];
                (base_dir, tracked_files, false)
            } else if absolute_path.is_dir() {
                let tracked_files = scan_supported_files(&absolute_path)?;
                if tracked_files.is_empty() {
                    anyhow::bail!("No supported files found in directory (.md, .txt, .json, .csv, .yaml, .yml, .toml)");
                }
                (absolute_path, tracked_files, true)
            } else {
                anyhow::bail!("Path must be a file or directory");
            };

            serve_markdown(
                base_dir,
                tracked_files,
                is_directory_mode,
                cli.hostname,
                cli.port,
                !cli.no_open,
            )
            .await?;
        }
        None => {
            serve_daemon(cli.hostname, cli.port, !cli.no_open).await?;
        }
    }

    Ok(())
}

// -- window open (default): POST /api/window/new to running Tauri app --

fn try_window_open(path: &std::path::Path, port: u16, verbose: bool) -> bool {
    if verbose {
        eprintln!("[verbose] try_window_open: port {port}");
    }
    if try_window_open_on_port(path, port, verbose) {
        return true;
    }

    if let Some(daemon_port) = read_daemon_port() {
        if verbose {
            eprintln!("[verbose] portfile says {daemon_port}");
        }
        if daemon_port != port && try_window_open_on_port(path, daemon_port, verbose) {
            return true;
        }
    } else if verbose {
        eprintln!("[verbose] no portfile found");
    }

    false
}

fn try_window_open_on_port(path: &std::path::Path, port: u16, verbose: bool) -> bool {
    use std::io::{Read as _, Write as _};

    let path_str = path.display().to_string();
    let body = serde_json::json!({ "path": path_str }).to_string();
    let request = format!(
        "POST /api/window/new HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );

    let addr = format!("127.0.0.1:{port}");
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(
        &addr.parse().unwrap(),
        std::time::Duration::from_millis(500),
    ) else {
        if verbose {
            eprintln!("[verbose] connect to {addr} failed");
        }
        return false;
    };

    if verbose {
        eprintln!("[verbose] connected to {addr}, sending window/new request");
    }

    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.flush();

    let mut buf = vec![0u8; 1024];
    let _ = stream.read(&mut buf);
    let response = String::from_utf8_lossy(&buf);

    if verbose {
        let first_line = response.lines().next().unwrap_or("");
        eprintln!("[verbose] window/new response: {first_line}");
    }

    response.contains("\"success\":true")
}

fn try_launch_app_window(path: &std::path::Path, port: u16, verbose: bool) -> bool {
    if !app_exists() {
        if verbose {
            eprintln!("[verbose] no mdlive.app found");
        }
        return false;
    }

    eprintln!("Launching mdlive app...");
    if std::process::Command::new("open")
        .args(["-a", "mdlive"])
        .status()
        .is_err()
    {
        return false;
    }

    for i in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if verbose && i % 10 == 0 && i > 0 {
            eprintln!("[verbose] polling... {i}/50");
        }
        if try_window_open(path, port, verbose) {
            eprintln!("Opened in mdlive: {}", path.display());
            return true;
        }
    }

    eprintln!("App launched but could not open window");
    false
}

// -- join mode: hand off to running instance via /api/workspace/switch --

fn try_handoff(path: &std::path::Path, port: u16, verbose: bool) -> bool {
    if verbose {
        eprintln!("[verbose] try_handoff: port {port}");
    }
    if try_handoff_on_port(path, port, verbose) {
        return true;
    }

    if let Some(daemon_port) = read_daemon_port() {
        if verbose {
            eprintln!("[verbose] portfile says {daemon_port}");
        }
        if daemon_port != port && try_handoff_on_port(path, daemon_port, verbose) {
            return true;
        }
    } else if verbose {
        eprintln!("[verbose] no portfile found");
    }

    false
}

fn try_handoff_on_port(path: &std::path::Path, port: u16, verbose: bool) -> bool {
    use std::io::{Read as _, Write as _};

    let path_str = path.display().to_string();
    let body = serde_json::json!({ "path": path_str }).to_string();
    let request = format!(
        "POST /api/workspace/switch HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );

    let addr = format!("127.0.0.1:{port}");
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(
        &addr.parse().unwrap(),
        std::time::Duration::from_millis(500),
    ) else {
        if verbose {
            eprintln!("[verbose] connect to {addr} failed");
        }
        return false;
    };

    if verbose {
        eprintln!("[verbose] connected to {addr}, sending switch request");
    }

    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.flush();

    let mut buf = vec![0u8; 1024];
    let _ = stream.read(&mut buf);
    let response = String::from_utf8_lossy(&buf);

    if verbose {
        let first_line = response.lines().next().unwrap_or("");
        eprintln!("[verbose] response: {first_line}");
    }

    if !response.contains("\"success\":true") {
        if verbose {
            eprintln!(
                "[verbose] switch failed: {}",
                response.trim_matches('\0').trim()
            );
        }
        return false;
    }

    eprintln!("Switched running mdlive instance to: {}", path.display());

    let _ = std::process::Command::new("open")
        .args(["-a", "mdlive"])
        .status();

    true
}

fn try_launch_app_handoff(path: &std::path::Path, port: u16, verbose: bool) -> bool {
    if !app_exists() {
        if verbose {
            eprintln!("[verbose] no mdlive.app found");
        }
        return false;
    }

    eprintln!("Launching mdlive app...");
    if std::process::Command::new("open")
        .args(["-a", "mdlive"])
        .status()
        .is_err()
    {
        return false;
    }

    for i in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if verbose && i % 10 == 0 && i > 0 {
            eprintln!("[verbose] polling... {i}/50");
        }
        if try_handoff(path, port, verbose) {
            return true;
        }
    }

    eprintln!("App launched but server did not respond in time");
    false
}

// -- shared helpers --

fn app_exists() -> bool {
    std::path::Path::new("/Applications/mdlive.app").exists()
        || dirs::home_dir()
            .map(|h| h.join("Applications/mdlive.app").exists())
            .unwrap_or(false)
}

fn handle_service(action: ServiceAction, hostname: &str, port: u16) -> Result<()> {
    let plist_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join("Library/LaunchAgents");
    let plist_path = plist_dir.join(format!("{LAUNCHD_LABEL}.plist"));

    match action {
        ServiceAction::Install => {
            let exe = std::env::current_exe()?;
            std::fs::create_dir_all(&plist_dir)?;

            let plist = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCHD_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>-H</string>
        <string>{hostname}</string>
        <string>-p</string>
        <string>{port}</string>
        <string>--no-open</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/mdlive.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/mdlive.err</string>
</dict>
</plist>"#,
                exe = exe.display()
            );

            std::fs::write(&plist_path, plist)?;

            let uid = get_uid();
            let status = std::process::Command::new("launchctl")
                .arg("bootstrap")
                .arg(format!("gui/{uid}"))
                .arg(&plist_path)
                .status()?;

            if status.success() {
                println!("Installed and started {LAUNCHD_LABEL}");
                println!("  Plist: {}", plist_path.display());
                println!("  http://{hostname}:{port}");
            } else {
                anyhow::bail!("launchctl bootstrap failed");
            }
        }
        ServiceAction::Start => {
            if !plist_path.exists() {
                anyhow::bail!("LaunchAgent not installed. Run `mdlive service install` first.");
            }
            let status = std::process::Command::new("launchctl")
                .args(["start", LAUNCHD_LABEL])
                .status()?;
            if status.success() {
                println!("Started {LAUNCHD_LABEL}");
            } else {
                anyhow::bail!("launchctl start failed");
            }
        }
        ServiceAction::Stop => {
            let status = std::process::Command::new("launchctl")
                .args(["stop", LAUNCHD_LABEL])
                .status()?;
            if status.success() {
                println!("Stopped {LAUNCHD_LABEL}");
            } else {
                println!("{LAUNCHD_LABEL}: not running or not installed");
            }
            delete_daemon_port();
        }
        ServiceAction::Uninstall => {
            if plist_path.exists() {
                let uid = get_uid();
                let _ = std::process::Command::new("launchctl")
                    .arg("bootout")
                    .arg(format!("gui/{uid}"))
                    .arg(&plist_path)
                    .status();
                std::fs::remove_file(&plist_path)?;
                println!("Uninstalled {LAUNCHD_LABEL}");
            } else {
                println!("No plist found at {}", plist_path.display());
            }
        }
        ServiceAction::Status => {
            let output = std::process::Command::new("launchctl")
                .args(["list", LAUNCHD_LABEL])
                .output()?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                println!("{LAUNCHD_LABEL}: running");
                for line in stdout.lines() {
                    if line.contains("PID") || line.contains("pid") {
                        println!("  {}", line.trim());
                    }
                }
            } else {
                println!("{LAUNCHD_LABEL}: not running");
            }

            if plist_path.exists() {
                println!("  Plist: {}", plist_path.display());
            } else {
                println!("  Plist: not installed");
            }
        }
    }

    Ok(())
}
