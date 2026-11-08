//! Hardware smoke check: queues 100 ms of silence and requires a device-clock completion.
use lumen_voice::{PcmFrame, output::AudioOutput};
use std::time::{Duration, Instant};
fn main() -> Result<(), String> {
    let device = std::env::var("LUMEN_AUDIO_DEVICE").ok();
    let output = AudioOutput::open(device.as_deref())?;
    output.begin(1, 1)?;
    output.push(PcmFrame {
        generation: 1,
        utterance: 1,
        sequence: 0,
        sample_rate: 24000,
        channels: 1,
        last: true,
        samples: vec![0.; 2400],
    })?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(event) = output.try_event() {
            if event? != (1, 1) {
                return Err("unexpected playback completion".into());
            }
            println!("CPAL silent playback completed after device-clock fence");
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("no device-clock completion within 3 seconds".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
