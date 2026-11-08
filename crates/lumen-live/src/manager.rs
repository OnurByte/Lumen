//! Runs many accounts concurrently. Each account gets its own task, cancellation token,
//! queue, health record and backoff state; a panic or failure in one account's adapter is
//! contained to that account and restarted with backoff.
use crate::adapter::{LiveTarget, PlatformAdapter};
use crate::health::{ConnectionState, HealthSnapshot, SharedHealth};
use crate::hub::EventHub;
use crate::platform::{AccountId, Platform};
use crate::secret::Credentials;
use crate::supervisor::{Backoff, supervise};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const MAX_PANICS: u32 = 5;

struct Running {
    cancel: CancellationToken,
    health: SharedHealth,
    handle: JoinHandle<()>,
}

pub struct LiveManager {
    hub: EventHub,
    adapters: Mutex<HashMap<Platform, Arc<dyn PlatformAdapter>>>,
    running: Mutex<HashMap<AccountId, Running>>,
    backoff: Backoff,
    cancel: CancellationToken,
}

impl LiveManager {
    pub fn new(hub: EventHub, backoff: Backoff, cancel: CancellationToken) -> Self {
        Self { hub, adapters: Mutex::new(HashMap::new()), running: Mutex::new(HashMap::new()), backoff, cancel }
    }

    pub fn hub(&self) -> &EventHub {
        &self.hub
    }

    pub fn register_adapter(&self, adapter: Arc<dyn PlatformAdapter>) {
        self.adapters.lock().unwrap_or_else(|e| e.into_inner()).insert(adapter.platform(), adapter);
    }

    pub fn adapter(&self, platform: Platform) -> Option<Arc<dyn PlatformAdapter>> {
        self.adapters.lock().unwrap_or_else(|e| e.into_inner()).get(&platform).cloned()
    }

    /// Start (or restart) reading LIVE events for one account.
    pub fn start(&self, target: LiveTarget, credentials: Credentials, label: &str) -> Result<SharedHealth, String> {
        let adapter = self.adapter(target.platform).ok_or_else(|| format!("no adapter registered for {}", target.platform))?;
        self.stop(&target.account_id);
        let account = target.account_id.clone();
        let health = SharedHealth::new(account.clone(), target.platform, label);
        let cancel = self.cancel.child_token();
        let hub = self.hub.clone();
        let backoff = self.backoff.clone();
        let (h, c) = (health.clone(), cancel.clone());
        let handle = tokio::spawn(async move {
            let mut panics = 0u32;
            loop {
                let sink = hub.register(target.account_id.clone(), h.clone());
                let task = tokio::spawn(supervise(adapter.clone(), target.clone(), credentials.clone(), sink, h.clone(), backoff.clone(), c.clone()));
                match task.await {
                    Ok(()) => break,
                    Err(join) if join.is_panic() => {
                        panics += 1;
                        tracing::error!(account = %target.account_id.short(), platform = %target.platform, panics, "adapter panicked; account isolated");
                        h.update(|s| s.last_error = Some(format!("adapter panic #{panics}")));
                        if panics >= MAX_PANICS {
                            h.set_state(ConnectionState::Failed { reason: format!("adapter panicked {panics} times") });
                            break;
                        }
                        let delay = backoff.initial.saturating_mul(1 << panics.min(6));
                        h.set_state(ConnectionState::Backoff { retry_in_ms: delay.as_millis() as u64, reason: "panic".into() });
                        tokio::select! { _ = c.cancelled() => { h.set_state(ConnectionState::Stopped); break; } _ = tokio::time::sleep(delay) => {} }
                    }
                    Err(_) => break,
                }
            }
        });
        self.running.lock().unwrap_or_else(|e| e.into_inner()).insert(account, Running { cancel, health: health.clone(), handle });
        Ok(health)
    }

    /// Stop one account; other accounts are unaffected.
    pub fn stop(&self, account: &AccountId) -> bool {
        let removed = self.running.lock().unwrap_or_else(|e| e.into_inner()).remove(account);
        if let Some(r) = removed {
            r.cancel.cancel();
            r.health.set_state(ConnectionState::Stopped);
            true
        } else {
            false
        }
    }

    pub fn is_running(&self, account: &AccountId) -> bool {
        self.running.lock().unwrap_or_else(|e| e.into_inner()).get(account).is_some_and(|r| !r.handle.is_finished())
    }

    pub fn health(&self) -> Vec<HealthSnapshot> {
        let mut out: Vec<_> = self.running.lock().unwrap_or_else(|e| e.into_inner()).values().map(|r| r.health.snapshot()).collect();
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out
    }

    /// Cancel every account and wait (bounded) for tasks to finish.
    pub async fn shutdown(&self, grace: Duration) {
        let all: Vec<Running> = self.running.lock().unwrap_or_else(|e| e.into_inner()).drain().map(|(_, r)| r).collect();
        for r in &all {
            r.cancel.cancel();
        }
        for r in all {
            let _ = tokio::time::timeout(grace, r.handle).await;
            r.health.set_state(ConnectionState::Stopped);
        }
    }
}
