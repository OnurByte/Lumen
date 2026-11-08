use lumen_debate::{
    Session,
    pipeline::{Playback, TurnContext, run_turn},
    provider::{ModelProvider, ModelRequest},
};
use lumen_storage::Journal;
use lumen_voice::{PcmFrame, worker::WorkerVoice};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
struct FixtureModel(&'static str);
impl ModelProvider for FixtureModel {
    async fn stream(
        &self,
        _: ModelRequest,
        output: Sender<String>,
        _: CancellationToken,
    ) -> Result<(), String> {
        output.send(self.0.into()).await.map_err(|e| e.to_string())
    }
}
#[derive(Default)]
struct FixturePlayback {
    pending: VecDeque<(u64, u64)>,
    cancelled: bool,
    withhold: bool,
}
impl Playback for FixturePlayback {
    fn begin(&mut self, _: u64, _: u64) -> Result<(), String> {
        Ok(())
    }
    fn has_capacity(&self, _: usize) -> bool {
        true
    }
    fn push(&mut self, frame: PcmFrame) -> Result<(), String> {
        if frame.last {
            self.pending.push_back((frame.generation, frame.utterance));
        }
        Ok(())
    }
    fn event(&mut self) -> Option<Result<(u64, u64), String>> {
        if self.withhold {
            None
        } else {
            self.pending.pop_front().map(Ok)
        }
    }
    fn cancel(&mut self) {
        self.pending.clear();
        self.cancelled = true;
    }
}
fn request() -> ModelRequest {
    ModelRequest {
        persona: "Ada".into(),
        position: "A".into(),
        skill: None,
        phase: None,
        topic: "Tarih".into(),
        audible_context: vec![],
        audience: None,
    }
}
fn context() -> TurnContext {
    TurnContext {
        generation: 1,
        speaker: "Ada".into(),
        voice: "female_2".into(),
        deadline: Duration::from_secs(5),
        clock: Instant::now(),
    }
}
fn voice() -> WorkerVoice {
    WorkerVoice::new(
        "python3",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../lumen-voice/tests/fixtures/voice.py"
        ),
    )
}
#[tokio::test]
async fn model_to_voice_to_playback_to_journal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.sqlite");
    let mut session = Session::new("s", Journal::open(&path).await.unwrap()).unwrap();
    let mut sink = FixturePlayback::default();
    run_turn(
        &FixtureModel("Merhaba. İkinci görüş."),
        &mut voice(),
        &mut sink,
        &mut session,
        request(),
        context(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        Journal::open(&path)
            .await
            .unwrap()
            .audible("s")
            .await
            .unwrap(),
        vec!["Merhaba.", "İkinci görüş."]
    );
}
#[tokio::test]
async fn cancel_during_tts_never_journals_generated_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.sqlite");
    let mut session = Session::new("s", Journal::open(&path).await.unwrap()).unwrap();
    let mut sink = FixturePlayback::default();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        trigger.cancel();
    });
    assert!(
        run_turn(
            &FixtureModel("hang"),
            &mut voice(),
            &mut sink,
            &mut session,
            request(),
            context(),
            cancel
        )
        .await
        .is_err()
    );
    assert!(sink.cancelled);
    assert!(
        Journal::open(&path)
            .await
            .unwrap()
            .audible("s")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn generated_and_consumed_audio_without_playback_ack_is_not_dialogue() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("show.sqlite");
    let mut session = Session::new("s", Journal::open(&path).await.unwrap()).unwrap();
    let mut sink = FixturePlayback {
        withhold: true,
        ..Default::default()
    };
    let mut run = context();
    run.deadline = Duration::from_millis(100);
    assert!(
        run_turn(
            &FixtureModel("Merhaba."),
            &mut voice(),
            &mut sink,
            &mut session,
            request(),
            run,
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert!(sink.cancelled);
    assert!(
        Journal::open(&path)
            .await
            .unwrap()
            .audible("s")
            .await
            .unwrap()
            .is_empty()
    );
}
