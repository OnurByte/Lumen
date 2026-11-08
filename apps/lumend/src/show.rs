//! Show backend runs on its own thread; the control endpoint remains independent.
use crate::control::Hub;
use lumen_debate::{
    Session,
    director::Director,
    pack::ShowPack,
    pipeline::{Playback, TurnContext, run_turn},
    provider::{ModelProvider, ModelRequest},
};
use lumen_storage::Journal;
use lumen_voice::{
    output::AudioOutput,
    worker::{VoiceProvider, WorkerVoice},
};
use std::{path::PathBuf, sync::Arc, time::Duration};

/// What the daemon runs behind the control socket.
pub struct Show {
    pub pack: ShowPack,
    /// Persona skill sheets, in agent order.
    pub skills: [Option<String>; 2],
    pub mode: Mode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Pack models, Python TTS and the audio device.
    Live,
    /// Rehearsal room: no TTS or audio device; speech timing is simulated. `models` uses the
    /// pack's model endpoints, otherwise skill-sheet sample lines are spoken.
    Rehearsal { models: bool },
}

/// TTS worker location: `$LUMEN_TTS_DIR`, then the installed data dir, then the checkout.
fn tts_root() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    [
        std::env::var_os("LUMEN_TTS_DIR").map(PathBuf::from),
        data.map(|d| d.join("lumen/workers/tts-python")),
    ]
    .into_iter()
    .flatten()
    .find(|dir| dir.join("worker.py").is_file())
    .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../workers/tts-python"))
}

/// Readable viewer name from an ingress user id (`rehearsal:mert_34` -> `mert_34`); numeric
/// platform ids are not names and are dropped.
pub fn viewer_name(user: &str) -> Option<&str> {
    let name = user.split_once(':').map_or(user, |(_, n)| n).trim();
    (!name.is_empty()
        && name.len() <= 40
        && !name.bytes().all(|b| b.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || "._-".contains(c)))
    .then_some(name)
}

/// Speech budget plus time for the model to produce the turn (GPT via codex needs ~8 s).
pub fn turn_deadline(max_seconds: u16, provider: lumen_debate::pack::Provider) -> Duration {
    let slack = match provider {
        lumen_debate::pack::Provider::Codex => 50,
        lumen_debate::pack::Provider::Http => 15,
    };
    Duration::from_secs((u64::from(max_seconds) + slack).min(120))
}

pub fn build_models(pack: &ShowPack) -> Result<Vec<crate::gpt::AnyModel>, String> {
    pack.agents.iter().map(crate::gpt::build).collect()
}

pub fn spawn(
    show: Show,
    database: PathBuf,
    hub: Arc<Hub>,
) -> Result<std::thread::JoinHandle<()>, String> {
    let Show { pack, skills, mode } = show;
    std::thread::Builder::new()
        .name("lumen-show".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let mut status = hub.status.blocking_lock();
                    status.paused = true;
                    status.error = Some(format!("show runtime: {error}"));
                    return;
                }
            };
            runtime.block_on(async {
                let result = run_mode(mode, pack, skills, database, hub.clone()).await;
                hub.available
                    .store(false, std::sync::atomic::Ordering::Release);
                if let Ok(mut stage) = hub.stage.lock() {
                    stage.clear_speech();
                }
                if let Err(error) = result {
                    hub.fail(error).await;
                }
            });
        })
        .map_err(|e| e.to_string())
}

async fn run_mode(
    mode: Mode,
    pack: ShowPack,
    skills: [Option<String>; 2],
    database: PathBuf,
    hub: Arc<Hub>,
) -> Result<(), String> {
    let python = tts_root().join(".venv/bin/python");
    let captions_only = std::env::var("LUMEN_VOICE").is_ok_and(|v| v == "captions") || !python.exists();
    match mode {
        // The Python TTS worker is optional: without it the live show runs captions-only in pure Rust.
        Mode::Live if captions_only => {
            eprintln!("voice: captions-only (no Python TTS worker at {}; set up workers/tts-python for speech)", python.display());
            let mut voice = crate::rehearse::SilentVoice;
            let mut output = StagePlayback::new(crate::rehearse::TimedPlayback::default(), hub.stage.clone());
            let models = build_models(&pack)?;
            run_loop(pack, skills, models, database, hub, &mut voice, &mut output).await
        }
        Mode::Live => {
            let output = AudioOutput::open(std::env::var("LUMEN_AUDIO_DEVICE").ok().as_deref())?;
            let root = tts_root();
            let mut voice = WorkerVoice::new(root.join(".venv/bin/python"), root.join("worker.py"));
            let mut output = StagePlayback::new(output, hub.stage.clone());
            let models = build_models(&pack)?;
            run_loop(pack, skills, models, database, hub, &mut voice, &mut output).await
        }
        Mode::Rehearsal { models } => {
            let mut voice = crate::rehearse::SilentVoice;
            let mut output =
                StagePlayback::new(crate::rehearse::TimedPlayback::default(), hub.stage.clone());
            if models {
                let models = build_models(&pack)?;
                run_loop(pack, skills, models, database, hub, &mut voice, &mut output).await
            } else {
                let models = crate::rehearse::scripted_models(&pack, &skills);
                run_loop(pack, skills, models, database, hub, &mut voice, &mut output).await
            }
        }
    }
}

