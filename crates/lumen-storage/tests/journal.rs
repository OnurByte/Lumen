use lumen_storage::{Event, Journal};
#[tokio::test]
async fn durable_ordered_journal_isolates_sessions_and_only_projects_playback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.sqlite");
    let journal = Journal::open(&path).await.unwrap();
    journal
        .append("session-a", 10, "queued", "unsaid")
        .await
        .unwrap();
    journal
        .append("session-a", 11, "played", "Merhaba")
        .await
        .unwrap();
    journal
        .append("session-b", 1, "played", "Other")
        .await
        .unwrap();
    drop(journal);
    let journal = Journal::open(&path).await.unwrap();
    let events = journal.events("session-a", 0, 100).await.unwrap();
    assert_eq!(events.len(), 2);
    assert!(events[0].sequence < events[1].sequence);
    assert_eq!(events[1].monotonic_ms, 11);
    assert_eq!(journal.audible("session-a").await.unwrap(), vec!["Merhaba"]);
    assert_eq!(
        journal
            .events("session-a", events[0].sequence, 100)
            .await
            .unwrap(),
        vec![events[1].clone()]
    );
}
#[tokio::test]
async fn validates_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(&dir.path().join("show.sqlite"))
        .await
        .unwrap();
    assert!(journal.append("", 0, "played", "hello").await.is_err());
    assert!(journal.append("s", 0, "invented", "hello").await.is_err());
    assert!(
        journal
            .append("s", 0, "played", &"x".repeat(65537))
            .await
            .is_err()
    );
    assert!(
        journal
            .append("s", u64::MAX, "played", "hello")
            .await
            .is_err()
    );
    assert_eq!(
        journal.events("s", 0, 100).await.unwrap(),
        Vec::<Event>::new()
    );
}

#[tokio::test]
async fn recent_context_is_bounded_and_chronological() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(&dir.path().join("show.sqlite"))
        .await
        .unwrap();
    for index in 0..40 {
        journal
            .append("s", index, "played", &index.to_string())
            .await
            .unwrap();
    }
    assert_eq!(
        journal.recent_audible("s", 2).await.unwrap(),
        vec!["38", "39"]
    );
    assert!(journal.recent_audible("s", 0).await.is_err());
    assert!(journal.recent_audible("s", 33).await.is_err());
}
