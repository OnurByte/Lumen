//! Kwai GLOBAL REST calls (room discovery). Ported from weteta/kwai-live (2024); the host,
//! app version and salt are configurable because they may be obsolete.
use crate::sign::{SALT, sign};
use lumen_live::AdapterError;
use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_API_HOST: &str = "az2-api-akpro.kwai-pro.com";
pub const APP_VERSION: &str = "10.3.30.535003";
const CLIENT_KEY: &str = "3c2cd3f3";

#[derive(Debug, Clone)]
pub struct KwaiApi {
    http: reqwest::Client,
    pub host: String,
    /// Per-account device id (`ANDROID_` + 16 hex); never shared between accounts.
    pub device_id: String,
    pub salt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRoom {
    pub user_id: String,
    pub live_stream_id: String,
    pub exp_tag: String,
}

impl KwaiApi {
    pub fn new(host: &str, device_id: &str, proxy: Option<&str>) -> Result<Self, AdapterError> {
        let mut b = reqwest::Client::builder().timeout(Duration::from_secs(15)).user_agent("kwai-android aegon/3.12.1");
        if let Some(p) = proxy {
            b = b.proxy(reqwest::Proxy::all(p).map_err(|e| AdapterError::Auth(format!("bad proxy: {e}")))?);
        }
        Ok(Self { http: b.build().map_err(|e| AdapterError::Transient(e.to_string()))?, host: host.into(), device_id: device_id.into(), salt: SALT.into() })
    }

