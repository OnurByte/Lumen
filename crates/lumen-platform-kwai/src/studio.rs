//! Kwai Studio (studio.kwai.com) broadcast provisioning, reimplemented from the request shapes in
//! the Studio web app's own JavaScript (not a documented public API). The account's own logged-in
//! Studio session cookie authorizes every call; Lumen never logs in on the user's behalf.
//!
//! Flow: checkPermission (hasPermission, account kpn) → livingRoom (reuse an open room) →
//! transcodeResource?isPush=true (push node) → createRoom (multipart, streamType=2) →
//! room?liveRoomId (pushUrl[0] = "<server>/<key>") → push video → startLive → stopLive.
use lumen_live::{AdapterError, BroadcastEndpoint, SecretString};
use serde_json::{Value, json};
use std::time::Duration;

pub const DEFAULT_HOST: &str = "studio.kwai.com";
const NO_AUTHORITY: i64 = 1_397_013_000;
const ROOM_EXISTED: i64 = 1_397_013_001;
const NO_STREAM: i64 = 1_397_013_003;
const INVALID_ROOM: i64 = 1_397_013_004;
const ACCOUNT_FORBIDDEN: i64 = 1_397_013_005;
const ROOM_LIVING: i64 = 1;
const NO_LIVE_PERMISSION: &str = "this account has no Kwai LIVE permission; it needs 500+ followers and approval (Kwai app: Camera → LIVE → Apply), see https://www.kwai.com/support/live-permissions/i-want-to-request-permission-to-live-on-my-profile";
/// streamType 2 = the operator pushes RTMP to Kwai (1 = Kwai pulls from a URL).
const STREAM_TYPE_PUSH: &str = "2";
/// Studio's own fallback when an account reports no kpn (3 = KWAI_BULLDOG, 4 = KWAI, 5 = KWAI.ME).
const DEFAULT_KPN: &str = "3";

pub struct StudioClient {
    http: reqwest::Client,
    base: String,
}

/// The logged-in account as `checkPermission` reports it. Exists only if LIVE permission is granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioAccount {
    pub ks_id: String,
    pub kpn: String,
    pub country: Option<String>,
}

/// Depth-first search for the first value under `key` (response envelopes vary between endpoints).
pub fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|c| find(c, key))),
        Value::Array(a) => a.iter().find_map(|c| find(c, key)),
        _ => None,
    }
}

pub fn check(json: &Value) -> Result<(), AdapterError> {
    match json.get("result").and_then(Value::as_i64) {
        Some(1) => Ok(()),
        Some(code) => {
            let msg = json.get("error_msg").and_then(Value::as_str).filter(|m| !m.is_empty()).unwrap_or("no details");
            Err(match code {
                ACCOUNT_FORBIDDEN | NO_AUTHORITY => AdapterError::Auth(format!("Kwai Studio: {NO_LIVE_PERMISSION} ({msg})")),
                INVALID_ROOM => AdapterError::NotFound(format!("Kwai Studio: this room no longer exists ({msg})")),
                NO_STREAM => AdapterError::Transient(format!("Kwai Studio: no video reaching the push URL yet ({msg})")),
                109 | 401 => AdapterError::Auth(format!("Kwai Studio session is missing or expired; log in at https://studio.kwai.com and store a fresh `studio_cookie` ({msg})")),
                _ => AdapterError::Transient(format!("Kwai Studio result={code} {msg}")),
            })
        }
        None => Err(AdapterError::Protocol("Kwai Studio response without result code".into())),
    }
}

/// `rtmp://host/app/KEY` → (`rtmp://host/app`, `KEY`), as the Studio UI splits `pushUrl[0]`.
pub fn split_push_url(push: &str) -> Result<(String, String), AdapterError> {
    match push.rsplit_once('/') {
        Some((server, key)) if !key.is_empty() && server.contains("://") => Ok((server.to_owned(), key.to_owned())),
        _ => Err(AdapterError::Protocol("unexpected Kwai push URL shape".into())),
    }
}

pub fn endpoint_from_room(json: &Value, room_id: &str) -> Result<BroadcastEndpoint, AdapterError> {
    let push = find(json, "pushUrl")
        .and_then(|p| p.as_array().and_then(|a| a.first()).or(Some(p)))
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| AdapterError::Protocol("Kwai Studio room has no pushUrl".into()))?;
    let (ingest_url, key) = split_push_url(push)?;
    Ok(BroadcastEndpoint { ingest_url, stream_key: SecretString::new(key), room_id: Some(room_id.to_owned()) })
}

fn id_string(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned).or_else(|| v.as_i64().map(|n| n.to_string())).filter(|s| !s.is_empty())
}

