use crate::api::{DEFAULT_API_HOST, KwaiApi};
use crate::session::KwaiSession;
use async_trait::async_trait;
use lumen_live::adapter::AuthState;
use lumen_live::event::stable_hash;
use lumen_live::{AdapterError, BroadcastEndpoint, Capabilities, Credentials, LiveSession, LiveTarget, LoginSpec, Platform, PlatformAdapter, Profile, SecretString, SessionCapture, Support};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// Kwai GLOBAL adapter. Reads LIVE events anonymously (`attach: unlogin`), so no credentials
/// are needed; each account still gets its own device id, HTTP client and socket.
/// Settings: `device_id`, `api_host`, `socket` (host:port), `proxy`, `watch` (another creator),
/// and for broadcasting `studio_cover` (jpg/png ≤ 2 MB, 720×1280 recommended), `studio_caption`, `studio_host`.
/// Credential for broadcasting: `studio_cookie` (Cookie header of a logged-in studio.kwai.com session).
#[derive(Debug, Default, Clone)]
pub struct KwaiAdapter;

/// Stable per-account device id so accounts never share a device identity.
pub fn device_id(target: &LiveTarget) -> String {
    target.settings.get("device_id").cloned().unwrap_or_else(|| format!("ANDROID_{}", &stable_hash(&["kwai-device", target.account_id.as_str()])[1..17]))
}

fn api_for(target: &LiveTarget) -> Result<KwaiApi, AdapterError> {
    KwaiApi::new(target.settings.get("api_host").map_or(DEFAULT_API_HOST, String::as_str), &device_id(target), target.settings.get("proxy").map(String::as_str))
}

#[async_trait]
impl PlatformAdapter for KwaiAdapter {
    fn platform(&self) -> Platform {
        Platform::KwaiGlobal
    }
    fn capabilities(&self) -> Capabilities {
        // Verified 2026-11-08 against a real global Kwai LIVE room (docs/platforms.md).
        Capabilities {
            profile: Support::Verified,
            search: Support::Verified,
            live_events: Support::Verified,
            chat: Support::Verified,
            like: Support::Verified,
            join: Support::Verified,
            viewer_count: Support::Verified,
            gift: Support::Experimental,
            share: Support::Experimental,
            follow: Support::Unsupported,
            broadcast_rtmp: Support::Experimental,
            // Kwai Studio flow ported from studio.kwai.com's own JS; not yet run with a real account.
            broadcast_provision: Support::Experimental,
            ..Capabilities::NONE
        }
    }
    async fn authenticate(&self, _credentials: &Credentials) -> Result<AuthState, AdapterError> {
        Ok(AuthState::Anonymous)
    }
    async fn profile(&self, handle: &str, _credentials: &Credentials) -> Result<Profile, AdapterError> {
        let api = KwaiApi::new(DEFAULT_API_HOST, "ANDROID_0000000000000000", None)?;
        let json = api.search_user(handle).await?;
        parse_profile(&json, handle)
    }
    async fn provision_broadcast(&self, target: &LiveTarget, credentials: &Credentials) -> Result<BroadcastEndpoint, AdapterError> {
        let (bytes, kind) = cover(target)?;
        let caption = target.settings.get("studio_caption").map_or("", String::as_str);
        studio(target, credentials)?.create_room(caption, bytes, kind).await
    }
    fn login_spec(&self, target: &LiveTarget) -> Option<LoginSpec> {
        let origin = crate::studio::origin(target.settings.get("studio_host").map_or(crate::studio::DEFAULT_HOST, String::as_str));
        Some(LoginSpec { url: format!("{origin}/user/login?redirect=%2Flive%2Flist"), credential: "studio_cookie", capture: SessionCapture::Cookies(format!("{origin}/rest/")) })
    }
    async fn session_valid(&self, target: &LiveTarget, session: &SecretString) -> bool {
        let host = target.settings.get("studio_host").map_or(crate::studio::DEFAULT_HOST, String::as_str);
        match crate::studio::StudioClient::new(host, session, target.settings.get("proxy").map(String::as_str)) {
            Ok(client) => client.logged_in().await,
            Err(_) => false,
        }
    }
    async fn check_broadcast(&self, target: &LiveTarget, credentials: &Credentials) -> Result<String, AdapterError> {
        cover(target)?;
        studio(target, credentials)?.preflight().await
    }
    async fn start_broadcast(&self, target: &LiveTarget, credentials: &Credentials, endpoint: &BroadcastEndpoint) -> Result<(), AdapterError> {
        studio(target, credentials)?.start_live(room_id(endpoint)?).await
    }
    async fn stop_broadcast(&self, target: &LiveTarget, credentials: &Credentials, endpoint: &BroadcastEndpoint) -> Result<(), AdapterError> {
        studio(target, credentials)?.stop_live(room_id(endpoint)?).await
    }
    async fn open_live(&self, target: &LiveTarget, _credentials: &Credentials, cancel: CancellationToken) -> Result<Box<dyn LiveSession>, AdapterError> {
        let api = api_for(target)?;
        let handle = target.settings.get("watch").unwrap_or(&target.handle);
        let room = api.find_live(handle).await?;
        let ticket = api.start_play(&room).await?;
        Ok(Box::new(KwaiSession::connect(&api, target, room, ticket, target.settings.get("socket").map(String::as_str), cancel).await?))
    }
}

