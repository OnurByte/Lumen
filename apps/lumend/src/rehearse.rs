//! Rehearsal room backend: the real Director, floor, session journal, audience ingress and
//! control socket, without TikTok, TTS or an audio device. Speech duration is simulated from
//! text length, so captions, speaker changes and mouth levels behave like a live show.
use crate::show::{Mode, Show};
use lumen_debate::{
    pack::ShowPack,
    pipeline::Playback,
    provider::{ModelProvider, ModelRequest},
};
use lumen_voice::{
    PcmFrame,
    worker::{VoiceCapabilities, VoiceHealth, VoiceProvider, VoiceRequest},
};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

const RATE: u32 = 24_000;

/// Speaks nothing; emits one silent frame whose length matches a natural speaking pace.
pub struct SilentVoice;

pub fn speech_ms(text: &str) -> u64 {
    (text.chars().count() as u64 * 62).clamp(1_200, 14_000)
}

impl VoiceProvider for SilentVoice {
    async fn synthesize_stream(
        &mut self,
        request: VoiceRequest,
        output: Sender<PcmFrame>,
        cancel: CancellationToken,
    ) -> Result<(), String> {
        let samples = (speech_ms(&request.text) * u64::from(RATE) / 1000) as usize;
        let frame = PcmFrame {
            generation: request.generation,
            utterance: request.utterance,
            sequence: 0,
            sample_rate: RATE,
            channels: 1,
            last: true,
            samples: vec![0.0; samples],
        };
        tokio::select! {
            _ = cancel.cancelled() => Err("cancelled".into()),
            sent = output.send(frame) => sent.map_err(|_| "playback closed".into()),
        }
    }
    async fn cancel(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn list_voices(&self) -> &[&str] {
        &[]
    }
    fn capabilities(&self) -> VoiceCapabilities {
        VoiceCapabilities {
            streaming: false,
            cancellation: true,
            sample_rate: RATE,
            channels: 1,
        }
    }
    fn health_check(&mut self) -> Result<VoiceHealth, String> {
        Ok(VoiceHealth {
            process_alive: true,
            model_produced_audio: false,
        })
    }
}

/// Acknowledges each utterance once its simulated duration has elapsed.
#[derive(Default)]
pub struct TimedPlayback {
    current: Option<(u64, u64)>,
    samples: usize,
    due: Option<(Instant, Instant)>,
}

impl Playback for TimedPlayback {
    fn level(&self) -> u8 {
        match self.due {
            Some((start, due)) if Instant::now() < due => {
                // Syllable-like envelope so stage mouths move while "speaking".
                let t = start.elapsed().as_secs_f32();
                let v = ((t * 11.0).sin().abs() * 0.7 + (t * 3.1).sin().abs() * 0.3) * 85.0;
                (v as u8).clamp(8, 100)
            }
            _ => 0,
        }
    }
    fn begin(&mut self, generation: u64, utterance: u64) -> Result<(), String> {
        self.current = Some((generation, utterance));
        self.samples = 0;
        self.due = None;
        Ok(())
    }
    fn has_capacity(&self, _: usize) -> bool {
        true
    }
    fn push(&mut self, frame: PcmFrame) -> Result<(), String> {
        if self.current != Some((frame.generation, frame.utterance)) || frame.sample_rate == 0 {
            return Err("stale or invalid rehearsal frame".into());
        }
        self.samples += frame.samples.len();
        if frame.last {
            let ms = self.samples as u64 * 1000 / u64::from(frame.sample_rate);
            let now = Instant::now();
            self.due = Some((now, now + Duration::from_millis(ms)));
        }
        Ok(())
    }
    fn event(&mut self) -> Option<Result<(u64, u64), String>> {
        let (_, due) = self.due?;
        if Instant::now() < due {
            return None;
        }
        self.due = None;
        self.current.take().map(Ok)
    }
    fn cancel(&mut self) {
        self.current = None;
        self.due = None;
    }
}

/// Speaks skill-sheet sample lines in order, reacting to audience questions first.
pub struct ScriptedModel {
    lines: Vec<String>,
    cursor: AtomicUsize,
}

impl ScriptedModel {
    pub fn new(lines: Vec<String>) -> Self {
        Self {
            lines,
            cursor: AtomicUsize::new(0),
        }
    }
    fn next_line(&self) -> String {
        let i = self.cursor.fetch_add(1, Ordering::Relaxed);
        self.lines[i % self.lines.len()].clone()
    }
}

/// Neutral natural lines used only when a character has no skill sheet yet.
fn fallback_lines() -> Vec<String> {
    [
        "Bakın, bu meseleye tek cümleyle bakarsak işin özünü kaçırırız.",
        "Haklı olduğunuz bir nokta var, kabul ediyorum; ama sonuca katılmıyorum.",
        "Somut bir örnekle gidelim, yoksa havada konuşuyoruz.",
        "Bilmediğim bir şeyi biliyormuş gibi yapmayacağım; ama mantığım şu.",
        "Bir dakika, bunu biraz daha sade anlatayım.",
        "Herkesin evinde bunun bir örneği vardır aslında.",
    ]
    .map(String::from)
    .to_vec()
}

impl ModelProvider for ScriptedModel {
    async fn stream(
        &self,
        request: ModelRequest,
        output: Sender<String>,
        cancel: CancellationToken,
    ) -> Result<(), String> {
        let mut text = String::new();
        if let Some(question) = &request.audience {
            let short: String = question.chars().take(60).collect();
            text.push_str(&format!("Sohbetten bir soru geldi: \"{short}\". "));
        }
        text.push_str(&self.next_line());
        if !text.ends_with(['.', '!', '?']) {
            text.push('.');
        }
        text.push(' ');
        text.push_str(&self.next_line());
        for word in text.split_inclusive(' ') {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err("cancelled".into()),
                _ = tokio::time::sleep(Duration::from_millis(35)) => {}
            }
            output
                .send(word.to_owned())
                .await
                .map_err(|_| "text consumer disconnected")?;
        }
        Ok(())
    }
}

