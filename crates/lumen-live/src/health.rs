//! Per-account connection health and metrics.
use crate::platform::{AccountId, Platform};
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionState {
    Idle,
    Connecting,
    Live { room_id: String },
    Backoff { retry_in_ms: u64, reason: String },
    Offline,
    /// Will not retry without operator action (e.g. bad credentials).
    Failed { reason: String },
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthSnapshot {
    pub account_id: AccountId,
    pub platform: Platform,
    pub label: String,
    pub state: ConnectionState,
    pub connects: u64,
    pub reconnects: u64,
    pub events_received: u64,
    pub events_forwarded: u64,
    pub dropped_backpressure: u64,
    pub dropped_rate_limit: u64,
    pub last_event_ms: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct SharedHealth(Arc<Mutex<HealthSnapshot>>);

impl SharedHealth {
    pub fn new(account_id: AccountId, platform: Platform, label: impl Into<String>) -> Self {
        Self(Arc::new(Mutex::new(HealthSnapshot {
            account_id,
            platform,
            label: label.into(),
            state: ConnectionState::Idle,
            connects: 0,
            reconnects: 0,
            events_received: 0,
            events_forwarded: 0,
            dropped_backpressure: 0,
            dropped_rate_limit: 0,
            last_event_ms: None,
            last_error: None,
        })))
    }
    pub fn update(&self, f: impl FnOnce(&mut HealthSnapshot)) {
        // A poisoned lock only means another task panicked mid-update; metrics stay usable.
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard);
    }
    pub fn snapshot(&self) -> HealthSnapshot {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn set_state(&self, state: ConnectionState) {
        self.update(|h| h.state = state);
    }
}
