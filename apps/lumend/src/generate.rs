//! `lumend generate-pack`: offline persona authoring via `codex exec`. Never runs during a show.
use lumen_personagen::{
    Brief, CodexExec, GenerateError, Options, SourceError, generate, write_pack,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const USAGE: &str = "usage: lumend generate-pack BRIEF.toml OUT.toml | BRIEF_DIR OUT_DIR \
[--force] [--model MODEL] [--attempts 1..5] [--timeout-secs 30..1800] [--print-prompt]";

struct Args {
    input: PathBuf,
    output: PathBuf,
    force: bool,
    model: Option<String>,
    attempts: u32,
    timeout: Duration,
    print_prompt: bool,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut positional = Vec::new();
    let (mut force, mut model, mut attempts, mut timeout, mut print_prompt) =
        (false, None, 3, 600, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or(format!("{name} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--force" => force = true,
            "--print-prompt" => print_prompt = true,
            "--model" => model = Some(value("--model")?),
            "--attempts" => {
                attempts = value("--attempts")?
                    .parse()
                    .map_err(|_| "invalid --attempts")?
            }
            "--timeout-secs" => {
                timeout = value("--timeout-secs")?
                    .parse()
                    .map_err(|_| "invalid --timeout-secs")?
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            path => positional.push(PathBuf::from(path)),
        }
    }
    if !(1..=5).contains(&attempts) || !(30..=1800).contains(&timeout) {
        return Err(USAGE.into());
    }
    let [input, output]: [PathBuf; 2] = positional.try_into().map_err(|_| USAGE.to_string())?;
    Ok(Args {
        input,
        output,
        force,
        model,
        attempts,
        timeout: Duration::from_secs(timeout),
        print_prompt,
    })
}

fn read_brief(path: &Path) -> Result<Brief, String> {
    let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Brief::parse(&source).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = parse(raw)?;
    let jobs: Vec<(PathBuf, PathBuf)> = if args.input.is_dir() {
        if !args.output.is_dir() {
            return Err(format!(
                "{} must be an existing directory",
                args.output.display()
            ));
        }
        let mut briefs: Vec<_> = std::fs::read_dir(&args.input)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        briefs.sort();
        briefs
            .into_iter()
            .map(|b| {
                let out = args
                    .output
                    .join(b.file_name().expect("read_dir entry has a name"));
                (b, out)
            })
            .collect()
    } else {
        vec![(args.input.clone(), args.output.clone())]
    };
    // Validate every brief before spending any drafting call.
    let briefs = jobs
        .iter()
        .map(|(brief, _)| read_brief(brief))
        .collect::<Result<Vec<_>, _>>()?;
    if args.print_prompt {
        for brief in &briefs {
            println!("{}", lumen_personagen::prompt::build(brief, &[]));
        }
        return Ok(());
    }
    let codex = CodexExec::from_env(args.model.clone(), args.timeout);
    let (mut written, mut skipped, mut failed) = (0, 0, Vec::new());
    for ((brief_path, out), brief) in jobs.iter().zip(&briefs) {
        if out.exists() && !args.force {
            eprintln!("skip {}: exists (use --force to regenerate)", out.display());
            skipped += 1;
            continue;
        }
        eprintln!("generate {} -> {}", brief_path.display(), out.display());
        let origin = brief_path.display().to_string();
        let progress = |attempt: u32, outcome: &str| eprintln!("  attempt {attempt}: {outcome}");
        let options = Options {
            attempts: args.attempts,
            origin: &origin,
            progress: &progress,
        };
        match generate(brief, &codex, &options) {
            Ok(generated) => {
                write_pack(out, &generated.toml, args.force)?;
                let [a, b] = &generated.pack.agents;
                println!(
                    "{}: {} / {} ({} attempt(s))",
                    out.display(),
                    a.name,
                    b.name,
                    generated.attempts
                );
                written += 1;
            }
            Err(GenerateError::Source(SourceError::Fatal(e))) => {
                return Err(format!(
                    "stopping: {e}\n(written {written}, skipped {skipped})"
                ));
            }
            Err(e) => {
                eprintln!("  failed: {e}");
                failed.push(brief_path.display().to_string());
            }
        }
    }
    eprintln!(
        "written {written}, skipped {skipped}, failed {}",
        failed.len()
    );
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("failed briefs: {}", failed.join(", ")))
    }
}

const SKILLS_USAGE: &str = "usage: lumend generate-skills PACK.toml | PERSONAS_DIR \
[--force] [--model MODEL] [--attempts 1..5] [--timeout-secs 30..1800]";

/// Drafts `packs/skills/<pack>/<agent>.md` for each pack via `codex exec`.
pub fn run_skills(raw: &[String]) -> Result<(), String> {
    let mut with_output = raw.to_vec();
    // Reuse the flag parser; the output position is unused for skills.
    let positional = raw.iter().filter(|a| !a.starts_with("--")).count();
    if positional != 1 {
        return Err(SKILLS_USAGE.into());
    }
    let first = raw
        .iter()
        .position(|a| !a.starts_with("--"))
        .ok_or(SKILLS_USAGE)?;
    with_output.insert(first + 1, ".".into());
    let args = parse(&with_output).map_err(|_| SKILLS_USAGE.to_string())?;
    let packs: Vec<PathBuf> = if args.input.is_dir() {
        let mut packs: Vec<_> = std::fs::read_dir(&args.input)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        packs.sort();
        packs
    } else {
        vec![args.input.clone()]
    };
    let loaded = packs
        .iter()
        .map(|path| crate::load_pack(path).map_err(|e| format!("{}: {e}", path.display())))
        .collect::<Result<Vec<_>, _>>()?;
    let codex = CodexExec::from_env(args.model.clone(), args.timeout);
    let (mut written, mut skipped, mut failed) = (0, 0, Vec::new());
    for (path, pack) in packs.iter().zip(&loaded) {
        let targets = pack
            .agents
            .each_ref()
            .map(|agent| lumen_personagen::skill::skill_path(path, &agent.name));
        if !args.force && targets.iter().all(|t| t.exists()) {
            eprintln!("skip {}: skill sheets exist (use --force)", path.display());
            skipped += 1;
            continue;
        }
        eprintln!(
            "skills {} -> {}",
            path.display(),
            targets[0].parent().unwrap_or(Path::new(".")).display()
        );
        let origin = path.display().to_string();
        let progress = |attempt: u32, outcome: &str| eprintln!("  attempt {attempt}: {outcome}");
        let options = Options {
            attempts: args.attempts,
            origin: &origin,
            progress: &progress,
        };
        let mut shared = String::new();
        for name in &pack.skills {
            let file = lumen_personagen::skill::shared_path(path, name);
            shared.push_str(
                &std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?,
            );
            shared.push('\n');
        }
        // A failed editor review of the previous version becomes revision instructions.
        let stem = path
            .file_stem()
            .map_or_else(|| "pack".into(), |s| s.to_string_lossy().into_owned());
        let review_file = path
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."))
            .join("reviews")
            .join(format!("{stem}.json"));
        let feedback: Vec<String> = std::fs::read_to_string(&review_file)
            .ok()
            .and_then(|text| serde_json::from_str::<lumen_personagen::review::Review>(&text).ok())
            .filter(|review| !review.passed())
            .map(|review| review.issues)
            .unwrap_or_default();
        if !feedback.is_empty() {
            eprintln!(
                "  using {} editor review issue(s) from {}",
                feedback.len(),
                review_file.display()
            );
        }
        match lumen_personagen::skill::generate_skills(pack, &shared, &feedback, &codex, &options) {
            Ok((sheets, attempts)) => {
                for (target, sheet) in targets.iter().zip(&sheets) {
                    if let Some(dir) = target.parent() {
                        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    }
                    write_pack(target, sheet, true)?;
                    println!("{}", target.display());
                }
                eprintln!("  {attempts} attempt(s)");
                written += 1;
            }
            Err(GenerateError::Source(SourceError::Fatal(e))) => {
                return Err(format!(
                    "stopping: {e}\n(written {written}, skipped {skipped})"
                ));
            }
            Err(e) => {
                eprintln!("  failed: {e}");
                failed.push(path.display().to_string());
            }
        }
    }
    eprintln!(
        "packs written {written}, skipped {skipped}, failed {}",
        failed.len()
    );
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("failed packs: {}", failed.join(", ")))
    }
}

