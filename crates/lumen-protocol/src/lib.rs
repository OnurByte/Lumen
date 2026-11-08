//! Versioned, bounded local IPC. Socket owners must enforce local permissions and timeouts.
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const MAX_FRAME_BYTES: usize = 65536;
pub const PROTOCOL_VERSION: u16 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    Pause,
    Resume,
    MuteAll,
    SkipTurn,
    SkipTopic,
    StopStream,
    EmergencyStop,
    Status,
    Audience(AudienceInput),
    /// Replace the session's question list; each question runs `minutes`.
    SetTopics(TopicList),
}

pub const MAX_TOPICS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopicList {
    pub topics: Vec<String>,
    pub minutes: u16,
}

impl TopicList {
    /// Non-empty, bounded questions and 1..=180 minutes each.
    pub fn valid(&self) -> bool {
        !self.topics.is_empty()
            && self.topics.len() <= MAX_TOPICS
            && self.topics.iter().all(|t| {
                !t.trim().is_empty() && t.len() <= 1000 && !t.chars().any(char::is_control)
            })
            && (1..=180).contains(&self.minutes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudienceInput {
    pub id: String,
    pub user: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u16,
    pub command: Command,
}

impl Envelope {
    pub fn command(command: Command) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            command,
        }
    }
    fn validate(&self) -> io::Result<()> {
        if self.version != PROTOCOL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported protocol version",
            ));
        }
        if let Command::Audience(input) = &self.command
            && (input.id.is_empty()
                || input.id.len() > 128
                || input.user.trim().is_empty()
                || input.user.len() > 128
                || input.text.trim().is_empty()
                || input.text.len() > 1000
                || input
                    .text
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid audience data",
            ));
        }
        Ok(())
    }
}

pub fn read_frame(reader: &mut impl Read) -> io::Result<Envelope> {
    let message: Envelope = read_message(reader)?;
    message.validate()?;
    Ok(message)
}

fn read_message<T: serde::de::DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame length",
        ));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    let mut cursor = io::Cursor::new(payload);
    let message = T::deserialize(&mut rmp_serde::Deserializer::new(&mut cursor))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if cursor.position() != length as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "trailing frame data",
        ));
    }
    Ok(message)
}

pub fn write_frame(writer: &mut impl Write, message: &Envelope) -> io::Result<()> {
    message.validate()?;
    write_message(writer, message)
}

fn write_message(writer: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let payload = rmp_serde::to_vec_named(message)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(&payload)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageState {
    pub agents: [String; 2],
    pub topic: String,
    pub speaker: Option<String>,
    pub caption: String,
    pub question: Option<String>,
    pub level: u8,
    /// Topic rotation: 1-based index, total, and seconds until the next topic (0 = no timer).
    /// Short side labels shown under each host.
    #[serde(default)]
    pub sides: [String; 2],
    /// Avatar presets for the two hosts.
    #[serde(default)]
    pub avatars: [String; 2],
    /// Host preparing the next turn (shown as "thinking" until speech starts).
    #[serde(default)]
    pub thinking: Option<String>,
    /// Debate stage label (e.g. "CROSS-EXAMINATION"), empty when not applicable.
    #[serde(default)]
    pub phase: String,
    /// Show (concept) name, as the operator named it.
    #[serde(default)]
    pub show: String,
    /// Broadcast session clock for the operator (never drawn on air).
    #[serde(default)]
    pub session_elapsed_s: u32,
    #[serde(default)]
    pub session_total_s: u32,
    /// Every question has been debated; the runtime paused itself.
    #[serde(default)]
    pub finished: bool,
    #[serde(default)]
    pub topic_number: u16,
    #[serde(default)]
    pub topic_total: u16,
    #[serde(default)]
    pub topic_remaining_s: u32,
}
impl StageState {
    fn validate(&self) -> io::Result<()> {
        if self.agents.iter().any(|name| name.len() > 128)
            || self.sides.iter().any(|side| side.len() > 128)
            || self.phase.len() > 64
            || self.show.len() > 128
            || self.avatars.iter().any(|a| a.len() > 32)
            || self.thinking.as_ref().is_some_and(|t| t.len() > 128)
            || self.topic.len() > 1000
            || self.caption.len() > 512
            || self.level > 100
            || self.speaker.as_ref().is_some_and(|name| name.len() > 128)
            || self
                .question
                .as_ref()
                .is_some_and(|question| question.len() > 1000)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid stage snapshot",
            ));
        }
        Ok(())
    }
    pub fn clear_speech(&mut self) {
        self.thinking = None;
        self.speaker = None;
        self.caption.clear();
        self.level = 0;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u16,
    pub paused: bool,
    pub muted: bool,
    pub stopped: bool,
    pub error: Option<String>,
    pub stage: StageState,
}

pub fn read_response(reader: &mut impl Read) -> io::Result<Response> {
    let response: Response = read_message(reader)?;
    if response.version != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported response version",
        ));
    }
    response.stage.validate()?;
    Ok(response)
}

pub fn write_response(writer: &mut impl Write, response: &Response) -> io::Result<()> {
    if response.version != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported response version",
        ));
    }
    response.stage.validate()?;
    write_message(writer, response)
}

/// One request per connection; clients can detach without affecting the runtime.
pub fn request(
    path: &std::path::Path,
    command: Command,
    timeout: std::time::Duration,
) -> io::Result<Response> {
    let mut stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write_frame(&mut stream, &Envelope::command(command))?;
    read_response(&mut stream)
}
