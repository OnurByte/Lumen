#[test]
fn scenario_runs_real_director_and_persists_only_simulated_playback() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lumend"))
        .args([
            "simulate",
            dir.path().join("show.sqlite").to_str().unwrap(),
            "1800",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("simulated_seconds=1800"), "{text}");
    assert!(text.contains("turns=120"), "{text}");
    assert!(text.contains("stale_ack_rejected=24"), "{text}");
}
#[test]
fn rejects_invalid_scenario_duration() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lumend"))
        .args(["simulate", "/unused.sqlite", "0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn validates_builtin_show_pack_without_network_or_secrets() {
    let pack = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packs/personas/history-clash.toml");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lumend"))
        .arg("validate-pack")
        .arg(pack)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("History Clash: Ada / Deniz"));
}