const AVATARS_USAGE: &str =
    "usage: lumend generate-avatars AVATARS_DIR | AVATAR_DIR [--force] [--timeout-secs 30..1800]";

/// Draws stage avatar frames (`idle.png`, `talk.png`, `blink.png`) via `codex exec`.
pub fn run_avatars(raw: &[String]) -> Result<(), String> {
    let (mut input, mut force, mut timeout) = (None, false, 600u64);
    let mut it = raw.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--force" => force = true,
            "--timeout-secs" => {
                timeout = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|v| (30..=1800).contains(v))
                    .ok_or(AVATARS_USAGE)?
            }
            path if !path.starts_with("--") && input.is_none() => input = Some(PathBuf::from(path)),
            _ => return Err(AVATARS_USAGE.into()),
        }
    }
    let input = input.ok_or(AVATARS_USAGE)?;
    let dirs: Vec<PathBuf> = if input.join("avatar.toml").is_file() {
        vec![input.clone()]
    } else {
        let mut dirs: Vec<_> = std::fs::read_dir(&input)
            .map_err(|e| format!("{}: {e}", input.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join("avatar.toml").is_file())
            .collect();
        dirs.sort();
        dirs
    };
    if dirs.is_empty() {
        return Err(format!("no avatar.toml under {}", input.display()));
    }
    let program = std::env::var_os("LUMEN_CODEX_BIN").map_or_else(|| if cfg!(windows) { "codex.cmd" } else { "codex" }.into(), PathBuf::from);
    let mut failed = Vec::new();
    for dir in &dirs {
        eprintln!("avatar {}", dir.display());
        let progress = |step: &str| eprintln!("  {step}");
        match lumen_personagen::avatar::generate_frames(
            dir,
            &program,
            force,
            Duration::from_secs(timeout),
            &progress,
        ) {
            Ok(written) => written.iter().for_each(|p| println!("{}", p.display())),
            Err(e) if e.contains("usage limit") => return Err(format!("stopping: {e}")),
            Err(e) => {
                eprintln!("  failed: {e}");
                failed.push(dir.display().to_string());
            }
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("failed avatars: {}", failed.join(", ")))
    }
}