pub fn scripted_models(pack: &ShowPack, skills: &[Option<String>; 2]) -> Vec<ScriptedModel> {
    pack.agents
        .iter()
        .zip(skills)
        .map(|(_, skill)| {
            let lines = skill
                .as_deref()
                .map(lumen_personagen::skill::sample_lines)
                .filter(|lines| !lines.is_empty())
                .unwrap_or_else(fallback_lines);
            ScriptedModel::new(lines)
        })
        .collect()
}

pub fn load_skills(pack_path: &Path, pack: &ShowPack) -> Result<[Option<String>; 2], String> {
    let load = |i: usize| lumen_personagen::skill::combined(pack_path, pack, i);
    Ok([load(0)?, load(1)?])
}

fn default_runtime_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_else(|| "/tmp".into()))
        .join("lumen")
}

const USAGE: &str = "usage: lumend rehearse PACK.toml [--models] [--socket PATH] [--journal PATH]";

pub async fn run(args: &[String]) -> Result<(), String> {
    let (mut pack_path, mut models, mut socket, mut journal) = (None, false, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--models" => models = true,
            "--socket" => socket = Some(PathBuf::from(it.next().ok_or(USAGE)?)),
            "--journal" => journal = Some(PathBuf::from(it.next().ok_or(USAGE)?)),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            path if pack_path.is_none() => pack_path = Some(PathBuf::from(path)),
            _ => return Err(USAGE.into()),
        }
    }
    let pack_path = pack_path.ok_or(USAGE)?;
    let pack = crate::load_pack(&pack_path)?;
    let skills = load_skills(&pack_path, &pack)?;
    let dir = default_runtime_dir();
    if socket.is_none() || journal.is_none() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let socket = socket.unwrap_or_else(|| dir.join("control.sock"));
    let journal = journal.unwrap_or_else(|| dir.join("rehearsal.sqlite"));
    let rotation = pack.rotation();
    println!(
        "REHEARSAL ROOM — no TikTok, no TTS, no audio device ({})",
        if models {
            "pack model endpoints"
        } else {
            "scripted lines from skill sheets"
        }
    );
    println!(
        "show: {} — {} / {}",
        pack.name, pack.agents[0].name, pack.agents[1].name
    );
    for (agent, skill) in pack.agents.iter().zip(&skills) {
        println!(
            "  {}: {}",
            agent.name,
            if skill.is_some() {
                "skill sheet loaded"
            } else {
                "no skill sheet (fallback lines)"
            }
        );
    }
    println!(
        "topics: {} (every {} min); first: {}",
        rotation.len(),
        pack.topic_minutes,
        rotation[0]
    );
    println!(
        "open the room: lumen-studio --rehearsal {}",
        socket.display()
    );
    crate::control::serve(
        &socket,
        &journal,
        Some(Show {
            pack,
            skills,
            mode: Mode::Rehearsal { models },
        }),
    )
    .await
}
