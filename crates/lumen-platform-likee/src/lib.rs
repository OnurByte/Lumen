//! Likee GLOBAL web API, reimplemented in Rust from lucasintel/likee (Ruby, BSD-3) and
//! lucasintel/likee.cr (Crystal). Endpoints are unofficial and may change.
//!
//! Endpoints and the guest-session flow were rechecked on 2026-11-08 against Likee's live site and
//! its current web bundle. Profile, creator videos, trending hashtags and comments work; search and
//! trending videos answer `52010 system busy`. LIVE events come from the public web viewer protocol
//! (see [`live`]) and still need a real share link to validate (docs/platforms.md).
pub mod live;
use async_trait::async_trait;
use lumen_live::adapter::AuthState;
use lumen_live::{AdapterError, Capabilities, Credentials, LiveSession, LiveTarget, LoginSpec, Platform, PlatformAdapter, Profile, SecretString, SessionCapture, Support};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const API_BASE: &str = "https://api.like-video.com/likee-activity-flow-micro";
pub const WEB_BASE: &str = "https://likee.video";
const NO_STREAM_KEY_API: &str = "Likee has no known API that issues a stream key; store an RTMP URL and `stream_key` if Likee gave you one, otherwise use the official Likee Streamer app";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Video {
    pub id: String,
    pub creator_id: String,
    pub creator_username: String,
    pub creator_nickname: String,
    pub description: String,
    pub video_url: String,
    pub thumbnail_url: String,
    pub posted_at: Option<i64>,
    pub likes: u64,
    pub comments: u64,
    pub plays: u64,
    pub shares: u64,
    pub country: Option<String>,
    pub hashtags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hashtag {
    pub id: String,
    pub name: String,
    pub videos: u64,
    pub plays: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comment {
    pub id: String,
    pub user_id: String,
    pub username: String,
    pub nickname: String,
    pub text: String,
    pub created_at: Option<i64>,
    pub likes: u64,
}

fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}
fn n(v: &Value, k: &str) -> u64 {
    match v.get(k) {
        Some(Value::Number(x)) => x.as_u64().unwrap_or(0),
        Some(Value::String(x)) => x.parse().unwrap_or(0),
        _ => 0,
    }
}
fn opt(x: String) -> Option<String> {
    (!x.is_empty()).then_some(x)
}

/// Unwrap `{code: 0, data: ...}` envelopes.
pub fn data(body: &Value) -> Result<&Value, AdapterError> {
    match body.get("code").and_then(Value::as_i64) {
        Some(0) => body.get("data").ok_or_else(|| AdapterError::Protocol("response without data".into())),
        Some(code) => Err(AdapterError::Transient(format!("likee code={code} {}{}", s(body, "msg"), s(body, "message")))),
        None => Err(AdapterError::Protocol("response without code".into())),
    }
}

pub fn parse_video(v: &Value) -> Video {
    let hashtags = v
        .get("hashtagInfos")
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str::<Vec<Value>>(raw).ok())
        .unwrap_or_default()
        .iter()
        .map(|t| s(t, "hs_tg"))
        .filter(|t| !t.is_empty())
        .collect();
    Video {
        id: s(v, "postId"),
        creator_id: s(v, "posterUid"),
        creator_username: s(v, "likeeId"),
        creator_nickname: s(v, "nickname"),
        description: s(v, "msgText"),
        video_url: s(v, "videoUrl"),
        thumbnail_url: s(v, "coverUrl"),
        posted_at: s(v, "postTime").parse().ok(),
        likes: n(v, "likeCount"),
        comments: n(v, "commentCount"),
        plays: n(v, "playCount"),
        shares: n(v, "shareCount"),
        country: opt(s(v, "country")).or_else(|| opt(s(v, "videoCountry"))),
        hashtags,
    }
}

