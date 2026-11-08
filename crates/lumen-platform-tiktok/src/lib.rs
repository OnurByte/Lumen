//! TikTok LIVE adapter. Each Lumen account opens its own WebSocket session with its own
//! (optional) cookies, proxy, language and region — nothing is shared between accounts.
//!
//! Credentials (all optional, stored encrypted by the Account Center):
//! * `cookie` — a browser cookie string for the account (needed for age-restricted rooms).
//!
//! Settings (non-secret): `proxy`, `language`, `region`, `watch` (read another creator's room).
use async_trait::async_trait;
use lumen_live::adapter::AuthState;
use lumen_live::event::stable_hash;
use lumen_live::{AdapterError, Capabilities, Credentials, EventKind, LiveEvent, LiveSession, LiveTarget, LiveUser, Platform, PlatformAdapter, Profile, Support, now_ms};
use piratetok_live_rs::errors::TikTokLiveError;
use piratetok_live_rs::structs::TikTokLiveEvent;
use piratetok_live_rs::structs::proto::user::UserIdentity;
use piratetok_live_rs::{TikTokLive, TikTokLiveStream};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Default, Clone)]
pub struct TikTokAdapter {
    /// Set once live events were observed end-to-end from the real service in this build's
    /// validation run (see docs/platforms.md); reported via capabilities.
    pub live_verified: bool,
}

pub fn map_error(e: TikTokLiveError) -> AdapterError {
    use TikTokLiveError as E;
    match e {
        E::UserNotFound(u) => AdapterError::NotFound(format!("TikTok user @{u}")),
        E::ProfilePrivate(u) => AdapterError::NotFound(format!("TikTok profile @{u} is private")),
        E::HostNotOnline(u) => AdapterError::Offline(format!("@{u} is not LIVE")),
        E::TikTokBlocked(m) => AdapterError::RateLimited(m),
        E::AgeRestricted(m) => AdapterError::Auth(format!("age-restricted room; add a `cookie` credential ({m})")),
        E::SessionRequired(m) => AdapterError::Auth(format!("session cookie required ({m})")),
        E::Proto(e) => AdapterError::Protocol(e.to_string()),
        E::Json(e) => AdapterError::Protocol(e.to_string()),
        E::Decode(m) | E::InvalidResponse(m) => AdapterError::Protocol(m),
        E::RoomIdMissing => AdapterError::Protocol("room id missing".into()),
        E::ApiError(code) => AdapterError::Transient(format!("tiktok api statusCode={code}")),
        other => AdapterError::Transient(other.to_string()),
    }
}

fn user(u: &Option<UserIdentity>) -> Option<LiveUser> {
    u.as_ref().filter(|u| u.user_id > 0).map(|u| LiveUser { id: u.user_id.to_string(), display_name: u.nickname.clone() })
}

/// Pure mapping from a TikTok event to the normalized model (fixture-tested).
pub fn map_event(event: TikTokLiveEvent, target: &LiveTarget, room_id: &str, received_ts_ms: i64) -> Option<LiveEvent> {
    let make = |id: String, ts: Option<i64>, user: Option<LiveUser>, kind: EventKind| LiveEvent {
        platform: Platform::TikTok,
        account_id: target.account_id.clone(),
        room_id: room_id.to_owned(),
        event_id: id,
        platform_ts_ms: ts.filter(|t| *t > 0),
        received_ts_ms,
        user,
        kind,
    };
    macro_rules! common {
        ($m:expr) => {{
            let c = $m.common.as_ref();
            (c.map(|c| c.msg_id).filter(|id| *id > 0), c.map(|c| c.create_time))
        }};
    }
    match event {
        TikTokLiveEvent::Chat(m) => {
            let (id, ts) = common!(m);
            let text = m.comment.trim().to_owned();
            if text.is_empty() {
                return None;
            }
            let u = user(&m.user);
            let id = id.map(|i| i.to_string()).unwrap_or_else(|| stable_hash(&["chat", u.as_ref().map_or("", |u| &u.id), &text, &ts.unwrap_or(0).to_string()]));
            Some(make(id, ts, u, EventKind::Chat { text }))
        }
        TikTokLiveEvent::Gift(m) => {
            let combo = m.gift_details.as_ref().is_some_and(|g| g.combo || g.gift_type == 1);
            if combo && m.repeat_end != 1 {
                return None; // streak in progress: only the final message carries the total
            }
            let (id, ts) = common!(m);
            let details = m.gift_details.as_ref();
            Some(make(
                id?.to_string(),
                ts,
                user(&m.user),
                EventKind::Gift {
                    gift_id: m.gift_id.to_string(),
                    gift_name: details.map(|d| d.gift_name.clone()).filter(|n| !n.is_empty()),
                    count: m.repeat_count.max(1) as u32,
                    unit_value: details.map(|d| d.diamond_count).filter(|d| *d > 0).map(|d| d as u32),
                },
            ))
        }
        TikTokLiveEvent::Like(m) => {
            let (id, ts) = common!(m);
            Some(make(id?.to_string(), ts, user(&m.user), EventKind::Like { count: m.like_count.max(0) as u32, total: u64::try_from(m.total_like_count).ok() }))
        }
        TikTokLiveEvent::Join(m) => {
            let (id, ts) = common!(m);
            Some(make(id?.to_string(), ts, user(&m.user), EventKind::Join))
        }
        TikTokLiveEvent::Follow(m) => {
            let (id, ts) = common!(m);
            Some(make(id?.to_string(), ts, user(&m.user), EventKind::Follow))
        }
        TikTokLiveEvent::Share(m) => {
            let (id, ts) = common!(m);
            Some(make(id?.to_string(), ts, user(&m.user), EventKind::Share))
        }
        TikTokLiveEvent::RoomUserSeq(m) => {
            let (id, ts) = common!(m);
            Some(make(
                id?.to_string(),
                ts,
                None,
                EventKind::ViewerCount { current: u64::try_from(m.viewer_count).unwrap_or(0), total: u64::try_from(m.total_user).ok().filter(|t| *t > 0) },
            ))
        }
        TikTokLiveEvent::LiveEnded(m) => {
            let (id, ts) = common!(m);
            Some(make(id.map(|i| i.to_string()).unwrap_or_else(|| format!("ended-{room_id}")), ts, None, EventKind::LiveEnded))
        }
        _ => None,
    }
}

