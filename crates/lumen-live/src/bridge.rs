//! Combined inbox → moderation → lumend audience ingress (the AI agents' only view of chat).
use crate::event::{EventKind, LiveEvent};
use crate::moderation::{Moderator, Verdict};
use lumen_protocol::{AudienceInput, Command};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct BridgeStats {
    pub forwarded: u64,
    pub held: u64,
    pub rejected: u64,
    pub skipped: u64,
    pub ingress_errors: u64,
}

/// The audience prompt for an event, if it should reach the show. Gifts become short cues so
/// hosts can react; joins, likes and viewer counts stay out of prompts.
pub fn audience_input(event: &LiveEvent) -> Option<AudienceInput> {
    let user = event.user.as_ref()?;
    let text = match &event.kind {
        EventKind::Chat { text } => text.trim().to_owned(),
        EventKind::Gift { gift_name, count, .. } => format!("[{} gift: {} x{count}]", event.platform, gift_name.as_deref().unwrap_or("gift")),
        _ => return None,
    };
    let mut who = format!("{}:{}:{}", event.platform.short(), event.account_id.short(), user.id);
    who.truncate(128);
    Some(AudienceInput { id: event.compact_id(), user: who, text: text.chars().take(1000).collect() })
}

/// Forward until the inbox closes. Ingress failures are counted, never fatal: the show keeps
/// running without audience input rather than crashing the connectors.
pub async fn forward_to_lumend(mut inbox: mpsc::Receiver<LiveEvent>, socket: PathBuf, mut moderator: Moderator) -> BridgeStats {
    let mut stats = BridgeStats::default();
    while let Some(event) = inbox.recv().await {
        match moderator.review(&event, Instant::now()) {
            Verdict::Allow => {}
            Verdict::Hold(reason) => {
                stats.held += 1;
                tracing::info!(platform = %event.platform, account = %event.account_id.short(), reason, "held for operator review");
                continue;
            }
            Verdict::Reject(reason) => {
                stats.rejected += 1;
                tracing::debug!(reason, "rejected by moderation");
                continue;
            }
        }
        let Some(input) = audience_input(&event) else {
            stats.skipped += 1;
            continue;
        };
        let path = socket.clone();
        let result = tokio::task::spawn_blocking(move || lumen_protocol::request(&path, Command::Audience(input), Duration::from_secs(2))).await;
        match result {
            Ok(Ok(r)) if r.error.is_none() => stats.forwarded += 1,
            Ok(Ok(_)) => stats.rejected += 1,
            _ => {
                stats.ingress_errors += 1;
                tracing::warn!("lumend audience ingress unavailable");
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::LiveUser;
    use crate::platform::{AccountId, Platform};
    #[test]
    fn inputs_are_namespaced_per_platform_and_account() {
        let e = LiveEvent { platform: Platform::KwaiGlobal, account_id: AccountId("abcdef123456".into()), room_id: "r".into(), event_id: "99".into(), platform_ts_ms: None, received_ts_ms: 0, user: Some(LiveUser { id: "7".into(), display_name: "x".into() }), kind: EventKind::Chat { text: " hi ".into() } };
        let i = audience_input(&e).unwrap();
        assert_eq!((i.id.as_str(), i.user.as_str(), i.text.as_str()), ("kw:abcdef12:99", "kw:abcdef12:7", "hi"));
        let like = LiveEvent { kind: EventKind::Like { count: 1, total: None }, ..e };
        assert!(audience_input(&like).is_none());
    }
}
