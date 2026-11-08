use lumen_voice::output::PlaybackFence;
#[test]
fn consumed_audio_waits_for_device_clock_and_cancel_discards_it() {
    let mut fence = PlaybackFence::default();
    fence.consumed(1, 7, 100);
    assert_eq!(fence.confirmed(99), None);
    assert_eq!(fence.confirmed(100), Some((1, 7)));
    assert_eq!(fence.confirmed(101), None);
    fence.consumed(2, 8, 200);
    fence.cancel();
    assert_eq!(fence.confirmed(300), None);
}
