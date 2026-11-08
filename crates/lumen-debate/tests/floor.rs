use lumen_debate::{Floor, FloorError};

#[test]
fn cancellation_rejects_unplayed_and_late_audio() {
    let mut floor = Floor::default();
    let turn = floor.grant("Ada").unwrap();
    floor.queue(turn, 0, "Merhaba").unwrap();
    floor.queue(turn, 1, "Bu duyulmadı").unwrap();
    floor.played(turn, 0).unwrap();
    floor.cancel();
    assert_eq!(floor.played(turn, 1), Err(FloorError::StaleTurn));
    assert_eq!(floor.transcript(), &["Merhaba"]);
    let next = floor.grant("Deniz").unwrap();
    assert_ne!(turn, next);
    assert_eq!(floor.played(turn, 0), Err(FloorError::StaleTurn));
}

#[test]
fn one_floor_and_ordered_unique_acknowledgments() {
    let mut floor = Floor::default();
    let turn = floor.grant("Ada").unwrap();
    assert_eq!(floor.grant("Deniz"), Err(FloorError::Occupied));
    floor.queue(turn, 0, "Bir").unwrap();
    floor.queue(turn, 1, "İki").unwrap();
    assert_eq!(floor.played(turn, 1), Err(FloorError::OutOfOrder));
    floor.played(turn, 0).unwrap();
    assert_eq!(floor.played(turn, 0), Err(FloorError::OutOfOrder));
    floor.played(turn, 1).unwrap();
    assert_eq!(floor.transcript(), &["Bir", "İki"]);
}

#[test]
fn pause_and_emergency_stop_invalidate_pending_turns() {
    let mut floor = Floor::default();
    let turn = floor.grant("Ada").unwrap();
    floor.pause();
    assert_eq!(floor.queue(turn, 0, "late"), Err(FloorError::StaleTurn));
    assert_eq!(floor.grant("Ada"), Err(FloorError::Paused));
    floor.resume();
    assert!(floor.grant("Deniz").is_ok());
    floor.stop();
    floor.resume();
    assert_eq!(floor.grant("Ada"), Err(FloorError::Stopped));
}

#[test]
fn queues_are_bounded_and_validate_text() {
    let mut floor = Floor::default();
    assert_eq!(floor.grant(""), Err(FloorError::InvalidInput));
    let turn = floor.grant("Ada").unwrap();
    assert_eq!(floor.queue(turn, 0, ""), Err(FloorError::InvalidInput));
    assert_eq!(
        floor.queue(turn, 0, &"x".repeat(4097)),
        Err(FloorError::InvalidInput)
    );
    for n in 0..32 {
        floor.queue(turn, n, "ses").unwrap();
    }
    assert_eq!(floor.queue(turn, 32, "fazla"), Err(FloorError::QueueFull));
}

#[test]
fn new_runtime_rejects_old_runtime_playback() {
    let old = Floor::default().grant("Ada").unwrap();
    let mut restarted = Floor::default();
    let new = restarted.grant("Ada").unwrap();
    restarted.queue(new, 0, "New generation").unwrap();
    assert_ne!(old, new);
    assert_eq!(restarted.played(old, 0), Err(FloorError::StaleTurn));
    assert!(restarted.transcript().is_empty());
}
