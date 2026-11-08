//! Kwai GLOBAL LIVE protobuf messages, hand-written with prost (no protoc needed) from the
//! field numbers in weteta/kwai-live `proto/kwai.proto`. Field names are ours; numbers are the wire.
//! Only fields Lumen uses are declared — prost skips unknown fields safely.

/// Envelope: `payload_type` (status in the reference) selects the payload message.
#[derive(Clone, PartialEq, prost::Message)]
pub struct SocketMessage {
    #[prost(int64, tag = "1")]
    pub payload_type: i64,
    /// 1 = none; 2 = gzip (assumed from the Kuaishou-family layout, not observed globally).
    #[prost(int64, tag = "2")]
    pub compression: i64,
    #[prost(bytes = "vec", tag = "3")]
    pub payload: Vec<u8>,
}

pub mod payload_type {
    pub const CS_HEARTBEAT: i64 = 1;
    pub const CS_ENTER_ROOM: i64 = 200;
    pub const CS_START: i64 = 204;
    pub const SC_ENTER_ROOM_ACK: i64 = 300;
    pub const SC_HEARTBEAT_ACK: i64 = 307;
    pub const SC_FEED_PUSH: i64 = 310;
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Start {
    #[prost(string, tag = "1")]
    pub live_stream_id: String,
    #[prost(int64, tag = "10")]
    pub room: i64,
    #[prost(string, tag = "11")]
    pub device_id: String,
    #[prost(string, tag = "12")]
    pub language: String,
    #[prost(string, tag = "13")]
    pub app_version: String,
    #[prost(double, tag = "14")]
    pub latitude: f64,
    #[prost(double, tag = "15")]
    pub longitude: f64,
    #[prost(string, tag = "16")]
    pub network: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct EnterRoom {
    #[prost(int64, tag = "2")]
    pub n1: i64,
    #[prost(string, tag = "3")]
    pub device_id: String,
    #[prost(string, tag = "4")]
    pub live_stream_id: String,
    #[prost(int64, tag = "7")]
    pub n2: i64,
    #[prost(string, tag = "8")]
    pub locale: String,
    #[prost(string, tag = "9")]
    pub app_version: String,
    #[prost(string, tag = "11")]
    pub network: String,
    #[prost(int64, tag = "12")]
    pub n3: i64,
    #[prost(string, tag = "13")]
    pub exp_tag: String,
    #[prost(string, tag = "14")]
    pub attach: String,
    #[prost(int64, tag = "15")]
    pub n4: i64,
    #[prost(string, tag = "16")]
    pub token: String,
    #[prost(string, tag = "17")]
    pub kpn: String,
    #[prost(string, tag = "18")]
    pub kpf: String,
    #[prost(int64, tag = "19")]
    pub n5: i64,
    #[prost(string, tag = "21")]
    pub gift: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Heartbeat {
    #[prost(int64, tag = "1")]
    pub timestamp_ms: i64,
    #[prost(string, tag = "2")]
    pub live_stream_id: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct UserInfo {
    #[prost(int64, tag = "1")]
    pub user_id: i64,
    #[prost(string, tag = "2")]
    pub name: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Comment {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(message, optional, tag = "2")]
    pub user: Option<UserInfo>,
    #[prost(int64, tag = "3")]
    pub time_ms: i64,
    #[prost(string, tag = "4")]
    pub content: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Like {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(message, optional, tag = "2")]
    pub user: Option<UserInfo>,
    #[prost(int64, tag = "3")]
    pub time_ms: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Gift {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(message, optional, tag = "2")]
    pub user: Option<UserInfo>,
    #[prost(int64, tag = "3")]
    pub time_ms: i64,
    #[prost(int64, tag = "4")]
    pub gift_id: i64,
    #[prost(int64, tag = "7")]
    pub repeat_count: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Share {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(message, optional, tag = "2")]
    pub user: Option<UserInfo>,
    #[prost(int64, tag = "3")]
    pub time_ms: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct FeedPush {
    #[prost(message, repeated, tag = "1")]
    pub comments: Vec<Comment>,
    #[prost(message, repeated, tag = "2")]
    pub likes: Vec<Like>,
    #[prost(message, repeated, tag = "4")]
    pub gifts: Vec<Gift>,
    #[prost(message, repeated, tag = "9")]
    pub shares: Vec<Share>,
    #[prost(message, repeated, tag = "28")]
    pub joins: Vec<Comment>,
}