fn studio(target: &LiveTarget, credentials: &Credentials) -> Result<crate::studio::StudioClient, AdapterError> {
    let cookie = credentials.get("studio_cookie").ok_or_else(|| AdapterError::Auth("add a `studio_cookie` credential (log in at studio.kwai.com, copy the Cookie header)".into()))?;
    crate::studio::StudioClient::new(target.settings.get("studio_host").map_or(crate::studio::DEFAULT_HOST, String::as_str), cookie, target.settings.get("proxy").map(String::as_str))
}

fn cover(target: &LiveTarget) -> Result<(Vec<u8>, &'static str), AdapterError> {
    let path = target.settings.get("studio_cover").ok_or_else(|| AdapterError::Unsupported("set `studio_cover` to a jpg/png cover image (Kwai requires one)".into()))?;
    let bytes = std::fs::read(path).map_err(|e| AdapterError::Unsupported(format!("cannot read studio_cover: {e}")))?;
    let kind = match bytes.as_slice() {
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        _ => return Err(AdapterError::Unsupported("studio_cover must be a JPEG or PNG image".into())),
    };
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(AdapterError::Unsupported("studio_cover must be at most 2 MB".into()));
    }
    Ok((bytes, kind))
}

fn room_id(endpoint: &BroadcastEndpoint) -> Result<&str, AdapterError> {
    endpoint.room_id.as_deref().ok_or_else(|| AdapterError::Protocol("Kwai broadcast without a Studio room id".into()))
}

pub fn parse_profile(json: &Value, handle: &str) -> Result<Profile, AdapterError> {
    let wanted = handle.trim_start_matches('@').to_lowercase();
    let users = json.get("users").and_then(Value::as_array).ok_or_else(|| AdapterError::Protocol("no users".into()))?;
    let u = users
        .iter()
        .find(|u| u.get("kwai_id").and_then(Value::as_str).is_some_and(|v| v.to_lowercase() == wanted))
        .ok_or_else(|| AdapterError::NotFound(format!("Kwai user {handle}")))?;
    let s = |k: &str| u.get(k).and_then(Value::as_str).map(str::to_owned).filter(|v| !v.is_empty());
    Ok(Profile {
        platform: Platform::KwaiGlobal,
        user_id: u.get("user_id").map(|v| v.to_string().trim_matches('"').to_owned()).unwrap_or_default(),
        handle: s("kwai_id").unwrap_or_default(),
        display_name: s("user_name").unwrap_or_default(),
        avatar_url: s("headurl"),
        followers: u.get("fans_num").and_then(Value::as_u64),
        following: None,
        likes: None,
        bio: s("user_text"),
        country: None,
        live_room_id: u.pointer("/liveInfo/liveStreamId").and_then(Value::as_str).map(str::to_owned),
    })
}