pub async fn run_loop<M: ModelProvider, V: VoiceProvider, S: Playback>(
    pack: ShowPack,
    skills: [Option<String>; 2],
    models: Vec<M>,
    database: PathBuf,
    hub: Arc<Hub>,
    voice: &mut V,
    sink: &mut S,
) -> Result<(), String> {
    {
        let mut stage = hub.stage.lock().map_err(|_| "stage unavailable")?;
        stage.agents = [pack.agents[0].name.clone(), pack.agents[1].name.clone()];
        stage.sides = pack
            .agents
            .each_ref()
            .map(|a| a.side.clone().unwrap_or_default());
        stage.avatars = pack
            .agents
            .each_ref()
            .map(|a| a.avatar.clone().unwrap_or_default());
        stage.topic = pack.topic.clone();
        stage.show = pack.name.clone();
        stage.session_total_s = (pack.rotation().len() as u32) * u32::from(pack.topic_minutes) * 60;
    }
    if models.len() != 2 {
        return Err("exactly two models required".into());
    }
    let journal = Journal::open(&database).await.map_err(|e| e.to_string())?;
    let mut session = Session::new(
        &hub.session,
        Journal::open(&database).await.map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut director = Director::with_topics(
        [pack.agents[0].name.clone(), pack.agents[1].name.clone()],
        pack.rotation(),
        u64::from(pack.topic_minutes) * 60_000,
        pack.format,
    )
    .map_err(|e| format!("{e:?}"))?;
    director.end_after_last(true);
    hub.available
        .store(true, std::sync::atomic::Ordering::Release);
    let mut generation = 0u64;
    loop {
        if hub.shutdown.is_cancelled() {
            break;
        }
        let notified = hub.changed.notified();
        let status = hub.status.lock().await.clone();
        if status.paused || status.muted || status.stopped {
            tokio::select! { _ = notified => {}, _ = hub.shutdown.cancelled() => break }
            continue;
        }
        session.resume();
        if hub
            .skip_topic
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            director.advance_topic();
        }
        let pending = hub.pending_topics.lock().ok().and_then(|mut p| p.take());
        if let Some(list) = pending {
            let rotate = u64::from(list.minutes) * 60_000;
            let total = list.topics.len() as u32 * u32::from(list.minutes) * 60;
            director
                .set_topics(list.topics, rotate)
                .map_err(|e| format!("{e:?}"))?;
            hub.session_started_ms
                .store(0, std::sync::atomic::Ordering::Release);
            let mut stage = hub.stage.lock().map_err(|_| "stage unavailable")?;
            stage.session_total_s = total;
            stage.finished = false;
        }
        let now_ms = hub.clock.elapsed().as_millis() as u64;
        let Some(brief) = director.next(
            &mut *hub.audience.lock().map_err(|_| "audience unavailable")?,
            now_ms,
        ) else {
            if !director.finished() {
                return Err("director unavailable".into());
            }
            // Every question has been debated: end the session and wait for the operator.
            {
                let mut stage = hub.stage.lock().map_err(|_| "stage unavailable")?;
                stage.clear_speech();
                stage.finished = true;
            }
            hub.status.lock().await.paused = true;
            hub.topic_ends_ms
                .store(0, std::sync::atomic::Ordering::Release);
            continue;
        };
        if hub
            .session_started_ms
            .load(std::sync::atomic::Ordering::Acquire)
            == 0
        {
            hub.session_started_ms
                .store(now_ms.max(1), std::sync::atomic::Ordering::Release);
        }
        {
            let now = hub.clock.elapsed().as_millis() as u64;
            hub.topic_ends_ms.store(
                if brief.topic_remaining_ms == 0 {
                    0
                } else {
                    now + brief.topic_remaining_ms
                },
                std::sync::atomic::Ordering::Release,
            );
            let mut stage = hub.stage.lock().map_err(|_| "stage unavailable")?;
            stage.question = brief.audience.as_ref().map(|comment| comment.text.clone());
            stage.topic = brief.topic.clone();
            stage.topic_number = brief.topic_number;
            stage.thinking = Some(brief.speaker.clone());
            stage.phase = brief.phase.map_or(String::new(), |p| p.label().to_owned());
            stage.topic_total = brief.topic_total;
        }
        let index = usize::from(brief.speaker != pack.agents[0].name);
        let agent = &pack.agents[index];
        let context = journal
            .recent_audible(&hub.session, 32)
            .await
            .map_err(|e| e.to_string())?;
        generation = generation.checked_add(1).ok_or("generation exhausted")?;
        let cancel = hub.shutdown.child_token();
        *hub.active
            .lock()
            .map_err(|_| "turn cancellation unavailable")? = Some(cancel.clone());
        // Recheck after publishing the token, so a concurrent pause cannot miss this turn.
        let status = hub.status.lock().await.clone();
        if status.paused || status.muted || status.stopped {
            cancel.cancel();
        }
        let result = run_turn(
            &models[index],
            voice,
            sink,
            &mut session,
            ModelRequest {
                persona: format!(
                    "Your name is {}. The person opposite you is {}. {}",
                    agent.name,
                    pack.agents[1 - index].name,
                    agent.persona
                ),
                position: agent.position.clone(),
                skill: skills[index].clone(),
                phase: {
                    let show = (brief.cue == Some(lumen_debate::director::Cue::ShowOpening))
                        .then(|| format!("The show is called {}.", pack.name));
                    let parts: Vec<String> = show
                        .into_iter()
                        .chain(brief.cue.map(|c| c.instruction().to_owned()))
                        .chain(brief.phase.map(|p| p.instruction().to_owned()))
                        .collect();
                    (!parts.is_empty()).then(|| parts.join(" "))
                },
                topic: brief.topic,
                audible_context: context,
                audience: brief
                    .audience
                    .map(|comment| match viewer_name(&comment.user) {
                        Some(name) => format!("{name}: {}", comment.text),
                        None => comment.text,
                    }),
            },
            TurnContext {
                generation,
                speaker: brief.speaker,
                voice: agent.voice.clone(),
                deadline: turn_deadline(brief.max_seconds, agent.provider),
                clock: hub.clock,
            },
            cancel.clone(),
        )
        .await;
        *hub.active
            .lock()
            .map_err(|_| "turn cancellation unavailable")? = None;
        match result {
            Err(error) if !cancel.is_cancelled() && error == "turn deadline exceeded" => {
                // One slow turn must not take the show off air: drop it and move on.
                if let Ok(mut stage) = hub.stage.lock() {
                    stage.clear_speech();
                }
            }
            Err(error) if !cancel.is_cancelled() => hub.fail(error).await,
            _ => {}
        }
    }
    hub.available
        .store(false, std::sync::atomic::Ordering::Release);
    sink.cancel();
    voice.cancel().await
}

