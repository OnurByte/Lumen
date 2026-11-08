//! Broadcast relay: one MPEG-TS ingest (from OBS "Custom Output (FFmpeg)" or any encoder) is
//! fanned out to N destinations. Each destination owns an FFmpeg process, a bounded queue and
//! a restart loop, so a slow, rejected or crashed destination never stalls the others.
//!
//! ```text
//! OBS ──mpegts/tcp──► relay ─┬─► queue ─► ffmpeg ─► rtmps://tiktok-ingest/…  (account A)
//!                            ├─► queue ─► ffmpeg ─► rtmp://kwai-ingest/…     (account B)
//!                            └─► queue ─► ffmpeg ─► rtmp://likee-ingest/…    (account C)
//! ```
use bytes::Bytes;
use lumen_live::{AccountId, Platform, SecretString};
use serde::Serialize;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

#[derive(Clone)]
pub struct Destination {
    pub account_id: AccountId,
    pub label: String,
    pub platform: Platform,
    pub ingest_url: String,
    pub stream_key: SecretString,
    pub video_kbps: u32,
    pub audio_kbps: u32,
}

impl Destination {
    fn target_url(&self) -> String {
        if self.stream_key.is_empty() { self.ingest_url.clone() } else { format!("{}/{}", self.ingest_url.trim_end_matches('/'), self.stream_key.expose()) }
    }
    fn format(&self) -> &'static str {
        if self.ingest_url.starts_with("srt://") { "mpegts" } else { "flv" }
    }
    /// Replace the stream key anywhere in FFmpeg output before it reaches logs.
    pub fn redact(&self, line: &str) -> String {
        if self.stream_key.is_empty() { line.to_owned() } else { line.replace(self.stream_key.expose(), "[redacted]") }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DestinationState {
    Waiting,
    Streaming,
    Restarting,
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
pub struct DestinationHealth {
    pub label: String,
    pub platform: Platform,
    pub state: DestinationState,
    pub starts: u32,
    pub bytes_sent: u64,
    pub lagged: u32,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct RelayHandle {
    pub health: Arc<Mutex<Vec<DestinationHealth>>>,
}

impl RelayHandle {
    pub fn snapshot(&self) -> Vec<DestinationHealth> {
        self.health.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn update(h: &Arc<Mutex<Vec<DestinationHealth>>>, i: usize, f: impl FnOnce(&mut DestinationHealth)) {
    f(&mut h.lock().unwrap_or_else(|e| e.into_inner())[i]);
}

/// Accept MPEG-TS on `listener` and stream it to every destination until cancelled.
pub fn start(listener: TcpListener, destinations: Vec<Destination>, ffmpeg: String, cancel: CancellationToken) -> RelayHandle {
    // ~8 MB per destination at 64 KiB chunks before it is considered lagging.
    let (tx, _) = broadcast::channel::<Bytes>(128);
    let health = Arc::new(Mutex::new(
        destinations.iter().map(|d| DestinationHealth { label: d.label.clone(), platform: d.platform, state: DestinationState::Waiting, starts: 0, bytes_sent: 0, lagged: 0, last_error: None }).collect::<Vec<_>>(),
    ));
    for (i, d) in destinations.into_iter().enumerate() {
        let span = tracing::info_span!("destination", label = %d.label, platform = %d.platform, account = %d.account_id.short());
        tokio::spawn(run_destination(i, d, tx.clone(), health.clone(), ffmpeg.clone(), cancel.clone()).instrument(span));
    }
    let ingest_cancel = cancel.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, peer) = tokio::select! { _ = ingest_cancel.cancelled() => break, r = listener.accept() => match r { Ok(x) => x, Err(_) => continue } };
            tracing::info!(%peer, "broadcast ingest connected");
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = tokio::select! { _ = ingest_cancel.cancelled() => return, r = sock.read(&mut buf) => r.unwrap_or(0) };
                if n == 0 {
                    break;
                }
                let _ = tx.send(Bytes::copy_from_slice(&buf[..n]));
            }
            tracing::warn!("broadcast ingest disconnected; waiting for encoder");
        }
    });
    RelayHandle { health }
}

async fn run_destination(i: usize, d: Destination, tx: broadcast::Sender<Bytes>, health: Arc<Mutex<Vec<DestinationHealth>>>, ffmpeg: String, cancel: CancellationToken) {
    let mut failures = 0u32;
    while !cancel.is_cancelled() {
        let mut rx = tx.subscribe();
        update(&health, i, |h| { h.state = DestinationState::Streaming; h.starts += 1; });
        let mut child = match tokio::process::Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "warning", "-fflags", "+genpts", "-f", "mpegts", "-i", "pipe:0"])
            .args(["-c:v", "libx264", "-preset", "veryfast", "-tune", "zerolatency", "-g", "60"])
            .args(["-b:v", &format!("{}k", d.video_kbps), "-maxrate", &format!("{}k", d.video_kbps), "-bufsize", &format!("{}k", d.video_kbps * 2)])
            .args(["-c:a", "aac", "-b:a", &format!("{}k", d.audio_kbps), "-f", d.format()])
            // shortcut: the key is visible in this host's process list while streaming; use a dedicated OS user if other local users are untrusted.
            .arg(d.target_url())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                update(&health, i, |h| { h.state = DestinationState::Stopped; h.last_error = Some(format!("cannot start ffmpeg: {e}")); });
                return;
            }
        };
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stderr = child.stderr.take().expect("piped stderr");
        let (dc, hc) = (d.clone(), health.clone());
        let log = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let clean = dc.redact(&line);
                tracing::warn!(ffmpeg = %clean);
                update(&hc, i, |h| h.last_error = Some(clean.chars().take(300).collect()));
            }
        });
        let reason = loop {
            tokio::select! {
                _ = cancel.cancelled() => break "cancelled".to_string(),
                status = child.wait() => break format!("ffmpeg exited: {}", status.map(|s| s.to_string()).unwrap_or_default()),
                chunk = rx.recv() => match chunk {
                    Ok(bytes) => {
                        if stdin.write_all(&bytes).await.is_err() { break "ffmpeg stdin closed".into(); }
                        update(&health, i, |h| h.bytes_sent += bytes.len() as u64);
                        failures = 0;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        update(&health, i, |h| h.lagged += 1);
                        break format!("destination lagged by {n} chunks; restarting it alone");
                    }
                    Err(broadcast::error::RecvError::Closed) => break "relay closed".into(),
                },
            }
        };
        drop(stdin);
        let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        let _ = child.kill().await;
        let _ = log.await;
        if cancel.is_cancelled() {
            break;
        }
        failures += 1;
        let delay = Duration::from_millis(500).saturating_mul(1 << failures.min(6)).min(Duration::from_secs(30));
        tracing::warn!(%reason, retry_in_ms = delay.as_millis() as u64, "destination restarting");
        update(&health, i, |h| { h.state = DestinationState::Restarting; if h.last_error.is_none() { h.last_error = Some(reason.clone()); } });
        tokio::select! { _ = cancel.cancelled() => break, _ = tokio::time::sleep(delay) => {} }
    }
    update(&health, i, |h| h.state = DestinationState::Stopped);
}