struct TikTokSession {
    stream: TikTokLiveStream,
    target: LiveTarget,
    room_id: String,
    cancel: CancellationToken,
}

#[async_trait]
impl LiveSession for TikTokSession {
    fn room_id(&self) -> &str {
        &self.room_id
    }
    async fn next_events(&mut self) -> Result<Option<Vec<LiveEvent>>, AdapterError> {
        loop {
            let event = tokio::select! {
                _ = self.cancel.cancelled() => return Err(AdapterError::Cancelled),
                e = self.stream.next_event() => e.map_err(map_error)?,
            };
            match event {
                TikTokLiveEvent::Connected { room_id } => {
                    self.room_id = room_id;
                    continue;
                }
                TikTokLiveEvent::Disconnected => return Ok(None),
                TikTokLiveEvent::Reconnecting { attempt, .. } => {
                    tracing::debug!(attempt, "tiktok transport reconnecting");
                    continue;
                }
                other => {
                    if let Some(e) = map_event(other, &self.target, &self.room_id, now_ms()) {
                        return Ok(Some(vec![e]));
                    }
                }
            }
        }
    }
}

fn watched_handle(target: &LiveTarget) -> String {
    target.settings.get("watch").cloned().unwrap_or_else(|| target.handle.clone())
}

#[async_trait]
impl PlatformAdapter for TikTokAdapter {
    fn platform(&self) -> Platform {
        Platform::TikTok
    }
    fn capabilities(&self) -> Capabilities {
        let live = if self.live_verified { Support::Verified } else { Support::Experimental };
        Capabilities {
            profile: Support::Experimental,
            live_events: live,
            chat: live,
            gift: live,
            like: live,
            join: live,
            follow: live,
            share: live,
            viewer_count: live,
            broadcast_rtmp: Support::Experimental,
            ..Capabilities::NONE
        }
    }
    async fn authenticate(&self, credentials: &Credentials) -> Result<AuthState, AdapterError> {
        match credentials.get("cookie") {
            None => Ok(AuthState::Anonymous),
            Some(c) if c.expose().contains('=') && !c.expose().chars().any(|ch| ch.is_control()) => Ok(AuthState::Present),
            Some(_) => Err(AdapterError::Auth("cookie credential must look like 'name=value; ...'".into())),
        }
    }
    async fn profile(&self, handle: &str, credentials: &Credentials) -> Result<Profile, AdapterError> {
        use piratetok_live_rs::http::api::{FetchParams, fetch_room_id};
        use piratetok_live_rs::structs::config::Endpoints;
        let endpoints = Endpoints::tiktok();
        let cookie = credentials.get("cookie").map(|c| c.expose().to_owned());
        let params = FetchParams { timeout: Duration::from_secs(15), cookies: cookie.as_deref(), user_agent: None, proxy: None, language: None, region: None, endpoints: &endpoints };
        let room = fetch_room_id(handle, params.clone()).await;
        let (live_room_id, user_id) = match room {
            Ok(r) => (Some(r.room_id).filter(|r| !r.is_empty() && r != "0"), r.anchor_id),
            Err(TikTokLiveError::HostNotOnline(_)) => (None, String::new()),
            Err(e) => return Err(map_error(e)),
        };
        Ok(Profile { platform: Platform::TikTok, user_id, handle: handle.trim_start_matches('@').into(), display_name: handle.trim_start_matches('@').into(), avatar_url: None, followers: None, following: None, likes: None, bio: None, country: None, live_room_id })
    }
    async fn open_live(&self, target: &LiveTarget, credentials: &Credentials, cancel: CancellationToken) -> Result<Box<dyn LiveSession>, AdapterError> {
        self.authenticate(credentials).await?;
        let handle = watched_handle(target);
        let mut builder = TikTokLive::builder(&handle).timeout(Duration::from_secs(15)).max_retries(1).stale_timeout(Duration::from_secs(60));
        if let Some(c) = credentials.get("cookie") {
            builder = builder.cookies(c.expose());
        }
        if let Some(p) = target.settings.get("proxy") {
            builder = builder.proxy(p.clone());
        }
        if let Some(l) = target.settings.get("language") {
            builder = builder.language(l.clone());
        }
        if let Some(r) = target.settings.get("region") {
            builder = builder.region(r.clone());
        }
        let mut stream = tokio::select! {
            _ = cancel.cancelled() => return Err(AdapterError::Cancelled),
            s = builder.connect() => s.map_err(map_error)?,
        };
        // connect() always emits Connected first; take it so the room id is known up front.
        let room_id = match stream.next_event().await.map_err(map_error)? {
            TikTokLiveEvent::Connected { room_id } => room_id,
            _ => String::new(),
        };
        Ok(Box::new(TikTokSession { stream, target: target.clone(), room_id, cancel }))
    }
}
