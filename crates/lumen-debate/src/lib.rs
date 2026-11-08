//! Deterministic debate floor and authoritative playback state.
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnId(uuid::Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorError {
    Occupied,
    Paused,
    Stopped,
    StaleTurn,
    OutOfOrder,
    InvalidInput,
    QueueFull,
}

struct ActiveTurn {
    id: TurnId,
    next_sequence: u64,
    pending: VecDeque<(u64, String)>,
}

#[derive(Default)]
pub struct Floor {
    active: Option<ActiveTurn>,
    paused: bool,
    stopped: bool,
    transcript: Vec<String>,
}

impl Floor {
    pub fn grant(&mut self, speaker: &str) -> Result<TurnId, FloorError> {
        if self.stopped {
            return Err(FloorError::Stopped);
        }
        if self.paused {
            return Err(FloorError::Paused);
        }
        if self.active.is_some() {
            return Err(FloorError::Occupied);
        }
        if speaker.trim().is_empty() || speaker.len() > 128 {
            return Err(FloorError::InvalidInput);
        }
        let id = TurnId(uuid::Uuid::new_v4());
        self.active = Some(ActiveTurn {
            id,
            next_sequence: 0,
            pending: VecDeque::new(),
        });
        Ok(id)
    }

    fn active(&mut self, id: TurnId) -> Result<&mut ActiveTurn, FloorError> {
        self.active
            .as_mut()
            .filter(|turn| turn.id == id)
            .ok_or(FloorError::StaleTurn)
    }

    pub fn queue(&mut self, id: TurnId, sequence: u64, text: &str) -> Result<(), FloorError> {
        let turn = self.active(id)?;
        if text.trim().is_empty() || text.len() > 4096 {
            return Err(FloorError::InvalidInput);
        }
        if turn.pending.len() >= 32 {
            return Err(FloorError::QueueFull);
        }
        if sequence != turn.next_sequence {
            return Err(FloorError::OutOfOrder);
        }
        turn.next_sequence = sequence.checked_add(1).ok_or(FloorError::OutOfOrder)?;
        turn.pending.push_back((sequence, text.to_owned()));
        Ok(())
    }

    pub fn pending(&self, id: TurnId, sequence: u64) -> Result<&str, FloorError> {
        let turn = self
            .active
            .as_ref()
            .filter(|turn| turn.id == id)
            .ok_or(FloorError::StaleTurn)?;
        match turn.pending.front() {
            Some((next, text)) if *next == sequence => Ok(text),
            _ => Err(FloorError::OutOfOrder),
        }
    }

    /// Call only after the local audio backend confirms the complete segment played.
    pub fn played(&mut self, id: TurnId, sequence: u64) -> Result<(), FloorError> {
        let turn = self.active(id)?;
        if turn.pending.front().map(|segment| segment.0) != Some(sequence) {
            return Err(FloorError::OutOfOrder);
        }
        let (_, text) = turn.pending.pop_front().expect("front checked above");
        self.transcript.push(text);
        Ok(())
    }

    pub fn check_finish(&self, id: TurnId) -> Result<(), FloorError> {
        let turn = self
            .active
            .as_ref()
            .filter(|turn| turn.id == id)
            .ok_or(FloorError::StaleTurn)?;
        if !turn.pending.is_empty() {
            return Err(FloorError::Occupied);
        }
        Ok(())
    }
    pub fn finish(&mut self, id: TurnId) -> Result<(), FloorError> {
        self.check_finish(id)?;
        self.active = None;
        Ok(())
    }
    pub fn transcript(&self) -> &[String] {
        &self.transcript
    }
    pub fn cancel(&mut self) {
        self.active = None;
    }
    pub fn pause(&mut self) {
        self.cancel();
        self.paused = true;
    }
    pub fn resume(&mut self) {
        self.paused = false;
    }
    pub fn stop(&mut self) {
        self.cancel();
        self.stopped = true;
    }
}

pub mod session;
pub use session::{Session, SessionError};
pub mod director;
pub mod provider;

pub mod pack;

pub mod pipeline;
