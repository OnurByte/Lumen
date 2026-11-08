//! Deterministic in-process adapter for tests, stress runs and demos. Never touches the network.
use crate::adapter::{AdapterError, AuthState, LiveSession, LiveTarget, PlatformAdapter, Profile};
use crate::event::{EventKind, LiveEvent, LiveUser};
use crate::platform::{Capabilities, Platform, Support};
use crate::secret::Credentials;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum Fault {
    None,
    /// Fail the first N connection attempts with a transient error.
    FailConnects(u32),
    /// Drop the stream after N events on every connection.
    DropAfter(u64),
    /// Reject credentials permanently.
    BadAuth,
    /// Panic inside the session after N events (tests panic isolation).
    PanicAfter(u64),
}

pub struct MockAdapter {
    pub platform: Platform,
    pub events_per_batch: usize,
    pub batch_interval: Duration,
    pub total_events: Option<u64>,
    pub fault: Fault,
    attempts: Arc<AtomicU32>,
}

impl MockAdapter {
    pub fn new(platform: Platform) -> Self {
        Self { platform, events_per_batch: 5, batch_interval: Duration::from_millis(10), total_events: None, fault: Fault::None, attempts: Arc::new(AtomicU32::new(0)) }
    }
    pub fn attempts(&self) -> u32 {
        self.attempts.load(Ordering::Relaxed)
    }
}

struct MockSession {
    target: LiveTarget,
    platform: Platform,
    room: String,
    seq: u64,
    per_batch: usize,
    interval: Duration,
    total: Option<u64>,
    fault: Fault,
    cancel: CancellationToken,
}

#[async_trait]
impl LiveSession for MockSession {
    fn room_id(&self) -> &str {
        &self.room
    }
    async fn next_events(&mut self) -> Result<Option<Vec<LiveEvent>>, AdapterError> {
        tokio::select! {
            _ = self.cancel.cancelled() => return Err(AdapterError::Cancelled),
            _ = tokio::time::sleep(self.interval) => {}
        }
        let mut out = Vec::with_capacity(self.per_batch);
        for _ in 0..self.per_batch {
            if self.total.is_some_and(|t| self.seq >= t) {
                return Ok(if out.is_empty() { None } else { Some(out) });
            }
            match self.fault {
                Fault::DropAfter(n) if self.seq > 0 && self.seq.is_multiple_of(n) && out.is_empty() => {
                    self.seq += 1; // resume after the drop point on reconnect
                    return Err(AdapterError::Transient("mock drop".into()));
                }
                Fault::PanicAfter(n) if self.seq >= n => panic!("mock adapter panic"),
                _ => {}
            }
            let s = self.seq;
            self.seq += 1;
            let kind = match s % 7 {
                0..=2 => EventKind::Chat { text: format!("message {s} for {}", self.target.handle) },
                3 => EventKind::Like { count: 1, total: Some(s) },
                4 => EventKind::Gift { gift_id: "5655".into(), gift_name: Some("Rose".into()), count: 1, unit_value: Some(1) },
                5 => EventKind::Join,
                _ => EventKind::ViewerCount { current: 100 + s, total: None },
            };
            out.push(LiveEvent {
                platform: self.platform,
                account_id: self.target.account_id.clone(),
                room_id: self.room.clone(),
                event_id: s.to_string(),
                platform_ts_ms: Some(crate::now_ms()),
                received_ts_ms: crate::now_ms(),
                user: Some(LiveUser { id: format!("u{}", s % 97), display_name: format!("viewer{}", s % 97) }),
                kind,
            });
        }
        Ok(Some(out))
    }
}

#[async_trait]
impl PlatformAdapter for MockAdapter {
    fn platform(&self) -> Platform {
        self.platform
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { live_events: Support::Verified, chat: Support::Verified, ..Capabilities::NONE }
    }
    async fn authenticate(&self, _c: &Credentials) -> Result<AuthState, AdapterError> {
        match self.fault {
            Fault::BadAuth => Err(AdapterError::Auth("mock rejects credentials".into())),
            _ => Ok(AuthState::Anonymous),
        }
    }
    async fn profile(&self, handle: &str, _c: &Credentials) -> Result<Profile, AdapterError> {
        Ok(Profile { platform: self.platform, user_id: format!("mock-{handle}"), handle: handle.into(), display_name: handle.into(), avatar_url: None, followers: None, following: None, likes: None, bio: None, country: None, live_room_id: Some(format!("room-{handle}")) })
    }
    async fn open_live(&self, target: &LiveTarget, c: &Credentials, cancel: CancellationToken) -> Result<Box<dyn LiveSession>, AdapterError> {
        let attempt = self.attempts.fetch_add(1, Ordering::Relaxed) + 1;
        self.authenticate(c).await?;
        if let Fault::FailConnects(n) = self.fault
            && attempt <= n
        {
            return Err(AdapterError::Transient(format!("mock connect failure {attempt}")));
        }
        Ok(Box::new(MockSession {
            target: target.clone(),
            platform: self.platform,
            room: format!("room-{}", target.handle),
            seq: 0,
            per_batch: self.events_per_batch,
            interval: self.batch_interval,
            total: self.total_events,
            fault: self.fault.clone(),
            cancel,
        }))
    }
    fn stale_after(&self) -> Duration {
        Duration::from_secs(5)
    }
}
