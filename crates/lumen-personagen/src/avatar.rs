//! Stage avatar portraits drawn by GPT through `codex exec` image generation.
//!
//! Each preset lives in `packs/avatars/<preset>/` with an `avatar.toml` (`look = "..."`).
//! Generation draws `idle.png` (mouth closed) in a fixed serious broadcast style, then edits
//! that same image into `talk.png` (mouth open) and `blink.png` (eyes closed) so the frames
//! stay consistent for mouth flaps and blinks. Codex runs with write access to that one
//! directory only.
use serde::Deserialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    pub look: String,
}

pub const FRAMES: [&str; 3] = ["idle.png", "talk.png", "blink.png"];

const STYLE: &str = "A serious, semi-realistic digital painting portrait used as a live \
political-debate broadcast avatar. Chest-up framing, centered, front facing, soft studio \
lighting, plain flat dark navy background (#0b1220). Painterly but realistic like a high-end \
editorial illustration. Not a cartoon, not anime, no text, no logo, no frame. Must not resemble \
any real, identifiable person. Square format.";

pub fn load_look(dir: &Path) -> Result<Look, String> {
    let path = dir.join("avatar.toml");
    let source = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let look: Look = toml::from_str(&source).map_err(|e| format!("{}: {e}", path.display()))?;
    let n = look.look.trim().chars().count();
    if !(20..=1500).contains(&n) {
        return Err(format!(
            "{}: look must be 20-1500 characters",
            path.display()
        ));
    }
    Ok(look)
}

/// Prompt for one frame. `None` draws the base portrait; `Some(edit)` edits the attached one.
pub fn frame_prompt(look: &Look, frame: &str) -> String {
    match frame {
        "idle.png" => format!(
            "Use your image generation tool to create one image, then save it as ./idle.png in \
             the current directory (copy the generated file there). Do nothing else.\n\n\
             Image: {STYLE}\nSubject: {} Mouth closed.",
            look.look.trim()
        ),
        "talk.png" => "Use your image generation tool to edit the attached image and save the \
             result as ./talk.png in the current directory. Keep everything identical: same \
             person, face, clothing, pose, framing, lighting, colours and background. Change only \
             the mouth: slightly open mid-sentence as if speaking, a little of the upper teeth \
             visible. Do nothing else."
            .to_owned(),
        _ => "Use your image generation tool to edit the attached image and save the result as \
             ./blink.png in the current directory. Keep everything identical: same person, face, \
             clothing, pose, framing, lighting, colours and background. Change only the eyes: \
             naturally closed, as in the middle of a blink. Do nothing else."
            .to_owned(),
    }
}

fn run_codex(
    program: &Path,
    dir: &Path,
    prompt: &str,
    image: Option<&Path>,
    timeout: Duration,
) -> Result<(), String> {
    let log_path = dir.join(".codex.log");
    let log = fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let mut command = Command::new(program);
    command
        .args([
            "exec",
            "--skip-git-repo-check",
            "--ephemeral",
            "--sandbox",
            "workspace-write",
        ])
        .args(["--color", "never", "-C"])
        .arg(dir);
    if let Some(image) = image {
        command.arg("--image").arg(image);
    }
    let mut child = command
        .arg("-")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", program.display()))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("codex exceeded {}s", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let text = fs::read_to_string(&log_path).unwrap_or_default();
    if status.success() {
        let _ = fs::remove_file(&log_path);
    } else {
        let reason = text
            .lines()
            .rev()
            .find(|l| l.contains("ERROR"))
            .unwrap_or("codex exec failed");
        return Err(format!("{} (log: {})", reason.trim(), log_path.display()));
    }
    Ok(())
}

fn is_png(path: &Path) -> bool {
    fs::read(path).is_ok_and(|b| b.len() > 1024 && b.starts_with(b"\x89PNG\r\n\x1a\n"))
}

/// Generates missing frames (all frames with `force`). Returns the frames written.
pub fn generate_frames(
    dir: &Path,
    program: &Path,
    force: bool,
    timeout: Duration,
    progress: &dyn Fn(&str),
) -> Result<Vec<PathBuf>, String> {
    let dir = &fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let look = load_look(dir)?;
    let mut written = Vec::new();
    for frame in FRAMES {
        let target = dir.join(frame);
        if !force && is_png(&target) {
            continue;
        }
        let base = dir.join("idle.png");
        let image = (frame != "idle.png").then_some(base.as_path());
        if image.is_some_and(|b| !is_png(b)) {
            return Err(format!(
                "{}: idle.png missing; cannot derive {frame}",
                dir.display()
            ));
        }
        progress(&format!("drawing {frame}"));
        let _ = fs::remove_file(&target);
        run_codex(program, dir, &frame_prompt(&look, frame), image, timeout)?;
        if !is_png(&target) {
            return Err(format!("codex did not save a PNG at {}", target.display()));
        }
        written.push(target);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_keep_style_and_edit_only_one_feature() {
        let look = Look {
            look: "A fictional elderly man with a fez.".into(),
        };
        let idle = frame_prompt(&look, "idle.png");
        assert!(
            idle.contains("Not a cartoon") && idle.contains("fez") && idle.contains("./idle.png")
        );
        let talk = frame_prompt(&look, "talk.png");
        assert!(
            talk.contains("Change only") && talk.contains("mouth") && talk.contains("./talk.png")
        );
        assert!(frame_prompt(&look, "blink.png").contains("eyes"));
    }

    #[test]
    fn fake_codex_writes_frames_and_existing_ones_are_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("avatar.toml"),
            "look = \"A fictional teacher with glasses and grey hair.\"",
        )
        .unwrap();
        let fake = dir.path().join("fake-codex");
        // Writes a minimal PNG to whichever ./name.png the prompt asks for.
        fs::write(
            &fake,
            "#!/bin/sh\nname=$(grep -o '\\./[a-z]*\\.png' | head -1)\nprintf '\\211PNG\\r\\n\\032\\n' > \"$name\"\nhead -c 2048 /dev/zero >> \"$name\"\n",
        )
        .unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        let written =
            generate_frames(dir.path(), &fake, false, Duration::from_secs(10), &|_| {}).unwrap();
        assert_eq!(written.len(), 3);
        assert!(
            generate_frames(dir.path(), &fake, false, Duration::from_secs(10), &|_| {})
                .unwrap()
                .is_empty()
        );
    }
}