const REVIEW_USAGE: &str = "usage: lumend review-personas PACK.toml | PERSONAS_DIR [--model MODEL] [--timeout-secs 30..1800]";

/// Quality gate: codex reviews each pack + character sheets; writes `packs/reviews/<pack>.{md,json}`.
pub fn run_review(raw: &[String]) -> Result<(), String> {
    let mut with_output = raw.to_vec();
    let first = raw
        .iter()
        .position(|a| !a.starts_with("--"))
        .ok_or(REVIEW_USAGE)?;
    with_output.insert(first + 1, ".".into());
    let args = parse(&with_output).map_err(|_| REVIEW_USAGE.to_string())?;
    let packs: Vec<PathBuf> = if args.input.is_dir() {
        let mut packs: Vec<_> = std::fs::read_dir(&args.input)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        packs.sort();
        packs
    } else {
        vec![args.input.clone()]
    };
    let codex = CodexExec::from_env(args.model.clone(), args.timeout);
    let mut failed = Vec::new();
    for path in &packs {
        let pack = crate::load_pack(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let sheets = [0, 1].map(|i| {
            lumen_personagen::skill::load(&lumen_personagen::skill::skill_path(
                path,
                &pack.agents[i].name,
            ))
        });
        let sheets = [sheets[0].clone()?, sheets[1].clone()?];
        eprintln!("review {}", path.display());
        let origin = path.display().to_string();
        let progress = |attempt: u32, outcome: &str| eprintln!("  attempt {attempt}: {outcome}");
        let options = Options {
            attempts: args.attempts,
            origin: &origin,
            progress: &progress,
        };
        let review = match lumen_personagen::review::review(&pack, &sheets, &codex, &options) {
            Ok(review) => review,
            Err(GenerateError::Source(SourceError::Fatal(e))) => {
                return Err(format!("stopping: {e}"));
            }
            Err(e) => {
                eprintln!("  failed: {e}");
                failed.push(origin);
                continue;
            }
        };
        let stem = path
            .file_stem()
            .map_or_else(|| "pack".into(), |s| s.to_string_lossy().into_owned());
        let dir = path
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."))
            .join("reviews");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        write_pack(
            &dir.join(format!("{stem}.md")),
            &review.markdown(&pack),
            true,
        )?;
        write_pack(
            &dir.join(format!("{stem}.json")),
            &serde_json::to_string_pretty(&review).map_err(|e| e.to_string())?,
            true,
        )?;
        println!(
            "{stem}: {} (anlaşılırlık {} · ayrışma {} · ikna {} · güvenlik {} · uygunluk {})",
            if review.passed() {
                "GEÇTİ"
            } else {
                "DÜZELTİLMELİ"
            },
            review.clarity,
            review.distinctiveness,
            review.persuasion,
            review.safety,
            review.fit
        );
        if !review.passed() {
            failed.push(origin);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("packs not broadcast ready: {}", failed.join(", ")))
    }
}

const REFINE_USAGE: &str =
    "usage: lumend refine-personas PACK.toml | PERSONAS_DIR [--rounds 1..5] [--model MODEL]";

/// Review -> regenerate skills with the editor's issues -> review again, until every pack passes
/// the gate or the round budget is spent.
pub fn run_refine(raw: &[String]) -> Result<(), String> {
    let (mut input, mut rounds, mut extra) = (None, 3u32, Vec::new());
    let mut it = raw.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--rounds" => {
                rounds = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|v| (1..=5).contains(v))
                    .ok_or(REFINE_USAGE)?
            }
            "--model" => {
                extra.extend(["--model".to_owned(), it.next().ok_or(REFINE_USAGE)?.clone()])
            }
            path if !path.starts_with("--") && input.is_none() => input = Some(PathBuf::from(path)),
            _ => return Err(REFINE_USAGE.into()),
        }
    }
    let input = input.ok_or(REFINE_USAGE)?;
    let packs: Vec<PathBuf> = if input.is_dir() {
        let mut packs: Vec<_> = std::fs::read_dir(&input)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        packs.sort();
        packs
    } else {
        vec![input]
    };
    let mut failed = Vec::new();
    for pack in &packs {
        let target = pack.display().to_string();
        let with = |mut args: Vec<String>| {
            args.extend(extra.iter().cloned());
            args
        };
        let files = refine_files(pack)?;
        let keep = tempfile::tempdir().map_err(|e| e.to_string())?;
        let mut best: Option<u32> = None;
        let mut passed = false;
        for round in 0..=rounds {
            let outcome = run_review(&with(vec![target.clone()]));
            if let Err(e) = &outcome
                && e.starts_with("stopping")
            {
                return Err(e.clone());
            }
            // Remember the best version seen (safety must be perfect to qualify).
            if let Some(score) = review_score(&files[2])
                && best.is_none_or(|b| score > b)
            {
                best = Some(score);
                copy_all(&files, keep.path(), true)?;
            }
            if outcome.is_ok() {
                passed = true;
                break;
            }
            if round < rounds {
                eprintln!("refine {target}: round {}/{rounds}", round + 1);
                run_skills(&with(vec![target.clone(), "--force".into()]))?;
            }
        }
        if !passed {
            if best.is_some() {
                copy_all(&files, keep.path(), false)?;
                eprintln!("refine {target}: kept the best-scoring version");
            }
            failed.push(target);
        }
    }
    if failed.is_empty() {
        eprintln!("all {} pack(s) pass the review gate", packs.len());
        Ok(())
    } else {
        Err(format!(
            "did not reach the bar after {rounds} round(s): {}",
            failed.join(", ")
        ))
    }
}

