//! Platform-neutral LIVE core for Lumen.
//!
//! Adapters (TikTok, Kwai Global, Likee Global) implement [`PlatformAdapter`]; everything
//! downstream (supervision, deduplication, moderation, the AI inbox) only sees the typed
//! [`LiveEvent`] model and never platform-specific structures.
pub mod adapter;
pub mod bridge;
pub mod dedup;
pub mod event;
pub mod health;
pub mod hub;
pub mod manager;
pub mod mock;
pub mod moderation;
pub mod platform;
pub mod ratelimit;
pub mod secret;
pub mod supervisor;

pub use adapter::{AdapterError, LiveSession, LiveTarget, PlatformAdapter, Profile};
pub use event::{EventKind, LiveEvent, LiveUser};
pub use manager::LiveManager;
pub use health::{ConnectionState, HealthSnapshot, SharedHealth};
pub use platform::{AccountId, Capabilities, Platform, Support};
pub use secret::{Credentials, SecretString};

/// Wall-clock milliseconds since the Unix epoch (0 if the clock is before 1970).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
