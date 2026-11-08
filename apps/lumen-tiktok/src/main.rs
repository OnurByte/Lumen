//! Single-room compatibility entry point: `lumen-tiktok USERNAME CONTROL_SOCKET`.
//! Multi-account use goes through `lumen-live connect`; this runs the same adapter,
//! supervisor, moderation and bridge for one ad-hoc account.
use lumen_live::hub::{EventHub, HubConfig};
use lumen_live::moderation::{ModerationConfig, Moderator};
use lumen_live::supervisor::Backoff;
use lumen_live::{AccountId, Credentials, LiveManager, LiveTarget, Platform};
use lumen_platform_tiktok::TikTokAdapter;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: lumen-tiktok USERNAME CONTROL_SOCKET");
        std::process::exit(1);
    }
    let username = args[0].strip_prefix('@').unwrap_or(&args[0]);
    if username.is_empty() || username.len() > 64 || !username.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.') {
        eprintln!("invalid TikTok username");
        std::process::exit(1);
    }
    let cancel = CancellationToken::new();
    let (hub, inbox) = EventHub::start(HubConfig::default(), cancel.clone());
    let manager = LiveManager::new(hub, Backoff::default(), cancel.clone());
    manager.register_adapter(Arc::new(TikTokAdapter::default()));
    let target = LiveTarget { account_id: AccountId(format!("cli-{username}")), platform: Platform::TikTok, handle: username.into(), settings: Default::default() };
    let _ = manager.start(target, Credentials::new(), username);
    let bridge = tokio::spawn(lumen_live::bridge::forward_to_lumend(inbox, PathBuf::from(&args[1]), Moderator::new(ModerationConfig::default())));
    let _ = tokio::signal::ctrl_c().await;
    manager.shutdown(Duration::from_secs(3)).await;
    cancel.cancel();
    if let Ok(Ok(stats)) = tokio::time::timeout(Duration::from_secs(3), bridge).await {
        println!("TikTok connector ended: accepted={} rejected={}", stats.forwarded, stats.rejected + stats.held);
    }
}
