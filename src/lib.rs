pub mod config;
mod handlers;
mod router;
mod state;
mod template;
mod tree;
mod util;
mod watcher;

use anyhow::{Context, Result};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use tokio::net::TcpListener;

pub const DEFAULT_PORT: u16 = 4891;

pub use config::AppConfig;
pub use config::RecentWorkspace;
pub use router::new_daemon_router;
pub use router::new_daemon_router_with_config;
pub use router::new_router;
pub use router::new_router_with_config;
pub use state::ServerMessage;
pub use util::scan_supported_files;

pub async fn serve_markdown(
    base_dir: PathBuf,
    tracked_files: Vec<PathBuf>,
    is_directory_mode: bool,
    hostname: impl AsRef<str>,
    port: u16,
    open: bool,
) -> Result<()> {
    let hostname = hostname.as_ref();

    let first_file = tracked_files.first().cloned();
    let router = router::new_router(base_dir.clone(), tracked_files, is_directory_mode)?;

    let (listener, actual_port) = bind_with_port_increment(hostname, port).await?;

    if actual_port != port {
        println!("  Port {port} in use, using {actual_port} instead");
    }

    let listen_addr = format_host(hostname, actual_port);

    if is_directory_mode {
        println!("  Serving markdown files from: {}", base_dir.display());
    } else if let Some(file_path) = first_file {
        println!("  Serving markdown file: {}", file_path.display());
    }

    println!("  Server running at: http://{listen_addr}");
    println!("  Live reload enabled");
    println!("\nPress Ctrl+C to stop the server");

    if open {
        let browse_addr = format_host(&browsable_host(hostname), actual_port);
        open_browser(&format!("http://{browse_addr}"))?;
    }

    axum::serve(listener, router).await?;

    Ok(())
}

pub async fn serve_daemon(hostname: impl AsRef<str>, port: u16, open: bool) -> Result<()> {
    let hostname = hostname.as_ref();

    let router = router::new_daemon_router();

    let (listener, actual_port) = bind_with_port_increment(hostname, port).await?;

    if actual_port != port {
        println!("  Port {port} in use, using {actual_port} instead");
    }

    let listen_addr = format_host(hostname, actual_port);

    // the desktop app owns the port file while it runs -- clobbering it would
    // route `mdlive <file>` here, where there is no window to open
    if !app_daemon_alive() {
        write_daemon_port(actual_port);
    }

    println!("  mdlive daemon started");
    println!("  Server running at: http://{listen_addr}");
    println!("\nPress Ctrl+C to stop the server");

    if open {
        let browse_addr = format_host(&browsable_host(hostname), actual_port);
        open_browser(&format!("http://{browse_addr}"))?;
    }

    axum::serve(listener, router).await?;

    if read_daemon_port() == Some(actual_port) {
        delete_daemon_port();
    }

    Ok(())
}

const MAX_PORT_ATTEMPTS: u16 = 100;

pub async fn bind_with_port_increment(
    hostname: &str,
    start_port: u16,
) -> Result<(TcpListener, u16)> {
    let mut port = start_port;
    loop {
        match TcpListener::bind((hostname, port)).await {
            Ok(listener) => return Ok((listener, port)),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                let next = port.checked_add(1).context("port range exhausted")?;
                if next - start_port >= MAX_PORT_ATTEMPTS {
                    anyhow::bail!("no available port found after trying {start_port}-{port}");
                }
                port = next;
            }
            Err(e) => return Err(e).context(format!("failed to bind to {hostname}:{port}")),
        }
    }
}

fn format_host(hostname: &str, port: u16) -> String {
    if hostname.parse::<Ipv6Addr>().is_ok() {
        format!("[{hostname}]:{port}")
    } else {
        format!("{hostname}:{port}")
    }
}

fn browsable_host(hostname: &str) -> String {
    if hostname
        .parse::<Ipv4Addr>()
        .ok()
        .is_some_and(|ip| ip.is_unspecified())
    {
        "127.0.0.1".into()
    } else if hostname
        .parse::<Ipv6Addr>()
        .ok()
        .is_some_and(|ip| ip.is_unspecified())
    {
        "::1".into()
    } else {
        hostname.into()
    }
}

fn open_browser(url: &str) -> Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "linux") {
        "xdg-open"
    } else {
        anyhow::bail!("--open is not supported on this platform");
    };

    let mut child = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("failed to run {program}"))?;

    std::thread::spawn(move || match child.wait() {
        Ok(status) if !status.success() => {
            eprintln!("{program} exited with {status}");
        }
        Err(e) => eprintln!("Failed waiting on {program}: {e}"),
        _ => {}
    });

    Ok(())
}

