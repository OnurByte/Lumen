//! Deterministic two-agent scheduling; audience text is carried as data.
use crate::FloorError;
use lumen_audience::{AudienceQueue, Comment};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    FreeDebate,
    Crossfire,
    /// Structured debate: each topic runs opening -> rebuttal -> cross-examination -> closing.
    Munazara,
}

/// Münazara stage within a topic, derived from the topic's elapsed share of its time slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Opening,
    Rebuttal,
    Cross,
    Closing,
}

impl Phase {
    fn at(fraction: f32) -> Self {
        match fraction {
            f if f < 0.2 => Phase::Opening,
            f if f < 0.6 => Phase::Rebuttal,
            f if f < 0.8 => Phase::Cross,
            _ => Phase::Closing,
        }
    }
    /// On-screen label for viewers.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Opening => "AÇILIŞ",
            Phase::Rebuttal => "ÇÜRÜTME",
            Phase::Cross => "ÇAPRAZ SORGU",
            Phase::Closing => "KAPANIŞ",
        }
    }
    /// Turn instruction for the speaking model.
    pub fn instruction(self) -> &'static str {
        match self {
            Phase::Opening => {
                "Opening statement: state your side's strongest case on this question in 2-3 clear sentences."
            }
            Phase::Rebuttal => {
                "Rebuttal: answer the opponent's last point directly, then turn it into support for your side."
            }
            Phase::Cross => {
                "Cross-examination: one short pointed question or a one-sentence answer. Keep it under 2 sentences."
            }
            Phase::Closing => {
                "Closing: summarize why your side wins this question in 2 memorable sentences."
            }
        }
    }
}

/// Live-show moment a turn must also cover, on top of debating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// Very first turn: greet viewers, present the show, today's question and the opponent.
    ShowOpening,
    /// A host's first turn: greet, introduce yourself and your side.
    Introduce,
    /// First turn on a new topic: present the question to viewers, then take a stance.
    TopicIntro,
    /// Periodic: turn to the chat, invite comments, then bridge back to the debate.
    ChatCheckIn,
}

impl Cue {
    pub fn instruction(self) -> &'static str {
        match self {
            Cue::ShowOpening => {
                "Yayını sen açıyorsun: izleyicileri sıcak bir \"Merhaba arkadaşlar\" ile selamla, programı ve ilk soruyu tek cümleyle sun, karşındaki konuşmacıyı adıyla tanıt, sonra ilk tavrını koy."
            }
            Cue::Introduce => {
                "Bu senin ilk sözün: izleyicileri selamla, kendini adınla ve hangi tarafta durduğunla kısaca tanıt, sonra rakibine ilk cevabını ver."
            }
            Cue::TopicIntro => {
                "Yeni soruya geçiyoruz: soruyu izleyicilere sen sun, neden önemli olduğunu bir cümleyle söyle, sonra tavrını koy."
            }
            Cue::ChatCheckIn => {
                "Kısa bir an sohbete dön: izleyicilere seslen, yorumlara yazmalarını ve taraf tutmalarını iste, ardından tartışmaya bağla."
            }
        }
    }
}

/// Turns without a chat moment before the hosts turn to the chat on their own.
pub const CHAT_EVERY: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnBrief {
    pub speaker: String,
    pub topic: String,
    pub audience: Option<Comment>,
    pub max_seconds: u16,
    pub heat: u8,
    /// 1-based position of `topic` in the rotation, and the rotation length.
    pub topic_number: u16,
    pub topic_total: u16,
    /// True on the first turn after a topic change (start of show included).
    pub new_topic: bool,
    /// Milliseconds left before the rotation moves on (0 when rotation is off).
    pub topic_remaining_ms: u64,
    /// Münazara stage, when the format and a topic timer are active.
    pub phase: Option<Phase>,
    /// Live-show moment to cover this turn.
    pub cue: Option<Cue>,
}

pub const MAX_TOPICS: usize = 64;

pub struct Director {
    speakers: [String; 2],
    topics: Vec<String>,
    current: usize,
    /// Wall time when the current topic got its first turn.
    topic_started: Option<u64>,
    rotate_ms: u64,
    turns: u64,
    /// Stop after the last question instead of wrapping around.
    end_after_last: bool,
    finished: bool,
    introduced: [bool; 2],
    since_chat: u32,
    format: Format,
    next_speaker: usize,
    paused: bool,
    crossfire_until: u64,
    last_crossfire: Option<u64>,
}

impl Director {
    pub fn new(speakers: [String; 2], topic: &str, format: Format) -> Result<Self, FloorError> {
        Self::with_topics(speakers, vec![topic.into()], 0, format)
    }

