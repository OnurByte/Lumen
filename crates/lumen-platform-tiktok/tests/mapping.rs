use lumen_live::{AccountId, EventKind, LiveTarget, Platform};
use lumen_platform_tiktok::map_event;
use piratetok_live_rs::structs::TikTokLiveEvent;
use piratetok_live_rs::structs::proto::gift_types::GiftDetails;
use piratetok_live_rs::structs::proto::messages::{WebcastChatMessage, WebcastGiftMessage, WebcastRoomUserSeqMessage};
use piratetok_live_rs::structs::proto::types::CommonMessageData;
use piratetok_live_rs::structs::proto::user::UserIdentity;

fn target() -> LiveTarget {
    LiveTarget { account_id: AccountId("acct-1234".into()), platform: Platform::TikTok, handle: "h".into(), settings: Default::default() }
}
fn common(id: i64) -> Option<CommonMessageData> {
    Some(CommonMessageData { msg_id: id, create_time: 1_700_000_000_000, ..Default::default() })
}
fn who() -> Option<UserIdentity> {
    Some(UserIdentity { user_id: 42, nickname: "viewer".into(), ..Default::default() })
}

#[test]
fn chat_gift_and_viewers_normalize() {
    let chat = map_event(TikTokLiveEvent::Chat(WebcastChatMessage { common: common(7), user: who(), comment: " hi ".into(), ..Default::default() }), &target(), "room", 1).unwrap();
    assert_eq!(chat.event_id, "7");
    assert_eq!(chat.kind, EventKind::Chat { text: "hi".into() });
    assert_eq!(chat.user.unwrap().id, "42");
    assert_eq!(chat.platform_ts_ms, Some(1_700_000_000_000));

    let streak = |end| TikTokLiveEvent::Gift(WebcastGiftMessage { common: common(8), user: who(), gift_id: 5655, repeat_count: 3, repeat_end: end, gift_details: Some(GiftDetails { combo: true, gift_name: "Rose".into(), diamond_count: 1, ..Default::default() }), ..Default::default() });
    assert!(map_event(streak(0), &target(), "room", 1).is_none(), "mid-streak gifts are suppressed");
    let gift = map_event(streak(1), &target(), "room", 1).unwrap();
    assert_eq!(gift.kind, EventKind::Gift { gift_id: "5655".into(), gift_name: Some("Rose".into()), count: 3, unit_value: Some(1) });

    let seq = map_event(TikTokLiveEvent::RoomUserSeq(WebcastRoomUserSeqMessage { common: common(9), viewer_count: 120, total_user: 900, ..Default::default() }), &target(), "room", 1).unwrap();
    assert_eq!(seq.kind, EventKind::ViewerCount { current: 120, total: Some(900) });
    assert!(map_event(TikTokLiveEvent::Chat(WebcastChatMessage { common: common(10), comment: "   ".into(), ..Default::default() }), &target(), "room", 1).is_none());
}
