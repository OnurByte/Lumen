//! Kwai GLOBAL LIVE socket: TCP + 16-byte framing + protobuf, heartbeat every 10 s.
use crate::api::{KwaiApi, LiveRoom, PlayTicket};
use crate::frame;
use crate::proto::{self, FeedPush, SocketMessage, payload_type as pt};
use async_trait::async_trait;
use bytes::BytesMut;
use lumen_live::event::stable_hash;
use lumen_live::{AdapterError, EventKind, LiveEvent, LiveSession, LiveTarget, LiveUser, Platform, now_ms};
use prost::Message;
use std::io::Read;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_SOCKET: &str = "99.83.187.25:8888";
const HEARTBEAT: Duration = Duration::from_secs(10);

fn envelope(payload_type: i64, payload: impl Message) -> bytes::Bytes {
    frame::encode(&SocketMessage { payload_type, compression: 1, payload: payload.encode_to_vec() }.encode_to_vec())
}

pub fn heartbeat(live_stream_id: &str) -> bytes::Bytes {
    envelope(pt::CS_HEARTBEAT, proto::Heartbeat { timestamp_ms: now_ms(), live_stream_id: live_stream_id.into() })
}

pub fn hello(api: &KwaiApi, room: &LiveRoom, token: &str) -> [bytes::Bytes; 2] {
    let start = proto::Start { live_stream_id: room.live_stream_id.clone(), room: 2, device_id: api.device_id.clone(), language: "en_US".into(), app_version: crate::api::APP_VERSION.into(), latitude: 0.0, longitude: 0.0, network: "WIFI".into() };
    // Constants below are copied from observed traffic; their meaning is unknown.
    let enter = proto::EnterRoom {
        n1: 2, device_id: api.device_id.clone(), live_stream_id: room.live_stream_id.clone(), n2: 4_294_967_295, locale: "en_US".into(),
        app_version: crate::api::APP_VERSION.into(), network: "WIFI".into(), n3: 1, exp_tag: room.exp_tag.clone(), attach: "unlogin".into(),
        n4: 11, token: token.into(), kpn: "KWAI".into(), kpf: "ANDROID_PHONE".into(), n5: 100_000, gift: r#"{"giftId":0}"#.into(),
    };
    [envelope(pt::CS_START, start), envelope(pt::CS_ENTER_ROOM, enter)]
}

fn user(u: &Option<proto::UserInfo>) -> Option<LiveUser> {
    u.as_ref().filter(|u| u.user_id != 0).map(|u| LiveUser { id: u.user_id.to_string(), display_name: u.name.clone() })
}

/// Decode one frame payload into normalized events (fixture-tested).
pub fn decode_frame(payload: &[u8], target: &LiveTarget, room_id: &str) -> Result<Vec<LiveEvent>, AdapterError> {
    let msg = SocketMessage::decode(payload).map_err(|e| AdapterError::Protocol(format!("socket message: {e}")))?;
    if msg.payload_type != pt::SC_FEED_PUSH {
        return Ok(Vec::new());
    }
    let body = match msg.compression {
        2 => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(&msg.payload[..]).take(4 << 20).read_to_end(&mut out).map_err(|e| AdapterError::Protocol(format!("gzip: {e}")))?;
            out
        }
        _ => msg.payload,
    };
    let feed = FeedPush::decode(&body[..]).map_err(|e| AdapterError::Protocol(format!("feed push: {e}")))?;
    let now = now_ms();
    let mut out = Vec::new();
    let mut push = |id: &str, ts: i64, u: Option<LiveUser>, kind: EventKind| {
        let id = if id.is_empty() { stable_hash(&[kind.name(), u.as_ref().map_or("", |u| &u.id), &ts.to_string()]) } else { id.to_owned() };
        out.push(LiveEvent { platform: Platform::KwaiGlobal, account_id: target.account_id.clone(), room_id: room_id.into(), event_id: id, platform_ts_ms: (ts > 0).then_some(ts), received_ts_ms: now, user: u, kind });
    };
    for c in &feed.comments {
        if !c.content.trim().is_empty() {
            push(&c.id, c.time_ms, user(&c.user), EventKind::Chat { text: c.content.trim().into() });
        }
    }
    for l in &feed.likes {
        push(&l.id, l.time_ms, user(&l.user), EventKind::Like { count: 1, total: None });
    }
    for g in &feed.gifts {
        push(&g.id, g.time_ms, user(&g.user), EventKind::Gift { gift_id: g.gift_id.to_string(), gift_name: None, count: g.repeat_count.max(1) as u32, unit_value: None });
    }
    for s in &feed.shares {
        push(&s.id, s.time_ms, user(&s.user), EventKind::Share);
    }
    for j in &feed.joins {
        push(&j.id, j.time_ms, user(&j.user), EventKind::Join);
    }
    Ok(out)
}

