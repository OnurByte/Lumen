//! Normalized, typed LIVE events shared by every platform.
use crate::platform::{AccountId, Platform};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveUser {
    /// Platform-scoped user id (stringified; never a credential).
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    Chat { text: String },
    Gift { gift_id: String, gift_name: Option<String>, count: u32, unit_value: Option<u32> },
    Like { count: u32, total: Option<u64> },
    Join,
    Follow,
    Share,
    ViewerCount { current: u64, total: Option<u64> },
    /// The platform reported that the LIVE ended.
    LiveEnded,
}

impl EventKind {
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::Chat { .. } => "chat",
            EventKind::Gift { .. } => "gift",
            EventKind::Like { .. } => "like",
            EventKind::Join => "join",
            EventKind::Follow => "follow",
            EventKind::Share => "share",
            EventKind::ViewerCount { .. } => "viewer_count",
            EventKind::LiveEnded => "live_ended",
        }
    }
    /// High-value events are never dropped by backpressure (they may wait instead).
    pub fn is_high_value(&self) -> bool {
        matches!(self, EventKind::Chat { .. } | EventKind::Gift { .. } | EventKind::Follow | EventKind::LiveEnded)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveEvent {
    pub platform: Platform,
    pub account_id: AccountId,
    pub room_id: String,
    /// Platform-scoped event id; adapters derive a stable content hash when none exists.
    pub event_id: String,
    /// Timestamp reported by the platform, if any (Unix ms).
    pub platform_ts_ms: Option<i64>,
    /// When Lumen received the event (Unix ms).
    pub received_ts_ms: i64,
    pub user: Option<LiveUser>,
    pub kind: EventKind,
}

impl LiveEvent {
    /// Deduplication key: unique per platform room and event.
    pub fn dedup_key(&self) -> String {
        format!("{}|{}|{}|{}", self.platform.short(), self.room_id, self.kind.name(), self.event_id)
    }
    /// Compact id (≤128 bytes) suitable for the lumend audience ingress.
    pub fn compact_id(&self) -> String {
        let mut id = format!("{}:{}:{}", self.platform.short(), self.account_id.short(), self.event_id);
        id.truncate(128);
        id
    }
}

/// Stable 64-bit FNV-1a hash used for synthetic event ids.
pub fn stable_hash(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in part.as_bytes().iter().chain(std::iter::once(&0xffu8)) {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("h{h:016x}")
}
