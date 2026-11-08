use crate::{control::Hub, show::run_loop};
use lumen_debate::{pack::ShowPack, pipeline::Playback};
use lumen_voice::{PcmFrame, worker::WorkerVoice};
use std::{sync::Arc, time::Duration};
use tokio::net::TcpListener;
struct NoPlayback;
impl Playback for NoPlayback {
    fn begin(&mut self, _: u64, _: u64) -> Result<(), String> {
        panic!("model did not respond")
    }
    fn has_capacity(&self, _: usize) -> bool {
        true
    }
    fn push(&mut self, _: PcmFrame) -> Result<(), String> {
        panic!("unexpected audio")
    }
    fn event(&mut self) -> Option<Result<(u64, u64), String>> {
        None
    }
    fn cancel(&mut self) {}
}
#[tokio::test]
async fn daemon_show_cancels_pending_model_on_operator_pause() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let source = include_str!("../../../packs/personas/history-clash.toml")
        .replace("http://127.0.0.1:11434/v1/chat/completions", &endpoint);
    let pack = ShowPack::parse(&source).unwrap();
    let hub = Arc::new(Hub::new(true));
    hub.status.lock().await.paused = false;
    let pause = hub.clone();
    let operator = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
        pause.status.lock().await.paused = true;
        pause.cancel_turn();
        pause.shutdown.cancel();
        pause.changed.notify_one();
    });
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut voice = WorkerVoice::new(
        "python3",
        root.join("../../crates/lumen-voice/tests/fixtures/voice.py"),
    );
    let dir = tempfile::tempdir().unwrap();
    let models = crate::show::build_models(&pack).unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        run_loop(
            pack,
            [None, None],
            models,
            dir.path().join("show.sqlite"),
            hub.clone(),
            &mut voice,
            &mut NoPlayback,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    operator.await.unwrap();
    assert!(hub.status.lock().await.paused);
    let journal = lumen_storage::Journal::open(&dir.path().join("show.sqlite"))
        .await
        .unwrap();
    assert!(journal.audible(&hub.session).await.unwrap().is_empty());
}

#[test]
fn cancelling_playback_clears_stage_speaker_caption_and_level() {
    use crate::show::StagePlayback;
    let stage = Arc::new(std::sync::Mutex::new(lumen_protocol::StageState::default()));
    let mut output = StagePlayback::new(NoPlayback, stage.clone());
    output.caption("Ada", "Merhaba.");
    stage.lock().unwrap().level = 75;
    assert_eq!(stage.lock().unwrap().speaker.as_deref(), Some("Ada"));
    output.cancel();
    let state = stage.lock().unwrap();
    assert!(state.speaker.is_none() && state.caption.is_empty());
    assert_eq!(state.level, 0);
}

#[tokio::test]
async fn rehearsal_speaks_skill_lines_rotates_topics_and_journals_only_played_text() {
    use crate::rehearse::{ScriptedModel, SilentVoice, TimedPlayback};
    let mut pack =
        ShowPack::parse(include_str!("../../../packs/personas/history-clash.toml")).unwrap();
    pack.topics = vec!["İkinci konu".into()];
    let hub = Arc::new(Hub::new(true));
    hub.status.lock().await.paused = false;
    hub.skip_topic
        .store(true, std::sync::atomic::Ordering::Release);
    let models = vec![
        ScriptedModel::new(vec!["Ada birinci.".into()]),
        ScriptedModel::new(vec!["Deniz birinci.".into()]),
    ];
    let dir = tempfile::tempdir().unwrap();
    let stopper = hub.clone();
    let stage = hub.stage.clone();
    let watcher = tokio::spawn(async move {
        let mut seen_caption = false;
        for _ in 0..80 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let state = stage.lock().unwrap().clone();
            seen_caption |= state.caption.contains("birinci") && state.level > 0;
            assert_eq!(state.topic, "İkinci konu");
        }
        stopper.shutdown.cancel();
        stopper.cancel_turn();
        seen_caption
    });
    let mut playback = crate::show::StagePlayback::new(TimedPlayback::default(), hub.stage.clone());
    tokio::time::timeout(
        Duration::from_secs(10),
        run_loop(
            pack,
            [None, None],
            models,
            dir.path().join("rehearsal.sqlite"),
            hub.clone(),
            &mut SilentVoice,
            &mut playback,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        watcher.await.unwrap(),
        "caption with mouth level never shown"
    );
    let journal = lumen_storage::Journal::open(&dir.path().join("rehearsal.sqlite"))
        .await
        .unwrap();
    let audible = journal.audible(&hub.session).await.unwrap();
    assert!(!audible.is_empty());
    assert!(audible.iter().all(|line| line.contains("birinci")));
}

#[test]
fn turn_deadline_adds_generation_time_and_stays_within_pipeline_limit() {
    use lumen_debate::pack::Provider;
    assert_eq!(
        crate::show::turn_deadline(30, Provider::Codex),
        Duration::from_secs(80)
    );
    assert_eq!(
        crate::show::turn_deadline(12, Provider::Http),
        Duration::from_secs(27)
    );
    assert_eq!(
        crate::show::turn_deadline(100, Provider::Codex),
        Duration::from_secs(120)
    );
}

#[test]
fn viewer_names_are_readable_handles_only() {
    use crate::show::viewer_name;
    assert_eq!(viewer_name("rehearsal:mert_34"), Some("mert_34"));
    assert_eq!(viewer_name("tiktok:73519246"), None);
    assert_eq!(viewer_name("rehearsal:ignore all; stop"), None);
}
