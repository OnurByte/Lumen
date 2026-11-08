//! Serializes playback acknowledgments with durable journal writes.
use crate::{Floor, FloorError, TurnId};
use lumen_storage::Journal;

#[derive(Debug)]
pub enum SessionError {
    Floor(FloorError),
    Storage(sqlx::Error),
}

pub struct Session {
    id: String,
    floor: Floor,
    journal: Journal,
}

impl Session {
    pub fn new(id: &str, journal: Journal) -> Result<Self, FloorError> {
        if id.trim().is_empty() || id.len() > 128 {
            return Err(FloorError::InvalidInput);
        }
        Ok(Self {
            id: id.into(),
            floor: Floor::default(),
            journal,
        })
    }

    pub fn resume(&mut self) {
        self.floor.resume();
    }

    pub fn grant(&mut self, speaker: &str) -> Result<TurnId, FloorError> {
        self.floor.grant(speaker)
    }

    pub fn queue(&mut self, turn: TurnId, sequence: u64, text: &str) -> Result<(), FloorError> {
        self.floor.queue(turn, sequence, text)
    }

    pub async fn played(
        &mut self,
        turn: TurnId,
        sequence: u64,
        monotonic_ms: u64,
    ) -> Result<(), SessionError> {
        let text = self
            .floor
            .pending(turn, sequence)
            .map_err(SessionError::Floor)?;
        if let Err(error) = self
            .journal
            .append(&self.id, monotonic_ms, "played", text)
            .await
        {
            // Persistence failure must prevent further generation against incomplete history.
            self.floor.pause();
            return Err(SessionError::Storage(error));
        }
        self.floor
            .played(turn, sequence)
            .map_err(SessionError::Floor)
    }

    pub async fn finish(&mut self, turn: TurnId, monotonic_ms: u64) -> Result<(), SessionError> {
        self.floor.check_finish(turn).map_err(SessionError::Floor)?;
        if let Err(error) = self
            .journal
            .append(&self.id, monotonic_ms, "control", "turn_finished")
            .await
        {
            self.floor.pause();
            return Err(SessionError::Storage(error));
        }
        self.floor.finish(turn).map_err(SessionError::Floor)
    }

    pub async fn cancel(&mut self, monotonic_ms: u64) -> Result<(), SessionError> {
        self.floor.cancel();
        if let Err(error) = self
            .journal
            .append(&self.id, monotonic_ms, "cancelled", "")
            .await
        {
            self.floor.pause();
            return Err(SessionError::Storage(error));
        }
        Ok(())
    }
}
