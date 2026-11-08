//! Hardware benchmark, no playback. Emits measurements as JSON; no quality judgment.
use lumen_voice::worker::{VoiceProvider, VoiceRequest, WorkerVoice};
use std::{path::PathBuf, time::Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
#[tokio::main]
async fn main() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../workers/tts-python");
    let mut voice = WorkerVoice::new(root.join(".venv/bin/python"), root.join("worker.py"));
    let mut results = Vec::new();
    let cases = [
        ("female_2", "Merhaba. Bugün tarihi tartışıyoruz."),
        (
            "male_1",
            "Dr. Ayşe Yılmaz, 29 Ekim 1923 tarihini nasıl yorumluyor?",
        ),
        (
            "female_2",
            "Bu itirazı kabul ediyorum. Fakat kaynağını birlikte kontrol edelim.",
        ),
        ("male_1", "Bir dakika, bu iddianın dayanağı nedir?"),
    ];
    for (index, (preset, text)) in cases.into_iter().enumerate() {
        let (tx, mut rx) = mpsc::channel(8);
        let started = Instant::now();
        let request = VoiceRequest {
            generation: 1,
            utterance: index as u64,
            text: text.into(),
            voice: preset.into(),
        };
        let producer = voice.synthesize_stream(request, tx, CancellationToken::new());
        let consumer = async {
            let mut first_ms = None;
            let mut samples = 0usize;
            let mut frames = 0usize;
            let mut final_seen = false;
            while let Some(frame) = rx.recv().await {
                first_ms.get_or_insert(started.elapsed().as_secs_f64() * 1000.);
                samples += frame.samples.len();
                frames += 1;
                final_seen |= frame.last;
            }
            (first_ms, samples, frames, final_seen)
        };
        let (result, (first, samples, frames, final_seen)) = tokio::join!(producer, consumer);
        result?;
        if first.is_none() || samples == 0 || !final_seen {
            return Err("incomplete benchmark audio".into());
        }
        results.push(serde_json::json!({"voice":preset,"text":text,"cold_process":index==0,"first_chunk_ms":first,"total_ms":started.elapsed().as_secs_f64()*1000.,"audio_seconds":samples as f64/24000.,"frames":frames}));
    }
    voice.cancel().await?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"model_revision":"fcbd37c74dc127be846face0ff36062e9a939f7b","device":"cpu","model_cache":"warm","sample_rate":24000,"measurements":results})).map_err(|e| e.to_string())?);
    Ok(())
}
