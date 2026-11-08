//! Exercises the real process contract against a fake `codex` executable.
use lumen_personagen::{CodexExec, DraftSource, SourceError};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

fn fake(dir: &std::path::Path, body: &str) -> PathBuf {
    let path = dir.join("codex");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn exec(program: PathBuf, timeout: Duration) -> CodexExec {
    CodexExec {
        program,
        model: Some("test-model".into()),
        timeout,
    }
}

#[test]
fn passes_hardened_flags_schema_and_prompt_and_reads_last_message() {
    let dir = tempfile::tempdir().unwrap();
    let args = dir.path().join("args.txt");
    // Records argv, checks schema file exists, echoes stdin into the answer file.
    let script = format!(
        r#"printf '%s\n' "$@" > {args}
out=""; schema=""
while [ $# -gt 0 ]; do
  case "$1" in
    --output-last-message) out="$2"; shift ;;
    --output-schema) schema="$2"; shift ;;
  esac
  shift
done
grep -q additionalProperties "$schema" || exit 9
prompt=$(cat)
printf '{{"echo":"%s"}}' "$prompt" > "$out""#,
        args = args.display()
    );
    let answer = exec(fake(dir.path(), &script), Duration::from_secs(10))
        .draft("merhaba", r#"{"additionalProperties": false}"#)
        .unwrap();
    assert_eq!(answer, r#"{"echo":"merhaba"}"#);
    let argv = std::fs::read_to_string(args).unwrap();
    let argv: Vec<_> = argv.lines().collect();
    assert_eq!(argv[0], "exec");
    for flag in [
        "--skip-git-repo-check",
        "--ephemeral",
        "-C",
        "--output-schema",
        "--output-last-message",
    ] {
        assert!(argv.contains(&flag), "{flag} missing: {argv:?}");
    }
    let sandbox = argv.iter().position(|a| *a == "--sandbox").unwrap();
    assert_eq!(argv[sandbox + 1], "read-only");
    let model = argv.iter().position(|a| *a == "--model").unwrap();
    assert_eq!(argv[model + 1], "test-model");
    assert_eq!(*argv.last().unwrap(), "-");
}

#[test]
fn usage_limit_is_fatal_other_failures_transient() {
    let dir = tempfile::tempdir().unwrap();
    let limited = exec(
        fake(
            dir.path(),
            "cat >/dev/null; echo \"ERROR: You've hit your usage limit.\" >&2; exit 1",
        ),
        Duration::from_secs(10),
    );
    assert!(
        matches!(limited.draft("x", "{}"), Err(SourceError::Fatal(m)) if m.contains("usage limit"))
    );
    let crashed = exec(
        fake(dir.path(), "cat >/dev/null; echo boom >&2; exit 3"),
        Duration::from_secs(10),
    );
    assert!(
        matches!(crashed.draft("x", "{}"), Err(SourceError::Transient(m)) if m.contains("boom"))
    );
    let silent = exec(
        fake(dir.path(), "cat >/dev/null; exit 0"),
        Duration::from_secs(10),
    );
    assert!(
        matches!(silent.draft("x", "{}"), Err(SourceError::Transient(m)) if m.contains("no answer"))
    );
    let missing = exec(dir.path().join("nope"), Duration::from_secs(1));
    assert!(matches!(
        missing.draft("x", "{}"),
        Err(SourceError::Fatal(_))
    ));
}

#[test]
fn hung_codex_is_killed_at_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    let hung = exec(
        fake(dir.path(), "exec sleep 30"),
        Duration::from_millis(500),
    );
    assert!(
        matches!(hung.draft("x", "{}"), Err(SourceError::Transient(m)) if m.contains("deadline"))
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}