/// Plain http is accepted only for a loopback test server.
pub fn origin(host: &str) -> String {
    if host.starts_with("http://127.0.0.1:") { host.to_owned() } else { format!("https://{host}") }
}

impl StudioClient {
    pub fn new(host: &str, cookie: &SecretString, proxy: Option<&str>) -> Result<Self, AdapterError> {
        // DevTools "Copy as cURL"/raw headers include the header name; the value is what we need.
        let raw = cookie.expose().trim();
        let value = raw.get(..7).filter(|p| p.eq_ignore_ascii_case("cookie:")).map_or(raw, |_| raw[7..].trim());
        if !value.contains('=') || value.chars().any(|c| c.is_control()) {
            return Err(AdapterError::Auth("Kwai Studio needs a `studio_cookie` credential from a logged-in studio.kwai.com session".into()));
        }
        let mut headers = reqwest::header::HeaderMap::new();
        let mut cookie_header = reqwest::header::HeaderValue::from_str(value).map_err(|_| AdapterError::Auth("invalid studio_cookie".into()))?;
        cookie_header.set_sensitive(true);
        headers.insert(reqwest::header::COOKIE, cookie_header);
        let origin = origin(host);
        headers.insert(reqwest::header::ORIGIN, origin.parse().map_err(|_| AdapterError::Auth("invalid host".into()))?);
        headers.insert(reqwest::header::REFERER, format!("{origin}/").parse().map_err(|_| AdapterError::Auth("invalid host".into()))?);
        let mut b = reqwest::Client::builder().timeout(Duration::from_secs(20)).default_headers(headers).user_agent("Mozilla/5.0 (X11; Linux x86_64) Lumen");
        if let Some(p) = proxy {
            b = b.proxy(reqwest::Proxy::all(p).map_err(|e| AdapterError::Auth(format!("bad proxy: {e}")))?);
        }
        Ok(Self { http: b.build().map_err(|e| AdapterError::Transient(e.to_string()))?, base: origin })
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value, AdapterError> {
        let resp = req.send().await.map_err(|e| AdapterError::Transient(e.to_string()))?;
        match resp.status().as_u16() {
            401 | 403 => return Err(AdapterError::Auth("Kwai Studio rejected the session cookie".into())),
            429 => return Err(AdapterError::RateLimited("http 429".into())),
            _ => {}
        }
        let json: Value = resp.json().await.map_err(|e| AdapterError::Protocol(format!("Kwai Studio: {e}")))?;
        check(&json)?;
        Ok(json)
    }
    fn url(&self, path: &str) -> String {
        format!("{}/rest/o/w/live/supplier/cloudLive/{path}", self.base)
    }

    /// Session and LIVE-permission gate; Studio itself treats a falsy `hasPermission` as "no access".
    pub async fn account(&self) -> Result<StudioAccount, AdapterError> {
        let json = self.send(self.http.get(self.url("checkPermission"))).await?;
        if json.get("hasPermission").and_then(Value::as_bool) != Some(true) {
            return Err(AdapterError::Auth(format!("Kwai Studio: {NO_LIVE_PERMISSION}")));
        }
        let field = |k: &str| json.pointer(&format!("/userInfo/{k}")).and_then(id_string);
        Ok(StudioAccount {
            ks_id: field("ksId").ok_or_else(|| AdapterError::Protocol("Kwai Studio account without ksId".into()))?,
            kpn: field("kpn").unwrap_or_else(|| DEFAULT_KPN.to_owned()),
            country: field("country"),
        })
    }

    /// True once the session is logged in, whether or not the account has LIVE permission.
    pub async fn logged_in(&self) -> bool {
        let Ok(resp) = self.http.get(self.url("checkPermission")).send().await else { return false };
        let Ok(json) = resp.json::<Value>().await else { return false };
        json.get("result").and_then(Value::as_i64).is_some_and(|code| !matches!(code, 109 | 401))
    }

    /// Everything `create_room` depends on, without creating anything.
    pub async fn preflight(&self) -> Result<String, AdapterError> {
        let me = self.account().await?;
        self.push_node().await?;
        let room = self.living_room(&me.ks_id).await?;
        Ok(format!(
            "Kwai Studio session valid, LIVE permission granted, push node available (kpn {}, country {}); {}",
            me.kpn,
            me.country.as_deref().unwrap_or("?"),
            room.map_or("a new room will be created".to_owned(), |r| format!("open room {r} will be reused")),
        ))
    }

    async fn push_node(&self) -> Result<String, AdapterError> {
        let json = self.send(self.http.get(self.url("transcodeResource")).query(&[("isPush", "true")])).await?;
        json.get("transcodeResources")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|n| n.get("key"))
            .and_then(id_string)
            .ok_or_else(|| AdapterError::Protocol("Kwai Studio offered no push node".into()))
    }

