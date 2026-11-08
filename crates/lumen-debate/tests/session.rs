use lumen_debate::Session;
use lumen_storage::Journal;

#[tokio::test]
async fn interrupted_session_reopens_with_only_played_dialogue() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.sqlite");
    let journal = Journal::open(&path).await.unwrap();
    let mut session = Session::new("episode-1", journal).unwrap();
    let turn = session.grant("Ada").unwrap();
    session.queue(turn, 0, "Merhaba").unwrap();
    session.queue(turn, 1, "Duyulmadı").unwrap();
    session.played(turn, 0, 100).await.unwrap();
    session.cancel(101).await.unwrap();
    assert!(session.played(turn, 1, 102).await.is_err());
    drop(session);
    let journal = Journal::open(&path).await.unwrap();
    assert_eq!(journal.audible("episode-1").await.unwrap(), vec!["Merhaba"]);
    assert_eq!(journal.events("episode-1", 0, 100).await.unwrap().len(), 2);
}

#[tokio::test]
async fn duplicate_or_out_of_order_ack_does_not_touch_journal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.sqlite");
    let mut session = Session::new("episode-1", Journal::open(&path).await.unwrap()).unwrap();
    let turn = session.grant("Ada").unwrap();
    session.queue(turn, 0, "Bir").unwrap();
    assert!(session.played(turn, 1, 0).await.is_err());
    session.played(turn, 0, 1).await.unwrap();
    assert!(session.played(turn, 0, 2).await.is_err());
    let journal = Journal::open(&path).await.unwrap();
    assert_eq!(journal.audible("episode-1").await.unwrap(), vec!["Bir"]);
}

#[tokio::test]
async fn failed_persistence_pauses_and_rejects_late_ack() {
    use lumen_debate::FloorError;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.sqlite");
    let mut session = Session::new("episode-1", Journal::open(&path).await.unwrap()).unwrap();
    let turn = session.grant("Ada").unwrap();
    session.queue(turn, 0, "Bir").unwrap();
    assert!(session.played(turn, 0, u64::MAX).await.is_err());
    assert_eq!(session.grant("Deniz"), Err(FloorError::Paused));
    assert!(session.played(turn, 0, 1).await.is_err());
    assert!(
        Journal::open(&path)
            .await
            .unwrap()
            .audible("episode-1")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn cannot_journal_finish_while_unplayed_segments_remain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.sqlite");
    let mut session = Session::new("s", Journal::open(&path).await.unwrap()).unwrap();
    let turn = session.grant("Ada").unwrap();
    session.queue(turn, 0, "unplayed").unwrap();
    assert!(session.finish(turn, 1).await.is_err());
    assert!(
        Journal::open(&path)
            .await
            .unwrap()
            .events("s", 0, 100)
            .await
            .unwrap()
            .is_empty()
    );
}
