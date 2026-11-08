//! CPAL playback with delayed completion fences. Device timestamps are predictions, not remote-viewer proof.
use crate::{Arbiter, PcmFrame};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    time::Duration,
};

#[derive(Default)]
pub struct PlaybackFence {
    pending: Option<(u64, u64, u64)>,
}
impl PlaybackFence {
    pub fn consumed(&mut self, generation: u64, utterance: u64, end_us: u64) {
        self.pending = Some((generation, utterance, end_us));
    }
    pub fn confirmed(&mut self, now_us: u64) -> Option<(u64, u64)> {
        if self.pending.is_some_and(|(_, _, end)| now_us >= end) {
            return self
                .pending
                .take()
                .map(|(generation, utterance, _)| (generation, utterance));
        }
        None
    }
    pub fn cancel(&mut self) {
        self.pending = None;
    }
}

struct State {
    ready: bool,
    level: u8,
    audio: Arbiter,
    fence: PlaybackFence,
}

pub struct AudioOutput {
    _stream: cpal::Stream,
    state: Arc<Mutex<State>>,
    events: Receiver<Result<(u64, u64), String>>,
}

impl AudioOutput {
    /// Requires float32 24 kHz mono support; fail explicitly rather than misplay another format.
    pub fn open(device_name: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = match device_name {
            Some(name) => host
                .output_devices()
                .map_err(|e| e.to_string())?
                .find(|device| device.description().is_ok_and(|found| found.name() == name))
                .ok_or("audio device not found")?,
            None => host
                .default_output_device()
                .ok_or("no default output device")?,
        };
        let supported = device
            .supported_output_configs()
            .map_err(|e| e.to_string())?
            .find(|config| {
                config.channels() == 1
                    && config.sample_format() == cpal::SampleFormat::F32
                    && config.min_sample_rate() <= 24000
                    && config.max_sample_rate() >= 24000
            })
            .ok_or(
                "device does not support float32 24 kHz mono; select a compatible PipeWire device",
            )?;
        let config = supported.with_sample_rate(24000).config();
        let state = Arc::new(Mutex::new(State {
            ready: false,
            level: 0,
            audio: Arbiter::new(24000, 1).map_err(|e| format!("{e:?}"))?,
            fence: PlaybackFence::default(),
        }));
        let callback_state = state.clone();
        let (sender, events) = mpsc::sync_channel(16);
        let errors = sender.clone();
        let mut origin = None;
        let mut previous = None;
        let mut stable_callbacks = 0u8;
        let stream = device
            .build_output_stream(
                &config,
                move |output: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    output.fill(0.);
                    let Ok(mut state) = callback_state.try_lock() else {
                        return;
                    };
                    let timestamp = info.timestamp();
                    if std::env::var_os("LUMEN_AUDIO_TRACE").is_some() {
                        eprintln!("callback samples={} timestamp={timestamp:?}", output.len());
                    }
                    if previous.is_some_and(|last| timestamp.callback < last) {
                        if state.ready {
                            let _ = sender.try_send(Err(
                                "audio device clock reset; pending audio discarded".into(),
                            ));
                        }
                        state.audio.cancel();
                        state.fence.cancel();
                        origin = Some(timestamp.callback);
                        stable_callbacks = 0;
                    } else {
                        stable_callbacks = stable_callbacks.saturating_add(1);
                    }
                    previous = Some(timestamp.callback);
                    state.ready = stable_callbacks >= 3;
                    if !state.ready {
                        return;
                    }
                    let origin = *origin.get_or_insert(timestamp.callback);
                    let Some(now) = timestamp.callback.duration_since(&origin) else {
                        state.audio.cancel();
                        state.fence.cancel();
                        return;
                    };
                    if let Some(done) = state
                        .fence
                        .confirmed(now.as_micros().min(u64::MAX as u128) as u64)
                    {
                        let _ = sender.try_send(Ok(done));
                    }
                    match state.audio.render(output) {
                        Ok(completed) => {
                            if let Some((generation, utterance)) = completed.first() {
                                let Some(start) = timestamp.playback.duration_since(&origin) else {
                                    state.audio.cancel();
                                    return;
                                };
                                let end = start.saturating_add(Duration::from_secs_f64(
                                    output.len() as f64 / 24000.,
                                ));
                                state.fence.consumed(
                                    *generation,
                                    *utterance,
                                    end.as_micros().min(u64::MAX as u128) as u64,
                                );
                            }
                        }
                        Err(error) => {
                            state.audio.cancel();
                            state.fence.cancel();
                            let _ = sender.try_send(Err(format!("audio render: {error:?}")));
                        }
                    }
                    state.level = if output.is_empty() {
                        0
                    } else {
                        ((output.iter().map(|sample| sample * sample).sum::<f32>()
                            / output.len() as f32)
                            .sqrt()
                            * 100.)
                            .clamp(0., 100.) as u8
                    };
                },
                move |error| {
                    let _ = errors.try_send(Err(format!("audio device: {error}")));
                },
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !state.lock().map_err(|_| "audio state unavailable")?.ready {
            if std::time::Instant::now() >= deadline {
                return Err("audio device clock did not stabilize".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(Self {
            _stream: stream,
            state,
            events,
        })
    }
    pub fn begin(&self, generation: u64, utterance: u64) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "audio state unavailable")?;
        if !state.ready {
            return Err("audio device clock is not stable".into());
        }
        if state.fence.pending.is_some() {
            return Err("previous audio is still playing".into());
        }
        state
            .audio
            .begin(generation, utterance)
            .map_err(|e| format!("{e:?}"))
    }
    pub fn push(&self, frame: PcmFrame) -> Result<(), String> {
        self.state
            .lock()
            .map_err(|_| "audio state unavailable")?
            .audio
            .push(frame)
            .map_err(|e| format!("{e:?}"))
    }
    pub fn cancel(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.audio.cancel();
            state.fence.cancel();
            state.level = 0;
        }
        while self.events.try_recv().is_ok() {}
    }
    pub fn level(&self) -> u8 {
        self.state.lock().map_or(0, |state| state.level)
    }
    pub fn has_capacity(&self, samples: usize) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.audio.has_capacity(samples))
    }
    pub fn try_event(&self) -> Option<Result<(u64, u64), String>> {
        self.events.try_recv().ok()
    }
}
