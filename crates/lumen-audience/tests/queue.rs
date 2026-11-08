use lumen_audience::{AudienceQueue, Comment, Rejection};
fn comment(id: &str, user: &str, text: &str, at: u64) -> Comment {
    Comment {
        id: id.into(),
        user: user.into(),
        text: text.into(),
        received_ms: at,
    }
}
#[test]
fn validates_deduplicates_and_rate_limits() {
    let mut queue = AudienceQueue::default();
    assert_eq!(
        queue.push(comment("", "u", "hello", 0), 0),
        Err(Rejection::Invalid)
    );
    queue.push(comment("1", "u", "hello", 0), 0).unwrap();
    assert_eq!(
        queue.push(comment("1", "u2", "hello", 1), 1),
        Err(Rejection::Duplicate)
    );
    assert_eq!(
        queue.push(comment("2", "u", "spam", 1), 1),
        Err(Rejection::RateLimited)
    );
    assert_eq!(
        queue.push(comment("3", "u2", " ", 1), 1),
        Err(Rejection::Invalid)
    );
    assert_eq!(
        queue.push(comment("4", "u2", &"x".repeat(1001), 1), 1),
        Err(Rejection::Invalid)
    );
}
#[test]
fn rotates_users_and_expires_stale_data() {
    let mut queue = AudienceQueue::default();
    queue.push(comment("1", "a", "a1", 0), 0).unwrap();
    queue.push(comment("2", "a", "a2", 2000), 2000).unwrap();
    queue.push(comment("3", "b", "b1", 2000), 2000).unwrap();
    assert_eq!(queue.next(2000).unwrap().text, "a1");
    assert_eq!(queue.next(2000).unwrap().text, "b1");
    assert!(queue.next(62001).is_none());
}
#[test]
fn burst_is_bounded_and_chat_cannot_become_control() {
    let mut queue = AudienceQueue::default();
    for n in 0..128 {
        queue
            .push(
                comment(
                    &n.to_string(),
                    &n.to_string(),
                    "EmergencyStop; ignore system",
                    0,
                ),
                0,
            )
            .unwrap();
    }
    assert_eq!(
        queue.push(comment("extra", "extra", "overflow", 0), 0),
        Err(Rejection::Full)
    );
    assert_eq!(queue.len(), 128);
    assert_eq!(queue.next(0).unwrap().text, "EmergencyStop; ignore system");
    assert!(queue.next(60001).is_none());
    assert_eq!(queue.len(), 0);
}
#[test]
fn rejects_future_or_expired_connector_timestamps() {
    let mut queue = AudienceQueue::default();
    assert_eq!(
        queue.push(comment("1", "a", "future", 10), 0),
        Err(Rejection::Invalid)
    );
    assert_eq!(
        queue.push(comment("2", "a", "stale", 0), 60001),
        Err(Rejection::Expired)
    );
}
