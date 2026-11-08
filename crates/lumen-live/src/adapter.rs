//! The contract every platform connector implements.
use crate::event::LiveEvent;
use crate::platform::{AccountId, Capabilities, Platform};
use crate::secret::Credentials;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    /// Credentials missing, rejected or expired. Not retried automatically.
    #[error("authentication failed: {0}")]
    Auth(String),
    /// The target account/room does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// The target exists but is not LIVE right now. Retried slowly.
    #[error("not live: {0}")]
    Offline(String),
    /// The platform throttled us. Retried with longer backoff.
    #[error("rate limited: {0}")]
    RateLimited(String),
    /// The capability is not available for this platform.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Wire/protocol data did not match expectations (possible upstream change).
    #[error("protocol error: {0}")]
    Protocol(String),
    /// Network or transient server failure. Retried with backoff.
    #[error("transient: {0}")]
    Transient(String),
    #[error("cancelled")]
    Cancelled,
}

impl AdapterError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AdapterError::Offline(_) | AdapterError::RateLimited(_) | AdapterError::Protocol(_) | AdapterError::Transient(_)
        )
    }
    pub fn class(&self) -> &'static str {
        match self {
            AdapterError::Auth(_) => "auth",
            AdapterError::NotFound(_) => "not_found",
            AdapterError::Offline(_) => "offline",
            AdapterError::RateLimited(_) => "rate_limited",
            AdapterError::Unsupported(_) => "unsupported",
            AdapterError::Protocol(_) => "protocol",
            AdapterError::Transient(_) => "transient",
            AdapterError::Cancelled => "cancelled",
        }
    }
}

/// What to connect to on behalf of which account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveTarget {
    pub account_id: AccountId,
    pub platform: Platform,
    /// The broadcaster handle whose LIVE room is read (usually the account itself).
    pub handle: String,
    /// Adapter-specific non-secret settings (e.g. region, device id override).
    #[serde(default)]
    pub settings: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub platform: Platform,
    pub user_id: String,
    pub handle: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub followers: Option<u64>,
    pub following: Option<u64>,
    pub likes: Option<u64>,
    pub bio: Option<String>,
    pub country: Option<String>,
    /// LIVE room id if the platform says the user is currently live.
    pub live_room_id: Option<String>,
}

/// Outcome of validating credentials without opening a LIVE connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthState {
    /// No credentials needed for read-only LIVE events.
    Anonymous,
    /// Credentials present and structurally valid; not checked server-side.
    Present,
    /// Credentials verified against the service.
    Verified,
}

/// Where to push video for one broadcast, as issued by the platform (or entered by the operator).
#[derive(Debug, Clone)]
pub struct BroadcastEndpoint {
    pub ingest_url: String,
    pub stream_key: crate::secret::SecretString,
    /// Platform room/session id needed to start or stop the broadcast, if any.
    pub room_id: Option<String>,
}

/// An open LIVE connection. Dropping it must close the underlying socket.
#[async_trait]
pub trait LiveSession: Send {
    fn room_id(&self) -> &str;
    /// Next batch of normalized events. `Ok(None)` means the stream ended cleanly.
    async fn next_events(&mut self) -> Result<Option<Vec<LiveEvent>>, AdapterError>;
}

#[async_trait]
pub trait PlatformAdapter: Send + Sync + 'static {
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> Capabilities;
    /// Validate credentials for this platform (structure, and server-side where supported).
    async fn authenticate(&self, credentials: &Credentials) -> Result<AuthState, AdapterError>;
    async fn profile(&self, handle: &str, credentials: &Credentials) -> Result<Profile, AdapterError>;
    /// Open a LIVE event connection. Implementations must honour `cancel`.
    async fn open_live(
        &self,
        target: &LiveTarget,
        credentials: &Credentials,
        cancel: CancellationToken,
    ) -> Result<Box<dyn LiveSession>, AdapterError>;
    /// Ask the platform for an ingest endpoint for this account (e.g. Kwai Studio creates a room
    /// and returns its push URL). Platforms where the operator copies the key by hand return
    /// `Unsupported`; their stored `ingest_url` + `stream_key` are used instead.
    async fn provision_broadcast(&self, _target: &LiveTarget, _credentials: &Credentials) -> Result<BroadcastEndpoint, AdapterError> {
        Err(AdapterError::Unsupported(format!("{}: copy the server URL and stream key from the platform's own tools", self.platform())))
    }
    /// Tell the platform the stream is flowing and the LIVE should start (no-op where pushing
    /// video starts the LIVE by itself).
    async fn start_broadcast(&self, _target: &LiveTarget, _credentials: &Credentials, _endpoint: &BroadcastEndpoint) -> Result<(), AdapterError> {
        Ok(())
    }
    /// End the platform-side LIVE (no-op where stopping the push ends it).
    async fn stop_broadcast(&self, _target: &LiveTarget, _credentials: &Credentials, _endpoint: &BroadcastEndpoint) -> Result<(), AdapterError> {
        Ok(())
    }
    /// Silence after which a connection is considered stale.
    fn stale_after(&self) -> Duration {
        Duration::from_secs(90)
    }
}
