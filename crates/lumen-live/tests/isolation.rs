use lumen_live::hub::{EventHub, HubConfig};
use lumen_live::mock::{Fault, MockAdapter};
use lumen_live::supervisor::Backoff;
use lumen_live::{AccountId, ConnectionState, LiveManager, LiveTarget, Platform, Credentials};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn target(id: &str, platform: Platform) -> LiveTarget {
    LiveTarget { account_id: AccountId(id.into()), platform, handle: id.into(), settings: Default::default() }
}

fn fast_backoff() -> Backoff {
    Backoff { initial: Duration::from_millis(20), max: Duration::from_millis(100), offline_poll: Duration::from_millis(50), rate_limited: Duration::from_millis(50), connect_timeout: Duration::from_secs(2), max_consecutive_failures: None }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failures_on_one_platform_do_not_affect_others() {
    let cancel = CancellationToken::new();
    let (hub, mut inbox) = EventHub::start(HubConfig::default(), cancel.clone());
    let manager = LiveManager::new(hub, fast_backoff(), cancel.clone());

    let mut tiktok = MockAdapter::new(Platform::TikTok);
    tiktok.total_events = Some(200);
    manager.register_adapter(Arc::new(tiktok));
    let mut kwai = MockAdapter::new(Platform::KwaiGlobal);
    kwai.fault = Fault::PanicAfter(3);
    manager.register_adapter(Arc::new(kwai));
    let mut likee = MockAdapter::new(Platform::LikeeGlobal);
    likee.fault = Fault::BadAuth;
    manager.register_adapter(Arc::new(likee));

    manager.start(target("tt-a", Platform::TikTok), Credentials::new(), "tt-a").unwrap();
    manager.start(target("tt-b", Platform::TikTok), Credentials::new(), "tt-b").unwrap();
    manager.start(target("kw-a", Platform::KwaiGlobal), Credentials::new(), "kw-a").unwrap();
    manager.start(target("lk-a", Platform::LikeeGlobal), Credentials::new(), "lk-a").unwrap();

    let mut per_account = std::collections::HashMap::<String, usize>::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while per_account.get("tt-a").copied().unwrap_or(0) < 200 || per_account.get("tt-b").copied().unwrap_or(0) < 200 {
        let event = tokio::time::timeout_at(deadline, inbox.recv()).await.expect("events from healthy accounts").unwrap();
        *per_account.entry(event.account_id.0.clone()).or_default() += 1;
    }
    assert_eq!(per_account["tt-a"], 200);
    assert_eq!(per_account["tt-b"], 200);

    let health = manager.health();
    let lk = health.iter().find(|h| h.label == "lk-a").unwrap();
    assert!(matches!(lk.state, ConnectionState::Failed { .. }), "bad auth is terminal: {:?}", lk.state);
    let kw = health.iter().find(|h| h.label == "kw-a").unwrap();
    assert!(kw.last_error.as_deref().unwrap_or("").contains("panic"), "panic recorded: {:?}", kw);

    // Stopping one account leaves the other running.
    assert!(manager.stop(&AccountId("kw-a".into())));
    assert!(!manager.is_running(&AccountId("kw-a".into())));
    manager.shutdown(Duration::from_secs(2)).await;
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconnects_after_drops_and_dedups_replays() {
    let cancel = CancellationToken::new();
    let (hub, mut inbox) = EventHub::start(HubConfig::default(), cancel.clone());
    let manager = LiveManager::new(hub.clone(), fast_backoff(), cancel.clone());
    let mut a = MockAdapter::new(Platform::TikTok);
    a.fault = Fault::FailConnects(3);
    a.total_events = Some(50);
    manager.register_adapter(Arc::new(a));
    let health = manager.start(target("acct", Platform::TikTok), Credentials::new(), "acct").unwrap();
    // Same room read by a second account: duplicates must be suppressed in the combined inbox.
    manager.start(target("acct", Platform::TikTok).clone_with_id("acct-2"), Credentials::new(), "acct-2").unwrap();
    let mut ids = HashSet::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while ids.len() < 50 {
        let e = tokio::time::timeout_at(deadline, inbox.recv()).await.expect("events").unwrap();
        assert!(ids.insert(e.event_id.clone()), "duplicate delivered: {}", e.event_id);
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(inbox.try_recv().is_err(), "no duplicates after the room was fully read");
    assert!(health.snapshot().connects >= 1);
    assert!(hub.counters.duplicates.load(std::sync::atomic::Ordering::Relaxed) >= 50);
    manager.shutdown(Duration::from_secs(2)).await;
}

trait CloneWithId { fn clone_with_id(&self, id: &str) -> LiveTarget; }
impl CloneWithId for LiveTarget {
    fn clone_with_id(&self, id: &str) -> LiveTarget { LiveTarget { account_id: AccountId(id.into()), ..self.clone() } }
}
