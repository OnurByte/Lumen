//! Scenario runner; generated text and playback acknowledgments are explicitly simulated.
use lumen_audience::{AudienceQueue, Comment};
use lumen_debate::{
    Session,
    director::{Director, Format},
};
use lumen_storage::Journal;
use std::path::Path;
mod audience;
mod control;
mod generate;
mod gpt;
mod overlay;
mod rehearse;
mod show;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 3 && args[0] == "replay-chat" {
        return audience::replay(Path::new(&args[1]), Path::new(&args[2]));
    }
    if args.len() == 3 && args[0] == "control" {
        use lumen_protocol::Command;
        let command = match args[2].as_str() {
            "status" => Command::Status,
            "pause" => Command::Pause,
            "resume" => Command::Resume,
            "mute" => Command::MuteAll,
            "stop" => Command::EmergencyStop,
            _ => return Err("unknown control command".into()),
        };
        let response = lumen_protocol::request(
            Path::new(&args[1]),
            command,
            std::time::Duration::from_secs(3),
        )
        .map_err(|e| format!("control socket: {e}"))?;
        println!(
            "paused={} muted={} stopped={}",
            response.paused, response.muted, response.stopped
        );
        return response.error.map_or(Ok(()), Err);
    }
    if (args.len() == 3 || args.len() == 4) && args[0] == "serve" {
        let show = args
            .get(3)
            .map(|path| {
                let path = Path::new(path);
                let pack = load_pack(path)?;
                let skills = rehearse::load_skills(path, &pack)?;
                Ok::<_, String>(show::Show {
                    pack,
                    skills,
                    mode: show::Mode::Live,
                })
            })
            .transpose()?;
        return control::serve(Path::new(&args[1]), Path::new(&args[2]), show).await;
    }
    if args.first().is_some_and(|a| a == "rehearse") {
        return rehearse::run(&args[1..]).await;
    }
    if args.first().is_some_and(|a| a == "refine-personas") {
        return generate::run_refine(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "review-personas") {
        return generate::run_review(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "generate-avatars") {
        return generate::run_avatars(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "generate-skills") {
        return generate::run_skills(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "generate-pack") {
        return generate::run(&args[1..]);
    }
    if args.len() == 2 && args[0] == "validate-pack" {
        let pack = load_pack(Path::new(&args[1]))?;
        println!(
            "{}: {} / {}",
            pack.name, pack.agents[0].name, pack.agents[1].name
        );
        return Ok(());
    }
    if args.len() != 3 || args[0] != "simulate" {
        return Err("usage: lumend simulate JOURNAL_PATH SECONDS (15..86400)".into());
    }
    let seconds: u64 = args[2].parse().map_err(|_| "invalid duration")?;
    if !(15..=86400).contains(&seconds) {
        return Err("duration must be 15..86400 seconds".into());
    }
    let journal = Journal::open(Path::new(&args[1]))
        .await
        .map_err(|e| e.to_string())?;
    let session_id = format!(
        "simulation-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let mut session = Session::new(&session_id, journal).map_err(|e| format!("{e:?}"))?;
    let mut director = Director::new(
        ["Ada".into(), "Deniz".into()],
        "Tarih nasıl yorumlanmalı?",
        Format::FreeDebate,
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut audience = AudienceQueue::default();
    let mut turns = 0;
    let mut stale = 0;
    for second in (0..seconds).step_by(15) {
        let now = second * 1000;
        if turns % 3 == 0 {
            audience
                .push(
                    Comment {
                        id: turns.to_string(),
                        user: format!("viewer-{}", turns % 7),
                        text: "Bu görüşün dayanağı nedir?".into(),
                        received_ms: now,
                    },
                    now,
                )
                .map_err(|e| format!("{e:?}"))?;
        }
        let brief = director
            .next(&mut audience, now)
            .ok_or("director paused unexpectedly")?;
        let turn = session
            .grant(&brief.speaker)
            .map_err(|e| format!("{e:?}"))?;
        session
            .queue(turn, 0, &format!("{}: Simülasyon yanıtı.", brief.speaker))
            .map_err(|e| format!("{e:?}"))?;
        session
            .queue(turn, 1, "Oynatılmayan simülasyon bölümü.")
            .map_err(|e| format!("{e:?}"))?;
        session
            .played(turn, 0, now + 100)
            .await
            .map_err(|e| format!("{e:?}"))?;
        session
            .cancel(now + 200)
            .await
            .map_err(|e| format!("{e:?}"))?;
        if turns % 5 == 0 {
            if session.played(turn, 1, now + 300).await.is_ok() {
                return Err("stale audio accepted".into());
            }
            stale += 1;
        }
        turns += 1;
    }
    let journal = Journal::open(Path::new(&args[1]))
        .await
        .map_err(|e| e.to_string())?;
    let audible = journal
        .audible(&session_id)
        .await
        .map_err(|e| e.to_string())?;
    if audible.len() != turns || audible.iter().any(|text| text.contains("Oynatılmayan")) {
        return Err("audibility journal invariant failed".into());
    }
    println!(
        "SIMULATION ONLY session={session_id} simulated_seconds={seconds} turns={turns} stale_ack_rejected={stale} audible_segments={}",
        audible.len()
    );
    Ok(())
}

#[cfg(test)]
mod show_tests;

fn load_pack(path: &Path) -> Result<lumen_debate::pack::ShowPack, String> {
    use std::io::Read;
    let mut source = String::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(65537)
        .read_to_string(&mut source)
        .map_err(|e| e.to_string())?;
    lumen_debate::pack::ShowPack::parse(&source)
}
