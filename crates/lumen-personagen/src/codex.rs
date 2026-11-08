//! `codex exec` drafting backend: isolated temp workspace, read-only sandbox, ephemeral
//! session, strict output schema, bounded output and a hard deadline.
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MAX_OUTPUT_BYTES: u64 = 64 * 1024;
const LOG_TAIL_BYTES: usize = 1500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// Worth retrying (empty answer, transient failure, timeout).
    Transient(String),
    /// Retrying will not help now (missing binary, auth, usage limit).
    Fatal(String),
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient(m) | Self::Fatal(m) => f.write_str(m),
        }
    }
}

/// Anything that turns a prompt + JSON schema into a JSON answer.
pub trait DraftSource {
    fn draft(&self, prompt: &str, schema: &str) -> Result<String, SourceError>;
    fn describe(&self) -> String;
}

#[derive(Debug, Clone)]
pub struct CodexExec {
    pub program: PathBuf,
    pub model: Option<String>,
    pub timeout: Duration,
}

impl CodexExec {
    /// `LUMEN_CODEX_BIN` overrides the binary (tests, pinned installs); default `codex`.
    pub fn from_env(model: Option<String>, timeout: Duration) -> Self {
        let program =
            std::env::var_os("LUMEN_CODEX_BIN").map_or_else(|| "codex".into(), PathBuf::from);
        Self {
            program,
            model,
            timeout,
        }
    }

    fn args(&self, dir: &std::path::Path) -> Vec<std::ffi::OsString> {
        let mut args: Vec<std::ffi::OsString> = [
            "exec",
            "--skip-git-repo-check",
            "--ephemeral",
            "--sandbox",
            "read-only",
            "--color",
            "never",
        ]
        .map(Into::into)
        .into();
        args.extend(["-C".into(), dir.into()]);
        args.extend(["--output-schema".into(), dir.join("schema.json").into()]);
        args.extend([
            "--output-last-message".into(),
            dir.join("answer.json").into(),
        ]);
        if let Some(model) = &self.model {
            args.extend(["--model".into(), model.into()]);
        }
        args.push("-".into());
        args
    }
}

fn tail(path: &std::path::Path) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    let start = bytes.len().saturating_sub(LOG_TAIL_BYTES);
    let text = String::from_utf8_lossy(&bytes[start..]).into_owned();
    // codex echoes the prompt into its log; prefer its own deduplicated error lines.
    let mut errors: Vec<&str> = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("ERROR"))
    {
        if !errors.contains(&line) {
            errors.push(line);
        }
    }
    if errors.is_empty() {
        text.trim().to_owned()
    } else {
        errors.join("\n")
    }
}

fn classify(log: &str) -> Option<&'static str> {
    let lower = log.to_lowercase();
    [
        ("usage limit", "codex usage limit reached"),
        (
            "not logged in",
            "codex is not logged in (run `codex login`)",
        ),
        ("unauthorized", "codex authentication failed"),
    ]
    .into_iter()
    .find_map(|(needle, message)| lower.contains(needle).then_some(message))
}

impl DraftSource for CodexExec {
    fn describe(&self) -> String {
        format!(
            "codex exec ({})",
            self.model.as_deref().unwrap_or("default model")
        )
    }

    fn draft(&self, prompt: &str, schema: &str) -> Result<String, SourceError> {
        let io = |e: std::io::Error| SourceError::Fatal(format!("persona workspace: {e}"));
        let dir = tempfile::Builder::new()
            .prefix("lumen-personagen-")
            .tempdir()
            .map_err(io)?;
        fs::write(dir.path().join("schema.json"), schema).map_err(io)?;
        let log_path = dir.path().join("codex.log");
        let log = fs::File::create(&log_path).map_err(io)?;
        let mut child = Command::new(&self.program)
            .args(self.args(dir.path()))
            .current_dir(dir.path())
            .stdin(Stdio::piped())
            .stdout(log.try_clone().map_err(io)?)
            .stderr(log)
            .spawn()
            .map_err(|e| {
                SourceError::Fatal(format!("cannot start {}: {e}", self.program.display()))
            })?;
        // Prompts are a few KiB, below the pipe buffer, so writing before polling cannot block.
        let written = child
            .stdin
            .take()
            .map(|mut stdin| stdin.write_all(prompt.as_bytes()));
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait().map_err(io)? {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SourceError::Transient(format!(
                        "codex exec exceeded {}s deadline",
                        self.timeout.as_secs()
                    )));
                }
                None => std::thread::sleep(Duration::from_millis(100)),
            }
        };
        if let Some(Err(e)) = written {
            return Err(SourceError::Transient(format!(
                "writing prompt to codex: {e}"
            )));
        }
        let log = tail(&log_path);
        if !status.success() {
            return Err(match classify(&log) {
                Some(reason) => SourceError::Fatal(format!("{reason}: {log}")),
                None => SourceError::Transient(format!("codex exec failed ({status}): {log}")),
            });
        }
        let mut answer = String::new();
        fs::File::open(dir.path().join("answer.json"))
            .map_err(|_| SourceError::Transient(format!("codex wrote no answer: {log}")))?
            .take(MAX_OUTPUT_BYTES + 1)
            .read_to_string(&mut answer)
            .map_err(|e| SourceError::Transient(format!("reading codex answer: {e}")))?;
        if answer.len() as u64 > MAX_OUTPUT_BYTES {
            return Err(SourceError::Transient("codex answer too large".into()));
        }
        if answer.trim().is_empty() {
            return Err(SourceError::Transient("codex answer was empty".into()));
        }
        Ok(answer)
    }
}
