//! Likee GLOBAL web API, reimplemented in Rust from lucasintel/likee (Ruby, BSD-3) and
//! lucasintel/likee.cr (Crystal). Endpoints are unofficial and may change.
//!
//! LIVE chat/gift events: **unsupported**. No public or documented Likee LIVE event protocol is
//! known, and none was validated (see docs/platforms.md). `open_live` returns `Unsupported`.
use async_trait::async_trait;
use lumen_live::adapter::AuthState;
use lumen_live::{AdapterError, Capabilities, Credentials, LiveSession, LiveTarget, Platform, PlatformAdapter, Profile, Support};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const API_BASE: &str = "https://api.like-video.com/likee-activity-flow-micro";
pub const WEB_BASE: &str = "https://likee.video";

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
        Some(code) => Err(AdapterError::Transient(format!("likee code={code} {}", s(body, "msg")))),
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
        posted_at: v.get("postTime").and_then(Value::as_i64),
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

/// Extract the `"userinfo":{...}` object embedded in a creator's profile page.
pub fn parse_profile_html(html: &str) -> Result<Profile, AdapterError> {
    let start = html.find("\"userinfo\":").ok_or_else(|| AdapterError::NotFound("profile data not found in page".into()))? + "\"userinfo\":".len();
    let u: Value = serde_json::Deserializer::from_str(&html[start..]).into_iter::<Value>().next().and_then(Result::ok).ok_or_else(|| AdapterError::Protocol("unparseable userinfo".into()))?;
    Ok(Profile {
        platform: Platform::LikeeGlobal,
        user_id: s(&u, "uid"),
        handle: s(&u, "likeeId"),
        display_name: s(&u, "nick_name"),
        avatar_url: opt(s(&u, "bigUrl")),
        followers: Some(n(&u, "fansCount")),
        following: Some(n(&u, "followCount")),
        likes: Some(n(&u, "allLikeCount")),
        bio: opt(s(&u, "bio")),
        country: opt(s(&u, "exactCountryCode")),
        live_room_id: None,
    })
}

#[derive(Debug, Clone)]
pub struct LikeeClient {
    http: reqwest::Client,
}

impl LikeeClient {
    pub fn new(proxy: Option<&str>) -> Result<Self, AdapterError> {
        let mut b = reqwest::Client::builder().timeout(Duration::from_secs(10)).user_agent("Mozilla/5.0 (X11; Linux x86_64) Lumen");
        if let Some(p) = proxy {
            b = b.proxy(reqwest::Proxy::all(p).map_err(|e| AdapterError::Auth(format!("bad proxy: {e}")))?);
        }
        Ok(Self { http: b.build().map_err(|e| AdapterError::Transient(e.to_string()))? })
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
    async fn post_form(&self, url: &str, form: &[(&str, String)]) -> Result<Value, AdapterError> {
        self.send(self.http.post(url).form(form)).await?.json().await.map_err(|e| AdapterError::Protocol(e.to_string()))
    }

    /// Creator lookup by Likee ID (the "search" the reference libraries offer).
    pub async fn find_creator(&self, username: &str) -> Result<Profile, AdapterError> {
        let html = self.send(self.http.get(format!("{WEB_BASE}/@{}", username.trim_start_matches('@')))).await?.text().await.map_err(|e| AdapterError::Transient(e.to_string()))?;
        parse_profile_html(&html)
    }
    pub async fn creator_videos(&self, creator_id: &str, cursor: &str, limit: u32) -> Result<Vec<Video>, AdapterError> {
        parse_videos(&self.post_json(&format!("{API_BASE}/videoApi/getUserVideo"), json!({"uid": creator_id, "lastPostId": cursor, "count": limit.min(100), "tabType": 0})).await?)
    }
    pub async fn creator_post_count(&self, creator_id: &str) -> Result<Value, AdapterError> {
        let body = self.post_json(&format!("{API_BASE}/userApi/getUserPostNum"), json!({"uid": creator_id})).await?;
        Ok(data(&body)?.pointer(&format!("/postInfoMap/{creator_id}")).cloned().unwrap_or(Value::Null))
    }
    pub async fn trending_videos(&self, country: &str, language: &str, start: u32, limit: u32) -> Result<Vec<Video>, AdapterError> {
        let device = uuid::Uuid::new_v4().simple().to_string();
        parse_videos(&self.post_json(&format!("{API_BASE}/videoApi/getSquareVideos"), json!({"scene": "WELOG_POPULAR", "fetchNum": limit.min(100), "startNum": start, "lastPostId": 0, "deviceId": device, "uid": 0, "language": language, "country": country})).await?)
    }
    pub async fn trending_hashtags(&self, country: &str, language: &str, page: u32, per: u32) -> Result<Vec<Hashtag>, AdapterError> {
        parse_hashtags(&self.post_form(&format!("{WEB_BASE}/official_website/RecommendApi/getRecommendHashtag"), &[("pagesize", per.min(100).to_string()), ("page", page.to_string()), ("language", language.into()), ("country", country.into())]).await?)
    }
    pub async fn hashtag_videos(&self, hashtag_id: &str, country: &str, page: u32, per: u32) -> Result<Vec<Video>, AdapterError> {
        parse_videos(&self.post_form(&format!("{WEB_BASE}/official_website/VideoApi/getEventVideo"), &[("topicId", hashtag_id.into()), ("pageSize", per.min(100).to_string()), ("page", page.to_string()), ("country", country.into())]).await?)
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
        // Fixture-tested only: likee.video refused TLS from the validation network (docs/platforms.md).
        Capabilities { profile: Support::Experimental, search: Support::Experimental, videos: Support::Experimental, trending: Support::Experimental, broadcast_rtmp: Support::Experimental, ..Capabilities::NONE }
    }
    async fn authenticate(&self, _c: &Credentials) -> Result<AuthState, AdapterError> {
        Ok(AuthState::Anonymous)
    }
    async fn profile(&self, handle: &str, _c: &Credentials) -> Result<Profile, AdapterError> {
        LikeeClient::new(None)?.find_creator(handle).await
    }
    async fn open_live(&self, _t: &LiveTarget, _c: &Credentials, _cancel: CancellationToken) -> Result<Box<dyn LiveSession>, AdapterError> {
        Err(AdapterError::Unsupported("Likee LIVE chat/gift events have no validated protocol; see docs/platforms.md".into()))
    }
}
