use crate::api::{DEFAULT_API_HOST, KwaiApi};
use crate::session::KwaiSession;
use async_trait::async_trait;
use lumen_live::adapter::AuthState;
use lumen_live::event::stable_hash;
use lumen_live::{AdapterError, Capabilities, Credentials, LiveSession, LiveTarget, Platform, PlatformAdapter, Profile, Support};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// Kwai GLOBAL adapter. Reads LIVE events anonymously (`attach: unlogin`), so no credentials
/// are needed; each account still gets its own device id, HTTP client and socket.
/// Settings: `device_id`, `api_host`, `socket` (host:port), `proxy`, `watch` (another creator).
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
    async fn open_live(&self, target: &LiveTarget, _credentials: &Credentials, cancel: CancellationToken) -> Result<Box<dyn LiveSession>, AdapterError> {
        let api = api_for(target)?;
        let handle = target.settings.get("watch").unwrap_or(&target.handle);
        let room = api.find_live(handle).await?;
        let ticket = api.start_play(&room).await?;
        Ok(Box::new(KwaiSession::connect(&api, target, room, ticket, target.settings.get("socket").map(String::as_str), cancel).await?))
    }
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