pub struct KwaiSession {
    stream: TcpStream,
    buf: BytesMut,
    target: LiveTarget,
    room: LiveRoom,
    heartbeat: tokio::time::Interval,
    cancel: CancellationToken,
    initial_viewers: Option<u64>,
}

impl KwaiSession {
    pub async fn connect(api: &KwaiApi, target: &LiveTarget, room: LiveRoom, ticket: PlayTicket, socket_override: Option<&str>, cancel: CancellationToken) -> Result<Self, AdapterError> {
        let hosts: Vec<String> = match socket_override {
            Some(h) => vec![h.to_owned()],
            None if ticket.hosts.is_empty() => vec![DEFAULT_SOCKET.to_owned()],
            None => ticket.hosts.clone(),
        };
        let mut last = AdapterError::Transient("no socket hosts".into());
        for host in &hosts {
            match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(host)).await {
                Ok(Ok(mut stream)) => {
                    stream.set_nodelay(true).ok();
                    for f in hello(api, &room, &ticket.token) {
                        stream.write_all(&f).await.map_err(|e| AdapterError::Transient(e.to_string()))?;
                    }
                    let mut heartbeat = tokio::time::interval(HEARTBEAT);
                    heartbeat.tick().await;
                    return Ok(Self { stream, buf: BytesMut::with_capacity(16 * 1024), target: target.clone(), room, heartbeat, cancel, initial_viewers: ticket.watching });
                }
                Ok(Err(e)) => last = AdapterError::Transient(format!("{host}: {e}")),
                Err(_) => last = AdapterError::Transient(format!("{host}: connect timeout")),
            }
        }
        Err(last)
    }
}

#[async_trait]
impl LiveSession for KwaiSession {
    fn room_id(&self) -> &str {
        &self.room.live_stream_id
    }
    async fn next_events(&mut self) -> Result<Option<Vec<LiveEvent>>, AdapterError> {
        if let Some(current) = self.initial_viewers.take() {
            let id = format!("watching-{}", now_ms());
            return Ok(Some(vec![LiveEvent { platform: Platform::KwaiGlobal, account_id: self.target.account_id.clone(), room_id: self.room.live_stream_id.clone(), event_id: id, platform_ts_ms: None, received_ts_ms: now_ms(), user: None, kind: EventKind::ViewerCount { current, total: None } }]));
        }
        loop {
            match frame::decode(&mut self.buf) {
                Ok(Some(payload)) => {
                    let events = decode_frame(&payload, &self.target, &self.room.live_stream_id)?;
                    if !events.is_empty() {
                        return Ok(Some(events));
                    }
                    continue;
                }
                Ok(None) => {}
                Err(e) => return Err(AdapterError::Protocol(format!("framing: {e:?}"))),
            }
            tokio::select! {
                _ = self.cancel.cancelled() => return Err(AdapterError::Cancelled),
                _ = self.heartbeat.tick() => {
                    self.stream.write_all(&heartbeat(&self.room.live_stream_id)).await.map_err(|e| AdapterError::Transient(e.to_string()))?;
                }
                n = self.stream.read_buf(&mut self.buf) => {
                    if n.map_err(|e| AdapterError::Transient(e.to_string()))? == 0 {
                        return Ok(None);
                    }
                }
            }
        }
    }
}