    fn query(&self) -> Vec<(String, String)> {
        let ts = lumen_live::now_ms().to_string();
        [
            ("mod", "unknown(unknown)"), ("lon", "0"), ("lat", "0"), ("countryInfo", "USA"), ("country_code", "us"), ("bucket", "us"),
            ("kpn", "KWAI"), ("kpf", "ANDROID_PHONE"), ("appver", APP_VERSION), ("ver", "10.3"), ("app", "1"), ("net", "WIFI"),
            ("c", "GOOGLE_PLAY"), ("language", "en-us"), ("pkg", "com.kwai.video"), ("sys", "KWAI_ANDROID"), ("time_zone", "UTC"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .chain([("timestamp".into(), ts), ("did".into(), self.device_id.clone())])
        .collect()
    }

    async fn post(&self, path: &str, body: Vec<(String, String)>) -> Result<Value, AdapterError> {
        let query = self.query();
        let mut body = body;
        body.push(("client_key".into(), CLIENT_KEY.into()));
        body.push(("os".into(), "android".into()));
        let all: Vec<_> = query.iter().chain(body.iter()).cloned().collect();
        body.push(("sig".into(), sign(&all, &self.salt)));
        let url = format!("https://{}{path}", self.host);
        let resp = self.http.post(url).query(&query).form(&body).send().await.map_err(|e| AdapterError::Transient(e.to_string()))?;
        let status = resp.status();
        if status.as_u16() == 429 {
            return Err(AdapterError::RateLimited("http 429".into()));
        }
        let json: Value = resp.json().await.map_err(|e| AdapterError::Protocol(format!("http {status}: {e}")))?;
        check_result(&json)?;
        Ok(json)
    }

    pub async fn search_user(&self, handle: &str) -> Result<Value, AdapterError> {
        self.post("/rest/o/user/search", vec![("user_name".into(), handle.into()), ("page".into(), "1".into()), ("source".into(), "USER_INPUT".into())]).await
    }

    /// Resolve `handle` to its LIVE room. `Offline` if the user is not live.
    pub async fn find_live(&self, handle: &str) -> Result<LiveRoom, AdapterError> {
        let json = self.search_user(handle).await?;
        parse_live_room(&json, handle)
    }

    /// Obtain the per-viewer token (and socket hosts, when offered) for the room socket.
    pub async fn start_play(&self, room: &LiveRoom) -> Result<PlayTicket, AdapterError> {
        parse_ticket(&self.start_play_raw(room).await?)
    }

    pub async fn start_play_raw(&self, room: &LiveRoom) -> Result<Value, AdapterError> {
        self
            .post("/rest/o/live/v2/startPlay", vec![("author".into(), room.user_id.clone()), ("exp_tag".into(), room.exp_tag.clone()), ("liveStreamId".into(), room.live_stream_id.clone()), ("isRetry".into(), "false".into())])
            .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayTicket {
    pub token: String,
    /// `host:port` socket endpoints offered by the server (may be empty).
    pub hosts: Vec<String>,
    pub watching: Option<u64>,
}

pub fn parse_ticket(json: &Value) -> Result<PlayTicket, AdapterError> {
    let token = json.get("token").and_then(Value::as_str).filter(|t| !t.is_empty()).ok_or_else(|| AdapterError::Protocol("startPlay returned no token".into()))?;
    let hosts = json
        .pointer("/race/rounds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("horses").and_then(Value::as_array))
        .flatten()
        .filter_map(|h| h.get("hostAndPort").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    let watching = json.get("watchingCount").and_then(Value::as_u64);
    Ok(PlayTicket { token: token.into(), hosts, watching })
}

/// Map Kwai's `result` codes. 1 = success; others are errors (40 observed as "server busy").
pub fn check_result(json: &Value) -> Result<(), AdapterError> {
    match json.get("result").and_then(Value::as_i64) {
        Some(1) => Ok(()),
        Some(code) => {
            let msg = json.get("error_msg").and_then(Value::as_str).unwrap_or("").to_owned();
            Err(match code {
                40 | 109 => AdapterError::RateLimited(format!("kwai result={code} {msg}")),
                _ => AdapterError::Transient(format!("kwai result={code} {msg}")),
            })
        }
        None => Err(AdapterError::Protocol("response without result code".into())),
    }
}

pub fn parse_live_room(json: &Value, handle: &str) -> Result<LiveRoom, AdapterError> {
    let users = json.get("users").and_then(Value::as_array).ok_or_else(|| AdapterError::Protocol("search response without users".into()))?;
    let wanted = handle.trim_start_matches('@').to_lowercase();
    let user = users
        .iter()
        .find(|u| ["kwai_id", "kwaiId", "user_name"].iter().any(|k| u.get(*k).and_then(Value::as_str).is_some_and(|v| v.to_lowercase() == wanted)))
        .or_else(|| users.first())
        .ok_or_else(|| AdapterError::NotFound(format!("Kwai user {handle}")))?;
    let live = user.get("liveInfo").filter(|l| !l.is_null()).ok_or_else(|| AdapterError::Offline(format!("{handle} is not LIVE")))?;
    let s = |v: Option<&Value>| v.and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_i64().map(|n| n.to_string())));
    Ok(LiveRoom {
        live_stream_id: s(live.get("liveStreamId")).filter(|v| !v.is_empty()).ok_or_else(|| AdapterError::Offline(format!("{handle} has no liveStreamId")))?,
        exp_tag: s(live.get("exp_tag")).unwrap_or_default(),
        user_id: s(live.get("user").and_then(|u| u.get("user_id"))).or_else(|| s(user.get("user_id"))).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_search_fixture() {
        let json: Value = serde_json::from_str(include_str!("../tests/fixtures/user_search_live.json")).unwrap();
        assert_eq!(parse_live_room(&json, "@Creator_One").unwrap(), LiveRoom { user_id: "150000001".into(), live_stream_id: "Abc123LiveId".into(), exp_tag: "1_a/2000_x".into() });
        let offline = serde_json::json!({"result":1,"users":[{"user_name":"x","liveInfo":null}]});
        assert!(matches!(parse_live_room(&offline, "x"), Err(AdapterError::Offline(_))));
        let t = parse_ticket(&serde_json::json!({"result":1,"token":"t","watchingCount":6,"race":{"rounds":[{"horses":[{"hostAndPort":"99.83.187.25:8888"}]}]}})).unwrap();
        assert_eq!((t.hosts, t.watching), (vec!["99.83.187.25:8888".to_string()], Some(6)));
        assert!(matches!(check_result(&serde_json::json!({"result":40,"error_msg":"busy"})), Err(AdapterError::RateLimited(_))));
    }
}