    /// Rotates through `topics` every `rotate_ms` of show time (0 = never), wrapping around.
    pub fn with_topics(
        speakers: [String; 2],
        topics: Vec<String>,
        rotate_ms: u64,
        format: Format,
    ) -> Result<Self, FloorError> {
        if speakers
            .iter()
            .any(|name| name.trim().is_empty() || name.len() > 128)
            || speakers[0] == speakers[1]
            || topics.is_empty()
            || topics.len() > MAX_TOPICS
            || topics
                .iter()
                .any(|topic| topic.trim().is_empty() || topic.len() > 1000)
        {
            return Err(FloorError::InvalidInput);
        }
        Ok(Self {
            speakers,
            topics,
            current: 0,
            topic_started: None,
            rotate_ms,
            turns: 0,
            end_after_last: false,
            finished: false,
            introduced: [false; 2],
            since_chat: 0,
            format,
            next_speaker: 0,
            paused: false,
            crossfire_until: 0,
            last_crossfire: None,
        })
    }
    pub fn next(&mut self, audience: &mut AudienceQueue, now_ms: u64) -> Option<TurnBrief> {
        if self.paused || self.finished {
            return None;
        }
        if self.rotate_ms > 0
            && self
                .topic_started
                .is_some_and(|start| now_ms.saturating_sub(start) >= self.rotate_ms)
        {
            self.advance_topic();
            if self.finished {
                return None;
            }
        }
        let new_topic = self.topic_started.is_none();
        let started = *self.topic_started.get_or_insert(now_ms);
        let mut comment = audience.next(now_ms);
        if comment
            .as_ref()
            .is_some_and(|comment| comment.text.trim() == "!çapraz")
        {
            if self
                .last_crossfire
                .is_none_or(|last| now_ms.saturating_sub(last) >= 60_000)
            {
                self.crossfire_until = now_ms.saturating_add(30_000);
                self.last_crossfire = Some(now_ms);
            }
            comment = None;
        }
        let addressed = comment.as_ref().and_then(|comment| {
            let text = comment.text.trim_start().to_lowercase();
            self.speakers
                .iter()
                .enumerate()
                .filter(|(_, name)| {
                    text.strip_prefix(&format!("@{}", name.to_lowercase()))
                        .is_some_and(|rest| {
                            rest.is_empty()
                                || rest.starts_with(char::is_whitespace)
                                || rest.starts_with(':')
                        })
                })
                .max_by_key(|(_, name)| name.len())
                .map(|(index, _)| index)
        });
        let index = addressed.unwrap_or(self.next_speaker);
        let phase = (self.format == Format::Munazara && self.rotate_ms > 0)
            .then(|| Phase::at(now_ms.saturating_sub(started) as f32 / self.rotate_ms as f32));
        let cue = if self.turns == 0 {
            Some(Cue::ShowOpening)
        } else if !self.introduced[index] {
            Some(Cue::Introduce)
        } else if new_topic {
            Some(Cue::TopicIntro)
        } else if comment.is_none() && self.since_chat >= CHAT_EVERY {
            Some(Cue::ChatCheckIn)
        } else {
            None
        };
        self.turns += 1;
        self.introduced[index] = true;
        if comment.is_some() || cue.is_some() {
            self.since_chat = 0;
        } else {
            self.since_chat += 1;
        }
        let crossfire = self.format == Format::Crossfire
            || phase == Some(Phase::Cross)
            || now_ms < self.crossfire_until;
        let brief = TurnBrief {
            speaker: self.speakers[index].clone(),
            topic: self.topics[self.current].clone(),
            audience: comment,
            max_seconds: if crossfire { 12 } else { 30 },
            heat: if crossfire { 3 } else { 1 },
            topic_number: self.current as u16 + 1,
            topic_total: self.topics.len() as u16,
            new_topic,
            topic_remaining_ms: if self.rotate_ms == 0 {
                0
            } else {
                (started + self.rotate_ms).saturating_sub(now_ms)
            },
            phase,
            cue,
        };
        self.next_speaker = 1 - index;
        Some(brief)
    }
    /// Moves to the next topic now (operator "skip topic"); the new topic starts its timer
    /// on its first turn.
    pub fn advance_topic(&mut self) {
        if self.end_after_last && self.current + 1 >= self.topics.len() {
            self.finished = true;
            return;
        }
        self.current = (self.current + 1) % self.topics.len();
        self.topic_started = None;
    }
    /// A broadcast session: every question once, then [`Director::finished`].
    pub fn end_after_last(&mut self, on: bool) {
        self.end_after_last = on;
    }
    pub fn finished(&self) -> bool {
        self.finished
    }
    /// Starts a new session with a fresh question list.
    pub fn set_topics(&mut self, topics: Vec<String>, rotate_ms: u64) -> Result<(), FloorError> {
        if topics.is_empty()
            || topics.len() > MAX_TOPICS
            || topics.iter().any(|t| t.trim().is_empty() || t.len() > 1000)
        {
            return Err(FloorError::InvalidInput);
        }
        self.topics = topics;
        self.rotate_ms = rotate_ms;
        self.current = 0;
        self.topic_started = None;
        self.finished = false;
        Ok(())
    }
    pub fn topic_count(&self) -> usize {
        self.topics.len()
    }
    pub fn rotate_ms(&self) -> u64 {
        self.rotate_ms
    }
    pub fn topic(&self) -> &str {
        &self.topics[self.current]
    }
    pub fn pause(&mut self) {
        self.paused = true;
    }
    pub fn resume(&mut self) {
        self.paused = false;
    }
}
