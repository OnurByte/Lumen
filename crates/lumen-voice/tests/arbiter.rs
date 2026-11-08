use lumen_voice::{Arbiter, AudioError, PcmFrame};
fn frame(generation: u64, sequence: u64, last: bool, samples: Vec<f32>) -> PcmFrame {
    PcmFrame {
        generation,
        utterance: 7,
        sequence,
        sample_rate: 24000,
        channels: 1,
        last,
        samples,
    }
}
#[test]
fn partial_playback_then_cancel_never_completes_utterance() {
    let mut audio = Arbiter::new(24000, 1).unwrap();
    audio.begin(1, 7).unwrap();
    audio.push(frame(1, 0, true, vec![0.5; 4])).unwrap();
    let mut output = [0.; 2];
    assert!(audio.render(&mut output).unwrap().is_empty());
    assert_eq!(output, [0.5; 2]);
    audio.cancel();
    assert!(audio.render(&mut output).unwrap().is_empty());
    assert_eq!(output, [0.; 2]);
    assert_eq!(
        audio.push(frame(1, 1, true, vec![0.5])),
        Err(AudioError::Stale)
    );
}
#[test]
fn completion_only_after_final_frame_and_floor_exclusive() {
    let mut audio = Arbiter::new(24000, 1).unwrap();
    audio.begin(1, 7).unwrap();
    assert_eq!(audio.begin(2, 8), Err(AudioError::Occupied));
    audio.push(frame(1, 0, false, vec![0.25; 2])).unwrap();
    audio.push(frame(1, 1, true, vec![0.5; 2])).unwrap();
    assert!(audio.render(&mut [0.; 2]).unwrap().is_empty());
    assert_eq!(audio.render(&mut [0.; 2]).unwrap(), vec![(1, 7)]);
    assert!(audio.begin(2, 8).is_ok());
}
#[test]
fn validates_format_sequence_samples_and_bounds() {
    let mut audio = Arbiter::new(24000, 1).unwrap();
    audio.begin(1, 7).unwrap();
    assert_eq!(
        audio.push(frame(1, 1, false, vec![0.])),
        Err(AudioError::Sequence)
    );
    assert_eq!(
        audio.push(frame(1, 0, false, vec![f32::NAN])),
        Err(AudioError::Invalid)
    );
    assert_eq!(
        audio.push(frame(1, 0, false, vec![2.])),
        Err(AudioError::Invalid)
    );
    for n in 0..32 {
        audio.push(frame(1, n, false, vec![0.; 100])).unwrap();
    }
    assert_eq!(
        audio.push(frame(1, 32, false, vec![0.])),
        Err(AudioError::Full)
    );
}
#[test]
fn rejects_audio_after_final_chunk() {
    let mut audio = Arbiter::new(24000, 1).unwrap();
    audio.begin(1, 7).unwrap();
    audio.push(frame(1, 0, true, vec![0.])).unwrap();
    assert_eq!(
        audio.push(frame(1, 1, true, vec![0.])),
        Err(AudioError::Sequence)
    );
}

#[test]
fn sample_budget_and_format_mismatch_rejected() {
    let mut audio = Arbiter::new(24000, 1).unwrap();
    audio.begin(1, 7).unwrap();
    let mut wrong = frame(1, 0, false, vec![0.]);
    wrong.sample_rate = 48000;
    assert_eq!(audio.push(wrong), Err(AudioError::Invalid));
    audio.push(frame(1, 0, false, vec![0.; 24000])).unwrap();
    audio.push(frame(1, 1, false, vec![0.; 24000])).unwrap();
    assert_eq!(
        audio.push(frame(1, 2, false, vec![0.])),
        Err(AudioError::Full)
    );
    assert_eq!(audio.queued_samples(), 48000);
    audio.cancel();
    assert_eq!(audio.queued_samples(), 0);
}
