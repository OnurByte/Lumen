//! Isolated, persistent TTS subprocess. Cancellation kills and reaps it; the next request starts fresh.
use crate::PcmFrame;
use std::{future::Future, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::mpsc::Sender,
};
use tokio_util::sync::CancellationToken;

pub const VOICES: [&str; 6] = [
    "female_1", "female_2", "female_3", "male_1", "male_2", "male_3",
];
pub struct VoiceRequest {
    pub generation: u64,
    pub utterance: u64,
    pub text: String,
    pub voice: String,
}
pub struct VoiceCapabilities {
    pub streaming: bool,
    pub cancellation: bool,
    pub sample_rate: u32,
    pub channels: u16,
}
pub struct VoiceHealth {
    pub process_alive: bool,
    pub model_produced_audio: bool,
}

pub trait VoiceProvider {
    fn synthesize_stream(
        &mut self,
        request: VoiceRequest,
        output: Sender<PcmFrame>,
        cancel: CancellationToken,
    ) -> impl Future<Output = Result<(), String>> + Send;
    fn cancel(&mut self) -> impl Future<Output = Result<(), String>> + Send;
    fn list_voices(&self) -> &[&str];
    fn capabilities(&self) -> VoiceCapabilities;
    fn health_check(&mut self) -> Result<VoiceHealth, String>;
}

struct Process {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}
pub struct WorkerVoice {
    python: PathBuf,
    script: PathBuf,
    process: Option<Process>,
    produced: bool,
}

impl WorkerVoice {
    pub fn new(python: impl Into<PathBuf>, script: impl Into<PathBuf>) -> Self {
        Self {
            python: python.into(),
            script: script.into(),
            process: None,
            produced: false,
        }
    }
    fn start(&mut self) -> Result<(), String> {
        if self.process.is_none() {
            let mut child = Command::new(&self.python)
                .arg(&self.script)
                .kill_on_drop(true)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| format!("start voice worker: {e}"))?;
            let input = child.stdin.take().ok_or("worker stdin unavailable")?;
            let output = child.stdout.take().ok_or("worker stdout unavailable")?;
            self.process = Some(Process {
                child,
                input,
                output,
            });
            self.produced = false;
        }
        Ok(())
    }
    async fn generate(
        &mut self,
        request: VoiceRequest,
        output: Sender<PcmFrame>,
    ) -> Result<(), String> {
        if request.text.trim().is_empty()
            || request.text.len() > 4096
            || !VOICES.contains(&request.voice.as_str())
        {
            return Err("invalid voice request".into());
        }
        self.start()?;
        let process = self.process.as_mut().ok_or("worker unavailable")?;
        let mut request_bytes = serde_json::to_vec(&serde_json::json!({"generation":request.generation,"utterance":request.utterance,"text":request.text,"voice":request.voice})).map_err(|e| e.to_string())?;
        request_bytes.push(b'\n');
        if request_bytes.len() > 8192 {
            return Err("voice request exceeds wire limit".into());
        }
        process
            .input
            .write_all(&request_bytes)
            .await
            .map_err(|e| e.to_string())?;
        let mut sequence = 0u32;
        let mut total = 0usize;
        loop {
            let mut header = [0u8; 35];
            process
                .output
                .read_exact(&mut header)
                .await
                .map_err(|e| format!("voice frame header: {e}"))?;
            let u64_at = |offset| {
                u64::from_le_bytes(header[offset..offset + 8].try_into().expect("fixed header"))
            };
            let u32_at = |offset| {
                u32::from_le_bytes(header[offset..offset + 4].try_into().expect("fixed header"))
            };
            let count = u32_at(24) as usize;
            if &header[..4] != b"PCM1"
                || u64_at(4) != request.generation
                || u64_at(12) != request.utterance
                || u32_at(20) != sequence
                || !(1..=24000).contains(&count)
                || u32_at(28) != 24000
                || u16::from_le_bytes([header[32], header[33]]) != 1
                || header[34] > 1
            {
                return Err("invalid or stale voice frame".into());
            }
            total += count;
            if total > 24000 * 30 {
                return Err("voice utterance exceeds 30 seconds".into());
            }
            let mut bytes = vec![0; count * 4];
            process
                .output
                .read_exact(&mut bytes)
                .await
                .map_err(|e| format!("voice PCM: {e}"))?;
            let samples: Vec<f32> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            if samples
                .iter()
                .any(|sample| !sample.is_finite() || sample.abs() > 1.)
            {
                return Err("invalid voice sample".into());
            }
            self.produced = true;
            let last = header[34] == 1;
            output
                .send(PcmFrame {
                    generation: request.generation,
                    utterance: request.utterance,
                    sequence: sequence as u64,
                    sample_rate: 24000,
                    channels: 1,
                    last,
                    samples,
                })
                .await
                .map_err(|_| "audio consumer disconnected")?;
            if last {
                return Ok(());
            }
            sequence = sequence.checked_add(1).ok_or("voice sequence overflow")?;
        }
    }
}

impl VoiceProvider for WorkerVoice {
    async fn synthesize_stream(
        &mut self,
        request: VoiceRequest,
        output: Sender<PcmFrame>,
        cancel: CancellationToken,
    ) -> Result<(), String> {
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err("voice cancelled".into()),
            result = tokio::time::timeout(Duration::from_secs(120), self.generate(request, output)) => result.unwrap_or_else(|_| Err("voice deadline exceeded".into())),
        };
        if result.is_err() {
            self.cancel().await?;
        }
        result
    }
    async fn cancel(&mut self) -> Result<(), String> {
        self.produced = false;
        if let Some(mut process) = self.process.take() {
            if process
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_none()
            {
                process.child.kill().await.map_err(|e| e.to_string())?;
            }
            process.child.wait().await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn list_voices(&self) -> &[&str] {
        &VOICES
    }
    fn capabilities(&self) -> VoiceCapabilities {
        VoiceCapabilities {
            streaming: true,
            cancellation: true,
            sample_rate: 24000,
            channels: 1,
        }
    }
    fn health_check(&mut self) -> Result<VoiceHealth, String> {
        let alive = match self.process.as_mut() {
            Some(process) => process
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_none(),
            None => false,
        };
        Ok(VoiceHealth {
            process_alive: alive,
            model_produced_audio: alive && self.produced,
        })
    }
}