pub struct StagePlayback<S> {
    inner: S,
    stage: Arc<std::sync::Mutex<lumen_protocol::StageState>>,
    current: Option<(u64, u64)>,
}
impl<S: Playback> StagePlayback<S> {
    pub fn new(inner: S, stage: Arc<std::sync::Mutex<lumen_protocol::StageState>>) -> Self {
        Self {
            inner,
            stage,
            current: None,
        }
    }
    fn clear(&self) {
        if let Ok(mut stage) = self.stage.lock() {
            stage.clear_speech();
        }
    }
}
impl<S: Playback> Playback for StagePlayback<S> {
    fn caption(&mut self, speaker: &str, text: &str) {
        if let Ok(mut stage) = self.stage.lock() {
            stage.thinking = None;
            stage.speaker = Some(speaker.into());
            stage.caption = text.into();
        }
    }
    fn level(&self) -> u8 {
        self.inner.level()
    }
    fn begin(&mut self, generation: u64, utterance: u64) -> Result<(), String> {
        self.inner.begin(generation, utterance)?;
        self.current = Some((generation, utterance));
        Ok(())
    }
    fn has_capacity(&self, samples: usize) -> bool {
        self.inner.has_capacity(samples)
    }
    fn push(&mut self, frame: lumen_voice::PcmFrame) -> Result<(), String> {
        self.inner.push(frame)
    }
    fn event(&mut self) -> Option<Result<(u64, u64), String>> {
        if let Ok(mut stage) = self.stage.lock() {
            stage.level = self.inner.level();
        }
        let event = self.inner.event();
        if event
            .as_ref()
            .is_some_and(|result| result.as_ref().map_or(true, |id| Some(*id) == self.current))
        {
            self.clear();
            self.current = None;
        }
        event
    }
    fn cancel(&mut self) {
        self.inner.cancel();
        self.clear();
        self.current = None;
    }
}