    async fn living_room(&self, ks_id: &str) -> Result<Option<String>, AdapterError> {
        let json = self.send(self.http.get(self.url("livingRoom")).query(&[("authorId", ks_id)])).await?;
        Ok(find(&json, "liveRoomId").and_then(id_string))
    }

    async fn new_room(&self, me: &StudioAccount, caption: &str, cover: Vec<u8>, cover_type: &str) -> Result<String, AdapterError> {
        let node = self.push_node().await?;
        let cover = reqwest::multipart::Part::bytes(cover).file_name("cover").mime_str(cover_type).map_err(|e| AdapterError::Protocol(e.to_string()))?;
        let mut form = reqwest::multipart::Form::new()
            .text("streamType", STREAM_TYPE_PUSH)
            .text("kpn", me.kpn.clone())
            .text("autoStartTime", lumen_live::now_ms().to_string())
            .text("notifyFans", "false")
            .part("cover", cover)
            .text("coverContentType", cover_type.to_owned())
            .text("pushNodes", node);
        if !caption.trim().is_empty() {
            form = form.text("caption", caption.trim().to_owned());
        }
        match self.send(self.http.post(self.url("createRoom")).multipart(form)).await {
            Ok(json) => find(&json, "liveRoomId").and_then(id_string).ok_or_else(|| AdapterError::Protocol("createRoom returned no liveRoomId".into())),
            Err(AdapterError::Transient(m)) if m.contains(&ROOM_EXISTED.to_string()) => {
                self.living_room(&me.ks_id).await?.ok_or_else(|| AdapterError::Protocol("existing Kwai room id not found".into()))
            }
            Err(e) => Err(e),
        }
    }

    /// Create a push room (or reuse the account's open one) and return its server + key.
    pub async fn create_room(&self, caption: &str, cover: Vec<u8>, cover_type: &str) -> Result<BroadcastEndpoint, AdapterError> {
        let me = self.account().await?;
        let room_id = match self.living_room(&me.ks_id).await? {
            Some(open) => open,
            None => self.new_room(&me, caption, cover, cover_type).await?,
        };
        let json = self.send(self.http.get(self.url("room")).query(&[("liveRoomId", room_id.as_str())])).await?;
        endpoint_from_room(&json, &room_id)
    }

    pub async fn start_live(&self, room_id: &str) -> Result<(), AdapterError> {
        // A reused room may already be LIVE or over; startLive would fail on it forever.
        let room = self.send(self.http.get(self.url("room")).query(&[("liveRoomId", room_id)])).await?;
        match find(&room, "roomStatus").and_then(Value::as_i64) {
            Some(ROOM_LIVING) => return Ok(()),
            Some(status) if status > ROOM_LIVING => return Err(AdapterError::NotFound(format!("Kwai Studio room {room_id} has already ended"))),
            _ => {}
        }
        let kpn: i64 = self.account().await?.kpn.parse().unwrap_or(3);
        self.send(self.http.post(self.url("startLive")).json(&json!({"liveRoomId": room_id, "noStreamCheckTime": 2, "kpn": kpn}))).await.map(drop)
    }

    pub async fn stop_live(&self, room_id: &str) -> Result<(), AdapterError> {
        self.send(self.http.post(self.url("stopLive")).json(&json!({"liveRoomId": room_id}))).await.map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn room_fixture_yields_server_and_key() {
        let room: Value = serde_json::from_str(include_str!("../tests/fixtures/studio_room.json")).unwrap();
        let ep = endpoint_from_room(&room, "R1").unwrap();
        assert_eq!(ep.ingest_url, "rtmp://push.example.kwai.net/livecloud");
        assert_eq!(ep.stream_key.expose(), "kszt_ABCdef123?auth=xyz");
        assert!(!format!("{ep:?}").contains("kszt_ABC"), "debug output redacts the key");
        assert!(split_push_url("no-slash").is_err());
        assert!(matches!(check(&json!({"result": 1397013005, "error_msg": "x"})), Err(AdapterError::Auth(_))));
        assert!(matches!(check(&json!({"result": 1397013000})), Err(AdapterError::Auth(_))));
        assert!(matches!(check(&json!({"result": 1397013004})), Err(AdapterError::NotFound(_))));
        assert!(matches!(check(&json!({"result": 1397013003})), Err(AdapterError::Transient(_))));
        assert_eq!(find(&json!({"a": [{"b": {"liveRoomId": 7}}]}), "liveRoomId").and_then(id_string).as_deref(), Some("7"));
    }
}
