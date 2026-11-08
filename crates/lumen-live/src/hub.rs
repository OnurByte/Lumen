//! Event hub: per-account bounded queues, fair merge, global dedup, fan-out.
//!
//! ```text
//! account A runner ─► AccountSink(cap N) ─┐
//! account B runner ─► AccountSink(cap N) ─┼─► fair merge ─► dedup ─► broadcast (observers, lossy)
//! account C runner ─► AccountSink(cap N) ─┘                       └► inbox (bounded, lossless)
//! ```
//! Each account has its own queue, so a flood or stall on one account cannot starve the others.
//! Backpressure: when an account queue is full, low-value events (likes, joins, viewer counts)
//! are dropped and counted; high-value events (chat, gifts, follows) wait up to
//! `high_value_wait` before being dropped.
use crate::dedup::Deduplicator;
use crate::event::LiveEvent;
use crate::health::SharedHealth;
use crate::platform::AccountId;
use crate::ratelimit::TokenBucket;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::{StreamExt, StreamMap, wrappers::ReceiverStream};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct HubConfig {
    pub account_queue: usize,
    pub inbox_queue: usize,
    pub observer_queue: usize,
    pub dedup_capacity: usize,
    pub dedup_ttl: Duration,
    pub high_value_wait: Duration,
    /// Per-account ingress rate (events/s) and burst.
    pub account_rate: f64,
    pub account_burst: u32,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            account_queue: 512,
            inbox_queue: 1024,
            observer_queue: 4096,
            dedup_capacity: 50_000,
            dedup_ttl: Duration::from_secs(600),
            high_value_wait: Duration::from_secs(2),
            account_rate: 200.0,
            account_burst: 400,
        }
    }
}

#[derive(Debug, Default)]
pub struct HubCounters {
    pub merged: AtomicU64,
    pub duplicates: AtomicU64,
    pub delivered: AtomicU64,
}

/// Write side handed to one account's supervisor.
pub struct AccountSink {
    tx: mpsc::Sender<LiveEvent>,
    health: SharedHealth,
    bucket: TokenBucket,
    high_value_wait: Duration,
}

impl AccountSink {
    /// Push one event applying rate limits and backpressure. Returns whether it was queued.
    pub async fn push(&mut self, event: LiveEvent) -> bool {
        let high = event.kind.is_high_value();
        if !self.bucket.try_take(Instant::now()) {
            if !high {
                self.health.update(|h| h.dropped_rate_limit += 1);
                return false;
            }
            let wait = self.bucket.wait_secs(Instant::now()).min(self.high_value_wait.as_secs_f64());
            tokio::time::sleep(Duration::from_secs_f64(wait)).await;
            if !self.bucket.try_take(Instant::now()) {
                self.health.update(|h| h.dropped_rate_limit += 1);
                return false;
            }
        }
        let queued = if high {
            self.tx.send_timeout(event, self.high_value_wait).await.is_ok()
        } else {
            self.tx.try_send(event).is_ok()
        };
        self.health.update(|h| {
            if queued {
                h.events_forwarded += 1
            } else {
                h.dropped_backpressure += 1
            }
        });
        queued
    }
}

#[derive(Clone)]
pub struct EventHub {
    register_tx: mpsc::UnboundedSender<(AccountId, mpsc::Receiver<LiveEvent>)>,
    observers: broadcast::Sender<LiveEvent>,
    config: HubConfig,
    pub counters: Arc<HubCounters>,
}

impl EventHub {
    /// Start the hub task. The returned receiver is the combined AI/moderation inbox.
    pub fn start(config: HubConfig, cancel: CancellationToken) -> (Self, mpsc::Receiver<LiveEvent>) {
        let (register_tx, mut register_rx) = mpsc::unbounded_channel::<(AccountId, mpsc::Receiver<LiveEvent>)>();
        let (observers, _) = broadcast::channel(config.observer_queue.max(1));
        let (inbox_tx, inbox_rx) = mpsc::channel(config.inbox_queue.max(1));
        let counters = Arc::new(HubCounters::default());
        let hub = Self { register_tx, observers: observers.clone(), config: config.clone(), counters: counters.clone() };
        tokio::spawn(async move {
            let mut streams: StreamMap<AccountId, ReceiverStream<LiveEvent>> = StreamMap::new();
            let mut dedup = Deduplicator::new(config.dedup_capacity, config.dedup_ttl);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    Some((id, rx)) = register_rx.recv() => { streams.insert(id, ReceiverStream::new(rx)); }
                    Some((_, event)) = streams.next(), if !streams.is_empty() => {
                        counters.merged.fetch_add(1, Ordering::Relaxed);
                        if !dedup.first_seen(&event.dedup_key(), Instant::now()) {
                            counters.duplicates.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        let _ = observers.send(event.clone());
                        tokio::select! {
                            _ = cancel.cancelled() => break,
                            sent = inbox_tx.send(event) => {
                                if sent.is_err() { break; }
                                counters.delivered.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    else => break,
                }
            }
            tracing::debug!("event hub stopped");
        });
        (hub, inbox_rx)
    }

    /// Create the isolated queue for one account. Re-registering replaces the old queue.
    pub fn register(&self, account: AccountId, health: SharedHealth) -> AccountSink {
        let (tx, rx) = mpsc::channel(self.config.account_queue.max(1));
        let _ = self.register_tx.send((account, rx));
        AccountSink {
            tx,
            health,
            bucket: TokenBucket::new(self.config.account_burst, self.config.account_rate, Instant::now()),
            high_value_wait: self.config.high_value_wait,
        }
    }

    /// Lossy observer stream (UI, overlays, WebSocket clients).
    pub fn subscribe(&self) -> broadcast::Receiver<LiveEvent> {
        self.observers.subscribe()
    }
}
