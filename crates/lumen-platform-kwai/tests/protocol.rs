use lumen_live::{AccountId, EventKind, LiveTarget, Platform};
use lumen_platform_kwai::proto::{self, FeedPush, SocketMessage};
use lumen_platform_kwai::{frame, session::decode_frame};
use prost::Message;

fn target() -> LiveTarget {
    LiveTarget { account_id: AccountId("acct".into()), platform: Platform::KwaiGlobal, handle: "h".into(), settings: Default::default() }
}
fn u(id: i64) -> Option<proto::UserInfo> {
    Some(proto::UserInfo { user_id: id, name: format!("user{id}") })
}

#[test]
fn feed_push_round_trips_through_framing_and_gzip() {
    let feed = FeedPush {
        comments: vec![proto::Comment { id: "c1".into(), user: u(1), time_ms: 1_000, content: " merhaba ".into() }],
        likes: vec![proto::Like { id: "l1".into(), user: u(2), time_ms: 2_000 }],
        gifts: vec![proto::Gift { id: "g1".into(), user: u(3), time_ms: 3_000, gift_id: 77, repeat_count: 4 }],
        shares: vec![proto::Share { id: "s1".into(), user: u(4), time_ms: 4_000 }],
        joins: vec![proto::Comment { id: "j1".into(), user: u(5), time_ms: 5_000, content: String::new() }],
    };
    for compression in [1, 2] {
        let body = feed.encode_to_vec();
        let payload = if compression == 2 {
            use std::io::Write;
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            gz.write_all(&body).unwrap();
            gz.finish().unwrap()
        } else {
            body
        };
        let wire = frame::encode(&SocketMessage { payload_type: 310, compression, payload }.encode_to_vec());
        let mut buf = bytes::BytesMut::from(&wire[..]);
        let one = frame::decode(&mut buf).unwrap().unwrap();
        let events = decode_frame(&one, &target(), "room").unwrap();
        let kinds: Vec<_> = events.iter().map(|e| e.kind.name()).collect();
        assert_eq!(kinds, ["chat", "like", "gift", "share", "join"]);
        assert_eq!(events[0].kind, EventKind::Chat { text: "merhaba".into() });
        assert_eq!(events[2].kind, EventKind::Gift { gift_id: "77".into(), gift_name: None, count: 4, unit_value: None });
        assert_eq!(events[0].platform_ts_ms, Some(1_000));
        assert_eq!(events[4].user.as_ref().unwrap().id, "5");
    }
    // Non-feed payloads (enter-room ack, heartbeat ack) carry no events; garbage is a protocol error.
    let ack = SocketMessage { payload_type: 300, compression: 1, payload: vec![] }.encode_to_vec();
    assert!(decode_frame(&ack, &target(), "room").unwrap().is_empty());
    assert!(decode_frame(&[0xff, 0xff, 0xff], &target(), "room").is_err());
}

#[test]
fn per_account_device_ids_differ_and_are_stable() {
    let a = lumen_platform_kwai::adapter::device_id(&target());
    let mut t2 = target();
    t2.account_id = AccountId("other".into());
    assert_ne!(a, lumen_platform_kwai::adapter::device_id(&t2));
    assert_eq!(a, lumen_platform_kwai::adapter::device_id(&target()));
    assert!(a.starts_with("ANDROID_") && a.len() == 24);
}
