//! Streaming turn delivery. Text validation here is not a substitute for content moderation.
use crate::{
    Session, TurnId,
    provider::{ModelProvider, ModelRequest},
};
use lumen_voice::{
    PcmFrame,
    output::AudioOutput,
    worker::{VoiceProvider, VoiceRequest},
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub trait Playback {
    fn caption(&mut self, _speaker: &str, _text: &str) {}
    fn level(&self) -> u8 {
        0
    }
    fn begin(&mut self, generation: u64, utterance: u64) -> Result<(), String>;
    fn has_capacity(&self, samples: usize) -> bool;
    fn push(&mut self, frame: PcmFrame) -> Result<(), String>;
    fn event(&mut self) -> Option<Result<(u64, u64), String>>;
    fn cancel(&mut self);
}
impl Playback for AudioOutput {
    fn level(&self) -> u8 {
        AudioOutput::level(self)
    }
    fn begin(&mut self, generation: u64, utterance: u64) -> Result<(), String> {
        AudioOutput::begin(self, generation, utterance)
    }
    fn has_capacity(&self, samples: usize) -> bool {
        AudioOutput::has_capacity(self, samples)
    }
    fn push(&mut self, frame: PcmFrame) -> Result<(), String> {
        AudioOutput::push(self, frame)
    }
    fn event(&mut self) -> Option<Result<(u64, u64), String>> {
        self.try_event()
    }
    fn cancel(&mut self) {
        AudioOutput::cancel(self);
    }
}

pub struct TurnContext {
    pub generation: u64,
    pub speaker: String,
    pub voice: String,
    pub deadline: Duration,
    pub clock: Instant,
}
impl TurnContext {
    fn now(&self) -> u64 {
        self.clock.elapsed().as_millis().min(i64::MAX as u128) as u64
    }
}

#[derive(Default)]
pub struct Segments {
    buffer: String,
    total: usize,
}
impl Segments {
    pub fn push(&mut self, delta: &str) -> Result<Vec<String>, String> {
        if self.total + delta.len() > 8192
            || delta
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err("invalid or excessive model text".into());
        }
        self.total += delta.len();
        self.buffer.push_str(delta);
        let mut output = Vec::new();
        loop {
            let sentence = self
                .buffer
                .char_indices()
                .find_map(|(index, c)| {
                    if !matches!(c, '.' | '?' | '!') {
                        return None;
                    }
                    let end = index + c.len_utf8();
                    if !self.buffer[end..]
                        .chars()
                        .next()
                        .is_some_and(char::is_whitespace)
                    {
                        return None;
                    }
                    let word = self.buffer[..end].split_whitespace().last().unwrap_or("");
                    if ["Dr.", "Prof.", "Doç.", "Sn.", "vb.", "vs."].contains(&word) {
                        return None;
                    }
                    Some(end)
                })
                .filter(|end| *end <= 256);
            let end = match sentence {
                Some(end) => end,
                None if self.buffer.len() > 256 => {
                    let mut end = 256;
                    while !self.buffer.is_char_boundary(end) {
                        end -= 1;
                    }
                    self.buffer[..end]
                        .rfind(char::is_whitespace)
                        .filter(|index| *index > 0)
                        .unwrap_or(end)
                }
                None => break,
            };
            let text = self
                .buffer
                .drain(..end)
                .collect::<String>()
                .trim()
                .to_owned();
            if !text.is_empty() {
                output.push(text);
            }
        }
        Ok(output)
    }
    pub fn finish(&mut self) -> Result<Option<String>, String> {
        let text = std::mem::take(&mut self.buffer).trim().to_owned();
        Ok((!text.is_empty()).then_some(text))
    }
}

pub async fn run_turn<M: ModelProvider, V: VoiceProvider, S: Playback>(
    model: &M,
    voice: &mut V,
    sink: &mut S,
    session: &mut Session,
    request: ModelRequest,
    context: TurnContext,
    cancel: CancellationToken,
) -> Result<(), String> {
    if context.deadline.is_zero() || context.deadline > Duration::from_secs(120) {
        return Err("invalid turn deadline".into());
    }
    let turn = session
        .grant(&context.speaker)
        .map_err(|e| format!("{e:?}"))?;
    let (tx, mut rx) = mpsc::channel(16);
    let production = model.stream(request, tx, cancel.child_token());
    let consumption = async {
        let mut segments = Segments::default();
        let mut sequence = 0u64;
        while let Some(delta) = rx.recv().await {
            for text in segments.push(&delta)? {
                deliver(voice, sink, session, turn, sequence, text, &context).await?;
                sequence += 1;
            }
        }
        if let Some(text) = segments.finish()? {
            deliver(voice, sink, session, turn, sequence, text, &context).await?;
            sequence += 1;
        }
        if sequence == 0 {
            return Err("model produced no spoken text".into());
        }
        Ok(())
    };
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("turn cancelled".into()),
        result = tokio::time::timeout(context.deadline, async { tokio::try_join!(production, consumption).map(|_| ()) }) => result.unwrap_or_else(|_| Err("turn deadline exceeded".into())),
    };
    match result {
        Ok(()) => session
            .finish(turn, context.now())
            .await
            .map_err(|e| format!("{e:?}")),
        Err(error) => {
            sink.cancel();
            let stopped = voice.cancel().await;
            let journaled = session.cancel(context.now()).await;
            stopped?;
            journaled.map_err(|e| format!("{e:?}"))?;
            Err(error)
        }
    }
}

async fn deliver<V: VoiceProvider, S: Playback>(
    voice: &mut V,
    sink: &mut S,
    session: &mut Session,
    turn: TurnId,
    sequence: u64,
    text: String,
    context: &TurnContext,
) -> Result<(), String> {
    session
        .queue(turn, sequence, &text)
        .map_err(|e| format!("{e:?}"))?;
    sink.begin(context.generation, sequence)?;
    let (tx, mut rx) = mpsc::channel(2);
    let synthesis = voice.synthesize_stream(
        VoiceRequest {
            generation: context.generation,
            utterance: sequence,
            text: text.clone(),
            voice: context.voice.clone(),
        },
        tx,
        CancellationToken::new(),
    );
    let playback = async {
        let mut pending: Option<PcmFrame> = None;
        let mut closed = false;
        let mut final_queued = false;
        let mut captioned = false;
        loop {
            if pending
                .as_ref()
                .is_some_and(|frame| sink.has_capacity(frame.samples.len()))
            {
                let frame = pending.take().expect("pending checked");
                final_queued |= frame.last;
                sink.push(frame)?;
                if !captioned {
                    sink.caption(&context.speaker, &text);
                    captioned = true;
                }
            }
            if let Some(event) = sink.event()
                && event? == (context.generation, sequence)
            {
                if !final_queued {
                    return Err("playback completed before final PCM".into());
                }
                return Ok(());
            }
            if pending.is_none() && !closed {
                tokio::select! {
                    frame = rx.recv() => match frame {
                        Some(frame) => pending = Some(frame),
                        None => { closed = true; if !final_queued { return Err("voice ended without final PCM".into()); } },
                    },
                    _ = tokio::time::sleep(Duration::from_millis(5)) => {},
                }
            } else {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    };
    tokio::try_join!(synthesis, playback)?;
    session
        .played(turn, sequence, context.now())
        .await
        .map_err(|e| format!("{e:?}"))
}
