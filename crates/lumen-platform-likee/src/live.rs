//! Likee LIVE events from the public web viewer, reimplemented from the "page-live-share" web
//! client (a saved copy is in rightbot/LiKeE): the share page (`likee.video/sid/<room>_…`, the
//! link the app's "Share LIVE" button produces) server-renders `wsUrl`, `roomId` and `live_num`.
//! The client opens `wsUrl`, sends the text `websocket`, then receives JSON arrays of
//! `{c, grade, data: {n: name, m: text|gift id, c: count}}`.
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use lumen_live::event::stable_hash;
use lumen_live::{AdapterError, EventKind, LiveEvent, LiveSession, LiveTarget, LiveUser, Platform, now_ms};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareRoom {
    pub room_id: String,
    /// Empty when the LIVE has ended.
    pub ws_url: String,
    pub viewers: Option<u64>,
}

fn page_var<'a>(html: &'a str, name: &str) -> Option<&'a str> {
    let i = html.find(&format!("{name} = '"))? + name.len() + 4;
    let end = html[i..].find('\'')?;
    Some(&html[i..i + end])
}

pub fn parse_share_page(html: &str) -> Result<ShareRoom, AdapterError> {
    let room_id = page_var(html, "roomId").filter(|r| !r.is_empty()).ok_or_else(|| AdapterError::Protocol("Likee share page without roomId".into()))?;
    Ok(ShareRoom { room_id: room_id.into(), ws_url: page_var(html, "wsUrl").unwrap_or("").into(), viewers: page_var(html, "live_num").and_then(|n| n.parse().ok()) })
}

/// The page swaps the scheme to match its own (https → wss).
pub fn normalize_ws(url: &str) -> String {
    format!("wss://{}", url.trim_start_matches("wss://").trim_start_matches("ws://"))
}

