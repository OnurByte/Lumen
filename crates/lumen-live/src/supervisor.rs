//! Per-account connection supervision: connect, read, detect staleness, back off, cancel.
use crate::adapter::{AdapterError, LiveTarget, PlatformAdapter};
use crate::health::{ConnectionState, SharedHealth};
use crate::hub::AccountSink;
use crate::secret::Credentials;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, warn};

#[derive(Debug, Clone)]
pub struct Backoff {
    pub initial: Duration,
    pub max: Duration,
    pub offline_poll: Duration,
    pub rate_limited: Duration,
    pub connect_timeout: Duration,
    /// Give up after this many consecutive failures (None = never).
    pub max_consecutive_failures: Option<u32>,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(120),
            offline_poll: Duration::from_secs(60),
            rate_limited: Duration::from_secs(180),
            connect_timeout: Duration::from_secs(30),
            max_consecutive_failures: None,
        }
    }
}

impl Backoff {
    /// Exponential delay with ±20% jitter for the n-th consecutive failure (n ≥ 1).
    pub fn delay(&self, failures: u32, error: &AdapterError) -> Duration {
        let base = match error {
            AdapterError::Offline(_) => return jitter(self.offline_poll),
            AdapterError::RateLimited(_) => self.rate_limited,
            _ => self.initial,
        };
        let exp = base.saturating_mul(1u32 << failures.saturating_sub(1).min(16));
        jitter(exp.min(self.max.max(base)))
    }
}

fn jitter(d: Duration) -> Duration {
    let factor = 0.8 + rand::random::<f64>() * 0.4;
    d.mul_f64(factor)
}

/// Supervise one account until cancelled or a non-retryable error occurs.
/// Panics inside the adapter are contained to this account's task by the caller (`tokio::spawn`).
pub async fn supervise(
    adapter: Arc<dyn PlatformAdapter>,
    target: LiveTarget,
    credentials: Credentials,
    mut sink: AccountSink,
    health: SharedHealth,
    backoff: Backoff,
    cancel: CancellationToken,
) {
    let span = tracing::info_span!("account", platform = %target.platform, account = %target.account_id.short(), handle = %target.handle);
    async move {
        let mut failures: u32 = 0;
        let mut ever_connected = false;
        loop {
            if cancel.is_cancelled() {
                break;
            }
            health.set_state(ConnectionState::Connecting);
            let opened = tokio::select! {
                _ = cancel.cancelled() => break,
                r = tokio::time::timeout(backoff.connect_timeout, adapter.open_live(&target, &credentials, cancel.child_token())) =>
                    r.unwrap_or_else(|_| Err(AdapterError::Transient("connect timeout".into()))),
            };
            let error = match opened {
                Ok(mut session) => {
                    let room = session.room_id().to_owned();
                    info!(room = %room, "connected");
                    health.update(|h| {
                        h.state = ConnectionState::Live { room_id: room.clone() };
                        h.connects += 1;
                        if ever_connected { h.reconnects += 1; }
                        h.last_error = None;
                    });
                    ever_connected = true;
                    failures = 0;
                    let stale = adapter.stale_after();
                    loop {
                        let next = tokio::select! {
                            _ = cancel.cancelled() => None,
                            r = tokio::time::timeout(stale, session.next_events()) => Some(r),
                        };
                        match next {
                            None => break None,
                            Some(Err(_)) => break Some(AdapterError::Transient(format!("no data for {}s", stale.as_secs()))),
                            Some(Ok(Ok(None))) => break Some(AdapterError::Transient("stream ended".into())),
                            Some(Ok(Err(e))) => break Some(e),
                            Some(Ok(Ok(Some(events)))) => {
                                let n = events.len() as u64;
                                if n > 0 {
                                    health.update(|h| { h.events_received += n; h.last_event_ms = Some(crate::now_ms()); });
                                }
                                for event in events {
                                    let ended = matches!(event.kind, crate::event::EventKind::LiveEnded);
                                    sink.push(event).await;
                                    if ended {
                                        info!("platform reported LIVE ended");
                                    }
                                }
                            }
                        }
                    }
                    .unwrap_or(AdapterError::Cancelled)
                }
                Err(e) => e,
            };
            if cancel.is_cancelled() || matches!(error, AdapterError::Cancelled) {
                break;
            }
            if !error.is_retryable() {
                warn!(class = error.class(), error = %error, "giving up on account");
                health.update(|h| { h.state = ConnectionState::Failed { reason: error.to_string() }; h.last_error = Some(error.to_string()); });
                return;
            }
            failures += 1;
            if backoff.max_consecutive_failures.is_some_and(|m| failures >= m) {
                health.update(|h| { h.state = ConnectionState::Failed { reason: format!("{failures} consecutive failures: {error}") }; h.last_error = Some(error.to_string()); });
                return;
            }
            let delay = backoff.delay(failures, &error);
            warn!(class = error.class(), error = %error, retry_in_ms = delay.as_millis() as u64, "connection lost");
            health.update(|h| {
                h.state = if matches!(error, AdapterError::Offline(_)) { ConnectionState::Offline } else {
                    ConnectionState::Backoff { retry_in_ms: delay.as_millis() as u64, reason: error.class().into() }
                };
                h.last_error = Some(error.to_string());
            });
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(delay) => {}
            }
        }
        health.set_state(ConnectionState::Stopped);
    }
    .instrument(span)
    .await
}
