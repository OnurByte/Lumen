//! Broadcast overlay for OBS: a 1080x1920 page (OBS Browser Source) driven by live stage state.
//!
//! Loopback only, GET only, bounded requests, and a whitelist of files: avatar frames from the
//! avatar directories and background images listed in the user's theme. Nothing here accepts
//! input from the network beyond the request line.
use crate::control::Hub;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub const DEFAULT_PORT: u16 = 47800;
const PAGE: &str = include_str!("overlay.html");
const MAX_REQUEST: usize = 8 * 1024;
const MAX_FILE: u64 = 24 * 1024 * 1024;

pub fn port() -> u16 {
    std::env::var("LUMEN_OVERLAY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

pub async fn serve(hub: Arc<Hub>, port: u16) {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("overlay disabled: cannot bind 127.0.0.1:{port}: {e}");
            return;
        }
    };
    println!("overlay ready: http://127.0.0.1:{port}/ (OBS Browser Source, 1080x1920)");
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            _ = hub.shutdown.cancelled() => break,
        };
        let Ok((stream, _)) = accepted else { continue };
        let hub = hub.clone();
        tokio::spawn(async move {
            let _ =
                tokio::time::timeout(std::time::Duration::from_secs(5), handle(stream, hub)).await;
        });
    }
}

async fn handle(mut stream: TcpStream, hub: Arc<Hub>) -> std::io::Result<()> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).await?;
        if n == 0 || buffer.len() + n > MAX_REQUEST {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buffer);
    let mut parts = head.lines().next().unwrap_or("").split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let path = target.split('?').next().unwrap_or("");
    let (status, kind, body) = if method != "GET" {
        (405, "text/plain", b"method not allowed".to_vec())
    } else {
        route(path, &hub).await
    };
    let header = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        if status == 200 { "OK" } else { "Error" },
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&body).await
}

async fn route(path: &str, hub: &Hub) -> (u16, &'static str, Vec<u8>) {
    match path {
        "/" | "/index.html" => (200, "text/html; charset=utf-8", PAGE.as_bytes().to_vec()),
        "/state.json" => (200, "application/json", state(hub).await.into_bytes()),
        _ => {
            if let Some(rest) = path.strip_prefix("/avatars/") {
                if let Some(file) = avatar_file(rest) {
                    return file_response(&file).await;
                }
            } else if let Some(index) = path
                .strip_prefix("/bg/")
                .and_then(|i| i.parse::<usize>().ok())
                && let Some(file) = theme_images().get(index)
            {
                return file_response(file).await;
            }
            (404, "text/plain", b"not found".to_vec())
        }
    }
}

async fn state(hub: &Hub) -> String {
    let status = hub.status.lock().await.clone();
    let mut stage = hub.stage.lock().map(|s| s.clone()).unwrap_or_default();
    let ends = hub.topic_ends_ms.load(Ordering::Acquire);
    let now = hub.clock.elapsed().as_millis() as u64;
    stage.topic_remaining_s = if ends == 0 {
        0
    } else {
        (ends.saturating_sub(now) / 1000) as u32
    };
    let avatars: Vec<bool> = stage
        .avatars
        .iter()
        .map(|a| avatar_dir(a).is_some())
        .collect();
    serde_json::json!({
        "stage": stage,
        "paused": status.paused,
        "stopped": status.stopped,
        "avatars": avatars,
        "theme": theme(),
        "backgrounds": theme_images().len(),
    })
    .to_string()
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// The user's theme as JSON; without a theme file, the shipped oriental carousel default.
fn theme() -> serde_json::Value {
    config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("lumen/theme.toml")).ok())
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
        .and_then(|value| serde_json::to_value(value).ok())
        .unwrap_or_else(|| {
            serde_json::json!({
                "background": "carousel",
                "images": (1..=4).map(|i| format!("lumen:backgrounds/oryantal-{i}.png")).collect::<Vec<_>>(),
                "carousel_seconds": 20,
                "image_dim": 0.6,
            })
        })
}