/// Character sheets and review report touched by one refine run: [sheet a, sheet b, review json, review md].
fn refine_files(pack_path: &Path) -> Result<[PathBuf; 4], String> {
    let pack = crate::load_pack(pack_path)?;
    let stem = pack_path
        .file_stem()
        .map_or_else(|| "pack".into(), |s| s.to_string_lossy().into_owned());
    let reviews = pack_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("."))
        .join("reviews");
    Ok([
        lumen_personagen::skill::skill_path(pack_path, &pack.agents[0].name),
        lumen_personagen::skill::skill_path(pack_path, &pack.agents[1].name),
        reviews.join(format!("{stem}.json")),
        reviews.join(format!("{stem}.md")),
    ])
}

/// Sum of the non-safety scores, only for reviews with perfect safety.
fn review_score(json: &Path) -> Option<u32> {
    let review: lumen_personagen::review::Review =
        serde_json::from_str(&std::fs::read_to_string(json).ok()?).ok()?;
    (review.safety == 5).then(|| {
        [
            review.clarity,
            review.distinctiveness,
            review.persuasion,
            review.fit,
        ]
        .iter()
        .map(|s| u32::from(*s))
        .sum()
    })
}

/// Copies the refine files into (`save`) or back out of (`!save`) the keep directory.
fn copy_all(files: &[PathBuf; 4], keep: &Path, save: bool) -> Result<(), String> {
    for (i, file) in files.iter().enumerate() {
        let kept = keep.join(i.to_string());
        let (from, to) = if save {
            (file.as_path(), kept.as_path())
        } else {
            (kept.as_path(), file.as_path())
        };
        if from.is_file() {
            std::fs::copy(from, to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    Ok(())
}
