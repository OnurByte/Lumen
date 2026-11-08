//! Live validation: `cargo run -p lumen-platform-kwai --example watch -- HANDLE SECONDS`
//! Prints event kinds and counts only (no viewer names) to keep logs free of personal data.
use lumen_live::{AccountId, LiveSession, LiveTarget, Platform};
use lumen_platform_kwai::{api::*, session::KwaiSession};
use std::collections::BTreeMap;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let handle = args.next().expect("handle");
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(60);
    let api = KwaiApi::new(DEFAULT_API_HOST, "ANDROID_5e1c0ffee0ddba11", None).unwrap();
    let room = api.find_live(&handle).await.expect("room");
    let ticket = api.start_play(&room).await.expect("ticket");
    let target = LiveTarget { account_id: AccountId("probe".into()), platform: Platform::KwaiGlobal, handle: handle.clone(), settings: Default::default() };
    let mut s = KwaiSession::connect(&api, &target, room, ticket, None, Default::default()).await.expect("connect");
    let mut counts = BTreeMap::<&str, u64>::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        match tokio::time::timeout_at(deadline, s.next_events()).await {
            Err(_) => break,
            Ok(Ok(Some(events))) => for e in events { *counts.entry(e.kind.name()).or_default() += 1; if let lumen_live::EventKind::Chat { text } = &e.kind { println!("chat ({} chars) id={}", text.chars().count(), e.event_id); } },
            Ok(Ok(None)) => { println!("stream closed"); break; }
            Ok(Err(e)) => { println!("error: {e}"); break; }
        }
    }
    println!("counts: {counts:?}");
}