pub fn parse_videos(body: &Value) -> Result<Vec<Video>, AdapterError> {
    Ok(data(body)?.get("videoList").and_then(Value::as_array).map(|l| l.iter().map(parse_video).collect()).unwrap_or_default())
}

pub fn parse_hashtags(body: &Value) -> Result<Vec<Hashtag>, AdapterError> {
    let list = data(body)?.get("eventList").and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(list.iter().map(|h| Hashtag { id: s(h, "eventId"), name: s(h, "tagName"), videos: n(h, "postCnt"), plays: n(h, "playCnt") }).collect())
}

pub fn parse_comments(body: &Value) -> Result<Vec<Comment>, AdapterError> {
    let list = data(body)?.as_array().cloned().unwrap_or_default();
    Ok(list
        .iter()
        .map(|c| {
            let msg: Value = c.get("comMsg").and_then(Value::as_str).and_then(|m| serde_json::from_str(m).ok()).unwrap_or(Value::Null);
            Comment { id: s(c, "commentId"), user_id: s(c, "uid"), username: s(c, "userName"), nickname: s(c, "nickName"), text: s(&msg, "txt"), created_at: s(c, "commentTime").parse().ok(), likes: n(c, "likeCount") }
        })
        .collect())
}

/// `official_website/WebView/getProfileDetail` response → profile.
pub fn parse_profile(body: &Value) -> Result<Profile, AdapterError> {
    let u = data(body)?.get("userinfo").filter(|u| u.is_object()).ok_or_else(|| AdapterError::NotFound("Likee user not found".into()))?;
    Ok(Profile {
        platform: Platform::LikeeGlobal,
        user_id: s(u, "uid"),
        handle: s(u, "user_name"),
        display_name: s(u, "nick_name"),
        avatar_url: opt(s(u, "data1")),
        followers: Some(n(u, "fansCount")),
        following: Some(n(u, "followCount")),
        likes: Some(n(u, "allLikeCount")),
        bio: opt(s(u, "bio")),
        country: None,
        live_room_id: None,
    })
}

/// Web session: a logged-in one, or an anonymous one from `user/getGuestInfoForPC`
/// (video endpoints return empty lists with neither).
#[derive(Clone, Deserialize)]
struct Guest {
    uid: String,
    #[serde(rename = "deviceId")]
    device_id: String,
    token: String,
    #[serde(default = "normal_channel")]
    channel: String,
}
impl std::fmt::Debug for Guest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Guest([redacted])")
    }
}
fn normal_channel() -> String {
    "normal".into()
}

/// Reads the session the Likee website keeps in localStorage after a login.
const SESSION_SCRIPT: &str = "(()=>{try{const s=JSON.parse(localStorage.getItem('page-homepage-pc'));return s&&s.web_token&&s.web_uid?JSON.stringify({token:s.web_token,uid:String(s.web_uid),deviceId:s.web_deviceid||''}):''}catch(e){return ''}})()";

#[derive(Debug, Clone)]
pub struct LikeeClient {
    http: reqwest::Client,
    session: Option<Guest>,
}