/// Accept only Likee's own share links.
pub fn valid_share_url(url: &str) -> bool {
    let rest = url.strip_prefix("https://").unwrap_or("");
    let host = rest.split(['/', '?']).next().unwrap_or("");
    matches!(host, "likee.video" | "l.likee.video" | "www.likee.video" | "likee.com") && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Decode one WebSocket text frame. `seq` disambiguates identical messages within a session.
pub fn decode(text: &str, target: &LiveTarget, room_id: &str, seq: &mut u64) -> Vec<LiveEvent> {
    let Ok(json) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let items = match json {
        Value::Array(a) => a,
        v @ Value::Object(_) => vec![v],
        _ => return Vec::new(),
    };
    let now = now_ms();
    items
        .iter()
        .filter_map(|item| {
            let code = item.get("c").and_then(Value::as_i64)?;
            let data = item.get("data").unwrap_or(&Value::Null);
            let name = data.get("n").and_then(Value::as_str).unwrap_or("").to_owned();
            let text = |k: &str| data.get(k).map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            let kind = match code {
                0 => EventKind::LiveEnded,
                1 | 12 => {
                    let m = text("m");
                    if m.trim().is_empty() { return None; }
                    EventKind::Chat { text: m.trim().to_owned() }
                }
                4 => EventKind::Share,
                8 => EventKind::Gift { gift_id: text("m"), gift_name: None, count: data.get("c").and_then(|c| c.as_u64().or_else(|| c.as_str()?.parse().ok())).unwrap_or(1).max(1) as u32, unit_value: None },
                9 => EventKind::Like { count: 1, total: None },
                10 => EventKind::Follow,
                _ => return None,
            };
            *seq += 1;
            // The web feed carries display names only; ids are derived from them.
            let user = (!name.is_empty()).then(|| LiveUser { id: stable_hash(&["likee-user", &name]), display_name: name.clone() });
            Some(LiveEvent {
                platform: Platform::LikeeGlobal,
                account_id: target.account_id.clone(),
                room_id: room_id.to_owned(),
                event_id: stable_hash(&[&seq.to_string(), &code.to_string(), &name, &text("m")]),
                platform_ts_ms: None,
                received_ts_ms: now,
                user,
                kind,
            })
        })
        .collect()
}

pub struct LikeeSession {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    target: LiveTarget,
    room: ShareRoom,
    seq: u64,
    cancel: CancellationToken,
    announce_viewers: bool,
}

impl LikeeSession {
    pub async fn connect(target: &LiveTarget, room: ShareRoom, cancel: CancellationToken) -> Result<Self, AdapterError> {
        if room.ws_url.is_empty() {
            return Err(AdapterError::Offline("Likee LIVE has ended (share page has no wsUrl)".into()));
        }
        let (mut ws, _) = tokio_tungstenite::connect_async(normalize_ws(&room.ws_url)).await.map_err(|e| AdapterError::Transient(format!("likee ws: {e}")))?;
        ws.send(Message::text("websocket")).await.map_err(|e| AdapterError::Transient(e.to_string()))?;
        Ok(Self { ws, target: target.clone(), room, seq: 0, cancel, announce_viewers: true })
    }
}

#[async_trait]
impl LiveSession for LikeeSession {
    fn room_id(&self) -> &str {
        &self.room.room_id
    }
    async fn next_events(&mut self) -> Result<Option<Vec<LiveEvent>>, AdapterError> {
        if std::mem::take(&mut self.announce_viewers)
            && let Some(current) = self.room.viewers
        {
            let e = LiveEvent { platform: Platform::LikeeGlobal, account_id: self.target.account_id.clone(), room_id: self.room.room_id.clone(), event_id: format!("viewers-{}", now_ms()), platform_ts_ms: None, received_ts_ms: now_ms(), user: None, kind: EventKind::ViewerCount { current, total: None } };
            return Ok(Some(vec![e]));
        }
        loop {
            let msg = tokio::select! {
                _ = self.cancel.cancelled() => return Err(AdapterError::Cancelled),
                m = self.ws.next() => m,
            };
            match msg {
                None | Some(Ok(Message::Close(_))) => return Ok(None),
                Some(Err(e)) => return Err(AdapterError::Transient(e.to_string())),
                Some(Ok(Message::Text(t))) => {
                    let events = decode(&t, &self.target, &self.room.room_id, &mut self.seq);
                    if !events.is_empty() {
                        return Ok(Some(events));
                    }
                }
                Some(Ok(_)) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_live::AccountId;

    fn target() -> LiveTarget {
        LiveTarget { account_id: AccountId("acct".into()), platform: Platform::LikeeGlobal, handle: "h".into(), settings: Default::default() }
    }

    #[test]
    fn share_page_and_frames_decode() {
        let html = include_str!("../tests/fixtures/live_share.html");
        let room = parse_share_page(html).unwrap();
        assert_eq!(room, ShareRoom { room_id: "966050152".into(), ws_url: "ws://1.2.3.4:443/&a&b&0".into(), viewers: Some(57) });
        assert_eq!(normalize_ws(&room.ws_url), "wss://1.2.3.4:443/&a&b&0");
        let ended = parse_share_page(&html.replace("ws://1.2.3.4:443/&a&b&0", "")).unwrap();
        assert!(ended.ws_url.is_empty());

        let mut seq = 0;
        let frame = include_str!("../tests/fixtures/live_frame.json");
        let events = decode(frame, &target(), "966050152", &mut seq);
        let kinds: Vec<_> = events.iter().map(|e| e.kind.name()).collect();
        assert_eq!(kinds, ["chat", "share", "gift", "like", "follow", "chat", "live_ended"]);
        assert_eq!(events[0].kind, EventKind::Chat { text: "merhaba".into() });
        assert_eq!(events[2].kind, EventKind::Gift { gift_id: "1001".into(), gift_name: None, count: 3, unit_value: None });
        assert_eq!(events[0].user.as_ref().unwrap().display_name, "Ayse");
        assert_ne!(events[0].event_id, events[5].event_id);
        assert!(decode("not json", &target(), "r", &mut seq).is_empty());

        assert!(valid_share_url("https://likee.video/sid/966050152_1_2?b=1"));
        assert!(!valid_share_url("https://evil.example/likee.video"));
        assert!(!valid_share_url("http://likee.video/sid/1"));
    }
}
