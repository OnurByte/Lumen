//! Bounded PCM arbitration. Buffer consumption is not proof of device playback.
use std::collections::VecDeque;

#[derive(Debug)]
pub struct PcmFrame {
    pub generation: u64,
    pub utterance: u64,
    pub sequence: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub last: bool,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioError {
    Invalid,
    Stale,
    Sequence,
    Full,
    Occupied,
}

struct Active {
    generation: u64,
    utterance: u64,
    next: u64,
    final_queued: bool,
}

pub struct Arbiter {
    sample_rate: u32,
    channels: u16,
    active: Option<Active>,
    pending: VecDeque<(PcmFrame, usize)>,
    samples: usize,
}

impl Arbiter {
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, AudioError> {
        if !(8000..=192000).contains(&sample_rate) || !(1..=2).contains(&channels) {
            return Err(AudioError::Invalid);
        }
        Ok(Self {
            sample_rate,
            channels,
            active: None,
            pending: VecDeque::new(),
            samples: 0,
        })
    }

    pub fn begin(&mut self, generation: u64, utterance: u64) -> Result<(), AudioError> {
        if self.active.is_some() {
            return Err(AudioError::Occupied);
        }
        self.active = Some(Active {
            generation,
            utterance,
            next: 0,
            final_queued: false,
        });
        Ok(())
    }

    pub fn push(&mut self, frame: PcmFrame) -> Result<(), AudioError> {
        let active = self.active.as_mut().ok_or(AudioError::Stale)?;
        if active.generation != frame.generation || active.utterance != frame.utterance {
            return Err(AudioError::Stale);
        }
        if active.final_queued || active.next != frame.sequence {
            return Err(AudioError::Sequence);
        }
        if frame.sample_rate != self.sample_rate
            || frame.channels != self.channels
            || frame.samples.is_empty()
            || !frame.samples.len().is_multiple_of(self.channels as usize)
            || frame.samples.len() > self.sample_rate as usize * self.channels as usize
            || frame
                .samples
                .iter()
                .any(|sample| !sample.is_finite() || sample.abs() > 1.)
        {
            return Err(AudioError::Invalid);
        }
        if self.pending.len() >= 32
            || self.samples + frame.samples.len()
                > 2 * self.sample_rate as usize * self.channels as usize
        {
            return Err(AudioError::Full);
        }
        active.next = active.next.checked_add(1).ok_or(AudioError::Sequence)?;
        active.final_queued = frame.last;
        self.samples += frame.samples.len();
        self.pending.push_back((frame, 0));
        Ok(())
    }

    /// Completed IDs identify consumed buffers; a device adapter must confirm playback timestamps.
    pub fn render(&mut self, output: &mut [f32]) -> Result<Vec<(u64, u64)>, AudioError> {
        output.fill(0.);
        if !output.len().is_multiple_of(self.channels as usize) {
            return Err(AudioError::Invalid);
        }
        let mut offset = 0;
        let mut completed = Vec::new();
        while offset < output.len() {
            let Some((frame, cursor)) = self.pending.front_mut() else {
                break;
            };
            let count = (output.len() - offset).min(frame.samples.len() - *cursor);
            output[offset..offset + count]
                .copy_from_slice(&frame.samples[*cursor..*cursor + count]);
            offset += count;
            *cursor += count;
            self.samples -= count;
            if *cursor == frame.samples.len() {
                let (frame, _) = self.pending.pop_front().expect("front exists");
                if frame.last {
                    completed.push((frame.generation, frame.utterance));
                    self.active = None;
                }
            }
        }
        Ok(completed)
    }

    pub fn cancel(&mut self) {
        self.pending.clear();
        self.active = None;
        self.samples = 0;
    }
    pub fn has_capacity(&self, samples: usize) -> bool {
        self.pending.len() < 32
            && self.samples + samples <= 2 * self.sample_rate as usize * self.channels as usize
    }
    pub fn queued_samples(&self) -> usize {
        self.samples
    }
}

pub mod output;
pub mod worker;
