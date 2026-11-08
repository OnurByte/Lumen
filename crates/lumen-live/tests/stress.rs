//! Stress: many accounts across platforms at high event rates with a slow consumer.
//! Asserts bounded memory (queues), fairness across accounts and no cross-account starvation.
use lumen_live::hub::{EventHub, HubConfig};
use lumen_live::mock::MockAdapter;
use lumen_live::supervisor::Backoff;
use lumen_live::{AccountId, Credentials, LiveManager, LiveTarget, Platform};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sixty_accounts_flood_with_slow_consumer() {
    let cancel = CancellationToken::new();
    let config = HubConfig { account_queue: 64, inbox_queue: 128, ..HubConfig::default() };
    let (hub, mut inbox) = EventHub::start(config, cancel.clone());
    let manager = LiveManager::new(hub.clone(), Backoff::default(), cancel.clone());
    for p in Platform::ALL {
        let mut a = MockAdapter::new(p);
        a.events_per_batch = 50;
        a.batch_interval = Duration::from_millis(5); // ~10k events/s per account offered
        manager.register_adapter(Arc::new(a));
    }
    for i in 0..60 {
        let p = Platform::ALL[i % 3];
        let t = LiveTarget { account_id: AccountId(format!("acct-{i:02}")), platform: p, handle: format!("h{i}"), settings: Default::default() };
        manager.start(t, Credentials::new(), &format!("acct-{i:02}")).unwrap();
    }
    let mut per_account: HashMap<String, u64> = HashMap::new();
    let end = tokio::time::Instant::now() + Duration::from_secs(4);
    while tokio::time::Instant::now() < end {
        if let Ok(Some(e)) = tokio::time::timeout(Duration::from_millis(50), inbox.recv()).await {
            *per_account.entry(e.account_id.0).or_default() += 1;
            tokio::task::yield_now().await; // a deliberately slow-ish consumer
        }
    }
    let health = manager.health();
    manager.shutdown(Duration::from_secs(3)).await;
    cancel.cancel();

    assert_eq!(per_account.len(), 60, "every account delivered events (no starvation)");
    let min = *per_account.values().min().unwrap();
    let max = *per_account.values().max().unwrap();
    assert!(min * 4 >= max, "fair merge: min={min} max={max}");
    let dropped: u64 = health.iter().map(|h| h.dropped_backpressure + h.dropped_rate_limit).sum();
    assert!(dropped > 0, "overload was shed by backpressure/rate limits instead of buffering unboundedly");
    assert!(health.iter().all(|h| h.reconnects == 0), "overload never forced reconnects");
}
