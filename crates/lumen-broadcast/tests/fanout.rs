//! End-to-end: one encoder → relay → two local RTMP servers + one dead destination.
use lumen_broadcast::{Destination, start};
use lumen_live::{AccountId, Platform, SecretString};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn dest(label: &str, platform: Platform, port: u16, key: &str) -> Destination {
    Destination { account_id: AccountId(format!("{label}-0000")), label: label.into(), platform, ingest_url: format!("rtmp://127.0.0.1:{port}/live"), stream_key: SecretString::new(key), video_kbps: 800, audio_kbps: 64 }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn simultaneous_destinations_with_isolated_failure() {
    if std::process::Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("ffmpeg not installed; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("lumen-fanout-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (p1, p2, dead) = (free_port(), free_port(), free_port());
    let sink = |port: u16, key: &str, out: &std::path::Path| {
        Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y", "-listen", "1", "-f", "flv", "-i"]).arg(format!("rtmp://127.0.0.1:{port}/live/{key}"))
            .args(["-c", "copy", "-f", "mpegts"]).arg(out).stdin(Stdio::null()).kill_on_drop(true).spawn().unwrap()
    };
    let (o1, o2) = (dir.join("tiktok.ts"), dir.join("kwai.ts"));
    let mut s1 = sink(p1, "key-a", &o1);
    let mut s2 = sink(p2, "key-b", &o2);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ingest = listener.local_addr().unwrap().port();
    let cancel = CancellationToken::new();
    let relay = start(listener, vec![dest("tiktok-main", Platform::TikTok, p1, "key-a"), dest("kwai-main", Platform::KwaiGlobal, p2, "key-b"), dest("likee-dead", Platform::LikeeGlobal, dead, "key-c")], "ffmpeg".into(), cancel.clone());

    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-re", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=15", "-f", "lavfi", "-i", "sine=frequency=440", "-t", "6"])
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-g", "15", "-c:a", "aac", "-f", "mpegts"]).arg(format!("tcp://127.0.0.1:{ingest}"))
        .status().await.unwrap();
    assert!(status.success());
    tokio::time::sleep(Duration::from_secs(2)).await;
    let health = relay.snapshot();
    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), s1.wait()).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), s2.wait()).await;

    for h in &health {
        eprintln!("{h:?}");
        assert!(!h.last_error.as_deref().unwrap_or("").contains("key-"), "stream key leaked into health/logs");
    }
    assert!(health[0].bytes_sent > 50_000 && health[1].bytes_sent > 50_000, "both live destinations streamed");
    assert!(health[2].starts >= 2 && health[2].last_error.is_some(), "dead destination failed and restarted on its own");
    for o in [&o1, &o2] {
        let size = std::fs::metadata(o).map(|m| m.len()).unwrap_or(0);
        assert!(size > 20_000, "{o:?} received {size} bytes");
    }
    std::fs::remove_dir_all(&dir).ok();
}