fn get_daemon_port_file() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config/mdlive/daemon.port"))
}

pub fn write_daemon_port(port: u16) {
    if let Some(port_file) = get_daemon_port_file() {
        if let Some(parent) = port_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&port_file, port.to_string());
    }
}

pub fn read_daemon_port() -> Option<u16> {
    let port_file = get_daemon_port_file()?;
    std::fs::read_to_string(port_file).ok()?.trim().parse().ok()
}

/// True when the port file names a live desktop app (it routes /api/window/new).
pub fn app_daemon_alive() -> bool {
    use std::io::{Read as _, Write as _};
    let Some(port) = read_daemon_port() else {
        return false;
    };
    let body = "{\"path\":\"\"}";
    let request = format!(
        "POST /api/window/new HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let Ok(addr) = format!("127.0.0.1:{port}").parse() else {
        return false;
    };
    let Ok(mut stream) =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300))
    else {
        return false;
    };
    let to = std::time::Duration::from_millis(300);
    let _ = stream.set_read_timeout(Some(to));
    let _ = stream.set_write_timeout(Some(to));
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.flush();
    let mut buf = vec![0u8; 512];
    let _ = stream.read(&mut buf);
    String::from_utf8_lossy(&buf).contains("path required")
}

pub fn delete_daemon_port() {
    if let Some(port_file) = get_daemon_port_file() {
        let _ = std::fs::remove_file(port_file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn test_format_host() {
        assert_eq!(format_host("127.0.0.1", 3000), "127.0.0.1:3000");
        assert_eq!(format_host("192.168.1.1", 8080), "192.168.1.1:8080");
        assert_eq!(format_host("localhost", 3000), "localhost:3000");
        assert_eq!(format_host("example.com", 80), "example.com:80");
        assert_eq!(format_host("::1", 3000), "[::1]:3000");
        assert_eq!(format_host("2001:db8::1", 8080), "[2001:db8::1]:8080");
    }

    #[test]
    fn test_browsable_host() {
        assert_eq!(browsable_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(browsable_host("::"), "::1");
        assert_eq!(browsable_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(browsable_host("::1"), "::1");
        assert_eq!(browsable_host("192.168.1.1"), "192.168.1.1");
        assert_eq!(browsable_host("localhost"), "localhost");
        assert_eq!(browsable_host("example.com"), "example.com");
    }

    #[tokio::test]
    async fn test_bind_with_port_increment_finds_free_port() {
        let blocker = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let blocked_port = blocker.local_addr().unwrap().port();

        let (listener, actual_port) = bind_with_port_increment("127.0.0.1", blocked_port)
            .await
            .unwrap();

        assert!(
            actual_port > blocked_port,
            "should have incremented past blocked port"
        );
        assert_eq!(listener.local_addr().unwrap().port(), actual_port);
    }

    #[tokio::test]
    async fn test_bind_with_port_increment_uses_requested_port_when_free() {
        // pick a high port unlikely to collide, retry a few times to handle CI flakiness
        for candidate in 59123..59133 {
            if let Ok((listener, actual_port)) =
                bind_with_port_increment("127.0.0.1", candidate).await
            {
                assert_eq!(actual_port, candidate);
                assert_eq!(listener.local_addr().unwrap().port(), candidate);
                return;
            }
        }
        panic!("could not find a free port in range 59123..59133");
    }

    #[tokio::test]
    async fn test_bind_with_port_increment_skips_multiple_occupied_ports() {
        let mut blockers = Vec::new();
        let mut base_port = None;

        for _ in 0..20 {
            let l1 = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let p = l1.local_addr().unwrap().port();
            if let (Ok(l2), Ok(l3)) = (
                TcpListener::bind(("127.0.0.1", p + 1)).await,
                TcpListener::bind(("127.0.0.1", p + 2)).await,
            ) {
                blockers.extend([l1, l2, l3]);
                base_port = Some(p);
                break;
            }
        }

        let base_port = base_port.expect("could not find three consecutive free ports");

        let (listener, actual_port) = bind_with_port_increment("127.0.0.1", base_port)
            .await
            .unwrap();

        assert!(
            actual_port >= base_port + 3,
            "should skip all three blocked ports"
        );
        assert_eq!(listener.local_addr().unwrap().port(), actual_port);

        drop(blockers);
    }
}