impl LikeeClient {
    pub fn new(proxy: Option<&str>) -> Result<Self, AdapterError> {
        let mut b = reqwest::Client::builder().timeout(Duration::from_secs(10)).user_agent("Mozilla/5.0 (X11; Linux x86_64) Lumen");
        if let Some(p) = proxy {
            b = b.proxy(reqwest::Proxy::all(p).map_err(|e| AdapterError::Auth(format!("bad proxy: {e}")))?);
        }
        Ok(Self { http: b.build().map_err(|e| AdapterError::Transient(e.to_string()))?, session: None })
    }
    /// Use the session stored by `accounts login` (credential `web_session`) instead of a guest one.
    pub fn with_session(mut self, session: &SecretString) -> Result<Self, AdapterError> {
        self.session = Some(serde_json::from_str(session.expose()).map_err(|_| AdapterError::Auth("invalid Likee `web_session`; run `accounts login` again".into()))?);
        Ok(self)
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response, AdapterError> {
        let resp = req.header("Origin", WEB_BASE).header("Referer", format!("{WEB_BASE}/")).send().await.map_err(|e| AdapterError::Transient(e.to_string()))?;
        match resp.status().as_u16() {
            429 => Err(AdapterError::RateLimited("http 429".into())),
            404 => Err(AdapterError::NotFound("http 404".into())),
            c if c >= 400 => Err(AdapterError::Transient(format!("http {c}"))),
            _ => Ok(resp),
        }
    }
    async fn post_json(&self, url: &str, body: Value) -> Result<Value, AdapterError> {
        self.send(self.http.post(url).json(&body)).await?.json().await.map_err(|e| AdapterError::Protocol(e.to_string()))
    }
    async fn guest(&self) -> Result<Guest, AdapterError> {
        if let Some(session) = &self.session {
            return Ok(session.clone());
        }
        let device = uuid::Uuid::new_v4().simple().to_string();
        let body = self.post_json(&format!("{API_BASE}/user/getGuestInfoForPC"), json!({"deviceId": device})).await?;
        let d = data(&body)?;
        Ok(Guest { uid: s(d, "uid"), device_id: s(d, "deviceId"), token: s(d, "cookie"), channel: "guest".into() })
    }
    async fn post_as(&self, g: &Guest, url: &str, body: Value) -> Result<Value, AdapterError> {
        let req = self.http.post(url).header("X-Auth-Token", &g.token).header("X-Uid", &g.uid).header("X-Client-DeviceId", &g.device_id).header("X-Channel", &g.channel).json(&body);
        self.send(req).await?.json().await.map_err(|e| AdapterError::Protocol(e.to_string()))
    }

    /// Keyword user search (`searchApi/searchUser`). Returns the raw `data` payload.
    pub async fn search_users(&self, keyword: &str, limit: u32, country: &str) -> Result<Value, AdapterError> {
        let g = self.guest().await?;
        let body = self.post_as(&g, &format!("{API_BASE}/searchApi/searchUser"), json!({"keyword": keyword, "start": 0, "limit": limit.clamp(1, 50), "countryCode": country})).await?;
        data(&body).cloned()
    }
    /// Resolve a LIVE share link (`likee.video/sid/…` or a short `likee.video/s/…` link).
    pub async fn live_room(&self, share_url: &str) -> Result<live::ShareRoom, AdapterError> {
        if !live::valid_share_url(share_url) {
            return Err(AdapterError::NotFound("`share_url` must be an https likee.video LIVE share link".into()));
        }
        let resp = self.send(self.http.get(share_url)).await?;
        if !live::valid_share_url(resp.url().as_str()) {
            return Err(AdapterError::Protocol("share link redirected away from likee.video".into()));
        }
        live::parse_share_page(&resp.text().await.map_err(|e| AdapterError::Transient(e.to_string()))?)
    }

    /// Creator lookup by Likee ID.
    pub async fn find_creator(&self, username: &str) -> Result<Profile, AdapterError> {
        parse_profile(&self.post_json(&format!("{API_BASE}/official_website/WebView/getProfileDetail"), json!({"likeeId": username.trim_start_matches('@')})).await?)
    }
    pub async fn creator_videos(&self, creator_id: &str, cursor: &str, limit: u32) -> Result<Vec<Video>, AdapterError> {
        let g = self.guest().await?;
        parse_videos(&self.post_as(&g, &format!("{API_BASE}/videoApi/getUserVideo"), json!({"uid": creator_id, "lastPostId": cursor, "count": limit.min(100), "tabType": 0})).await?)
    }
    pub async fn creator_post_count(&self, creator_id: &str) -> Result<Value, AdapterError> {
        let body = self.post_json(&format!("{API_BASE}/userApi/getUserPostNum"), json!({"uid": creator_id})).await?;
        Ok(data(&body)?.pointer(&format!("/postInfoMap/{creator_id}")).cloned().unwrap_or(Value::Null))
    }
    pub async fn trending_videos(&self, country: &str, language: &str, start: u32, limit: u32) -> Result<Vec<Video>, AdapterError> {
        let g = self.guest().await?;
        parse_videos(&self.post_as(&g, &format!("{API_BASE}/videoApi/getSquareVideos"), json!({"scene": "WELOG_POPULAR", "fetchNum": limit.min(100), "startNum": start, "lastPostId": 0, "deviceId": g.device_id, "uid": g.uid, "language": language, "country": country})).await?)
    }
    pub async fn trending_hashtags(&self, country: &str, language: &str, page: u32, per: u32) -> Result<Vec<Hashtag>, AdapterError> {
        parse_hashtags(&self.post_json(&format!("{API_BASE}/RecommendApi/getRecommendHashtag"), json!({"pagesize": per.min(100), "page": page, "language": language, "country": country})).await?)
    }
    pub async fn video_comments(&self, video_id: &str, language: &str, cursor: &str, limit: u32) -> Result<Vec<Comment>, AdapterError> {
        let req = self.http.get(format!("{WEB_BASE}/live/home/comments")).query(&[("post_id", video_id), ("lang", language), ("page_size", &limit.min(49).to_string()), ("last_comment_id", cursor)]);
        parse_comments(&self.send(req).await?.json().await.map_err(|e| AdapterError::Protocol(e.to_string()))?)
    }
}

#[derive(Debug, Default, Clone)]
pub struct LikeeAdapter;

#[async_trait]
impl PlatformAdapter for LikeeAdapter {
    fn platform(&self) -> Platform {
        Platform::LikeeGlobal
    }
    fn capabilities(&self) -> Capabilities {
        // Profile and videos verified against the real service on 2026-11-08 (docs/platforms.md).
        let e = Support::Experimental;
        Capabilities { profile: Support::Verified, search: e, videos: Support::Verified, trending: e, live_events: e, chat: e, gift: e, like: e, follow: e, share: e, viewer_count: e, broadcast_rtmp: e, ..Capabilities::NONE }
    }
    fn login_spec(&self, _t: &LiveTarget) -> Option<LoginSpec> {
        Some(LoginSpec { url: format!("{WEB_BASE}/"), credential: "web_session", capture: SessionCapture::Script(SESSION_SCRIPT.into()) })
    }
    async fn authenticate(&self, _c: &Credentials) -> Result<AuthState, AdapterError> {
        Ok(AuthState::Anonymous)
    }
    async fn profile(&self, handle: &str, _c: &Credentials) -> Result<Profile, AdapterError> {
        LikeeClient::new(None)?.find_creator(handle).await
    }
    /// Likee Streamer has URL / Stream Key fields, but no public API that issues a key is known.
    async fn provision_broadcast(&self, _t: &LiveTarget, _c: &Credentials) -> Result<lumen_live::BroadcastEndpoint, AdapterError> {
        Err(AdapterError::Unsupported(NO_STREAM_KEY_API.into()))
    }
    async fn check_broadcast(&self, _t: &LiveTarget, _c: &Credentials) -> Result<String, AdapterError> {
        Err(AdapterError::Unsupported(NO_STREAM_KEY_API.into()))
    }
    /// Setting `share_url`: the LIVE share link from the Likee app (a new link per LIVE).
    async fn open_live(&self, t: &LiveTarget, _c: &Credentials, cancel: CancellationToken) -> Result<Box<dyn LiveSession>, AdapterError> {
        let url = t.settings.get("share_url").ok_or_else(|| AdapterError::NotFound("set `share_url` to the LIVE share link from the Likee app".into()))?;
        let client = LikeeClient::new(t.settings.get("proxy").map(String::as_str))?;
        let room = client.live_room(url).await?;
        Ok(Box::new(live::LikeeSession::connect(t, room, cancel).await?))
    }
}
