use lumen_voice::worker::{VoiceProvider, VoiceRequest, WorkerVoice};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn request() -> VoiceRequest {
    VoiceRequest {
        generation: 1,
        utterance: 7,
        text: "Merhaba".into(),
        voice: "female_2".into(),
    }
}
#[tokio::test]
async fn parses_binary_pcm_from_isolated_worker() {
    let mut voice = WorkerVoice::new(
        "python3",
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/voice.py"),
    );
    let (tx, mut rx) = mpsc::channel(4);
    voice
        .synthesize_stream(request(), tx, CancellationToken::new())
        .await
        .unwrap();
    let frame = rx.recv().await.unwrap();
    assert_eq!(
        (
            frame.generation,
            frame.utterance,
            frame.sequence,
            frame.last
        ),
        (1, 7, 0, true)
    );
    assert_eq!(frame.samples, vec![0.25, -0.5]);
    assert!(rx.recv().await.is_none());
    voice.cancel().await.unwrap();
}
#[tokio::test]
async fn cancellation_stops_hanging_worker_and_allows_restart() {
    let mut voice = WorkerVoice::new(
        "python3",
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/voice.py"),
    );
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let mut hang = request();
    hang.text = "hang".into();
    let (tx, _) = mpsc::channel(1);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        trigger.cancel();
    });
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            voice.synthesize_stream(hang, tx, cancel)
        )
        .await
        .unwrap()
        .is_err()
    );
    let (tx, mut rx) = mpsc::channel(4);
    voice
        .synthesize_stream(request(), tx, CancellationToken::new())
        .await
        .unwrap();
    assert!(rx.recv().await.unwrap().last);
    voice.cancel().await.unwrap();
}
#[tokio::test]
async fn corrupt_or_stale_worker_frames_rejected() {
    for text in ["oversize", "stale", "nan", "badseq"] {
        let mut voice = WorkerVoice::new(
            "python3",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/voice.py"),
        );
        let mut bad = request();
        bad.text = text.into();
        let (tx, _) = mpsc::channel(4);
        assert!(
            voice
                .synthesize_stream(bad, tx, CancellationToken::new())
                .await
                .is_err(),
            "{text}"
        );
    }
}