/// `lumen:rel` -> installed `packs/rel` if present, else the source checkout's.
fn lumen_path(rel: &str) -> PathBuf {
    let installed = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|d| d.join("lumen/packs").join(rel));
    match installed {
        Some(p) if p.is_file() => p,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packs")
            .join(rel),
    }
}

fn theme_images() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    theme()
        .get("images")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .map(
                    |s| match (s.strip_prefix("lumen:"), s.strip_prefix("~/"), &home) {
                        (Some(rel), _, _) => lumen_path(rel),
                        (_, Some(rest), Some(home)) => home.join(rest),
                        _ => PathBuf::from(s),
                    },
                )
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default()
}

fn valid_preset(preset: &str) -> bool {
    !preset.is_empty()
        && preset.len() <= 32
        && preset
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Same lookup order as Studio: `$LUMEN_AVATARS`, installed data dir, source checkout.
fn avatar_dir(preset: &str) -> Option<PathBuf> {
    if !valid_preset(preset) {
        return None;
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    [
        std::env::var_os("LUMEN_AVATARS").map(PathBuf::from),
        data.map(|d| d.join("lumen/packs/avatars")),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/avatars")),
    ]
    .into_iter()
    .flatten()
    .map(|root| root.join(preset))
    .find(|dir| dir.join("idle.png").is_file() && dir.join("talk.png").is_file())
}

/// `<preset>/<idle|talk|blink>.png` only.
fn avatar_file(rest: &str) -> Option<PathBuf> {
    let (preset, file) = rest.split_once('/')?;
    if !matches!(file, "idle.png" | "talk.png" | "blink.png") {
        return None;
    }
    let path = avatar_dir(preset)?.join(file);
    path.is_file().then_some(path)
}

async fn file_response(path: &Path) -> (u16, &'static str, Vec<u8>) {
    let kind = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return (404, "text/plain", b"not found".to_vec()),
    };
    match tokio::fs::metadata(path).await {
        Ok(meta) if meta.len() <= MAX_FILE => match tokio::fs::read(path).await {
            Ok(bytes) => (200, kind, bytes),
            Err(_) => (404, "text/plain", b"not found".to_vec()),
        },
        _ => (404, "text/plain", b"not found".to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whitelisted_avatar_files_resolve() {
        assert!(avatar_file("fesli/idle.png").is_some());
        assert!(avatar_file("fesli/../../etc/passwd").is_none());
        assert!(avatar_file("../fesli/idle.png").is_none());
        assert!(avatar_file("fesli/avatar.toml").is_none());
        assert!(!valid_preset("a/b"));
    }

    #[tokio::test]
    async fn serves_page_state_and_rejects_other_methods_and_paths() {
        let hub = Arc::new(Hub::new(false));
        hub.stage.lock().unwrap().topic = "Konu".into();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let server = tokio::spawn(serve(hub.clone(), port));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let get = |request: &'static str| async move {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut out = Vec::new();
            stream.read_to_end(&mut out).await.unwrap();
            String::from_utf8_lossy(&out).into_owned()
        };
        assert!(get("GET / HTTP/1.1\r\n\r\n").await.contains("<html"));
        let state = get("GET /state.json HTTP/1.1\r\n\r\n").await;
        assert!(state.contains("\"topic\":\"Konu\"") && state.contains("application/json"));
        assert!(
            get("POST / HTTP/1.1\r\n\r\n")
                .await
                .starts_with("HTTP/1.1 405")
        );
        assert!(
            get("GET /../../etc/passwd HTTP/1.1\r\n\r\n")
                .await
                .starts_with("HTTP/1.1 404")
        );
        // Without a theme file the shipped oriental carousel is served; indexes past it are not.
        if theme_images().len() == 4 {
            let bg = get("GET /bg/0 HTTP/1.1\r\n\r\n").await;
            assert!(bg.starts_with("HTTP/1.1 200") && bg.contains("image/png"));
        }
        assert!(
            get("GET /bg/99 HTTP/1.1\r\n\r\n")
                .await
                .starts_with("HTTP/1.1 404")
        );
        hub.shutdown.cancel();
        let _ = server.await;
    }
}
