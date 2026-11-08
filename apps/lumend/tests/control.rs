use lumen_protocol::{Command, Envelope, read_response, write_frame};
use std::{
    os::unix::{fs::PermissionsExt, net::UnixStream},
    process::{Child, Command as Process},
    time::{Duration, Instant},
};
struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn command(path: &std::path::Path, command: Command) -> lumen_protocol::Response {
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(&mut stream, &Envelope::command(command)).unwrap();
    read_response(&mut stream).unwrap()
}
#[test]
fn disconnect_reconnect_and_emergency_stop_keep_daemon_alive() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = dir.path().join("control.sock");
    let mut daemon = Daemon(
        Process::new(env!("CARGO_BIN_EXE_lumend"))
            .arg("serve")
            .arg(&socket)
            .arg(dir.path().join("journal.sqlite"))
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(daemon.0.try_wait().unwrap().is_none(), "daemon exited");
        assert!(Instant::now() < deadline, "daemon did not bind");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let initial = command(&socket, Command::Status);
    assert!(initial.paused);
    let replay = Process::new(env!("CARGO_BIN_EXE_lumend"))
        .arg("replay-chat")
        .arg(&socket)
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/audience.jsonl"))
        .output()
        .unwrap();
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert!(String::from_utf8_lossy(&replay.stdout).contains("accepted=2 rejected=1"));
    assert!(command(&socket, Command::Status).paused);

    drop(UnixStream::connect(&socket).unwrap());
    assert!(!command(&socket, Command::Resume).paused);
    assert!(command(&socket, Command::Pause).paused);
    assert!(command(&socket, Command::MuteAll).muted);
    let stopped = command(&socket, Command::EmergencyStop);
    assert!(stopped.stopped && stopped.paused && stopped.muted);
    assert!(command(&socket, Command::Resume).error.is_some());
    assert!(command(&socket, Command::Status).stopped);
    assert!(daemon.0.try_wait().unwrap().is_none());
}
#[test]
fn refuses_existing_socket_path_without_deleting_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("control.sock");
    std::fs::write(&path, "keep me").unwrap();
    let output = Process::new(env!("CARGO_BIN_EXE_lumend"))
        .arg("serve")
        .arg(&path)
        .arg(dir.path().join("journal.sqlite"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "keep me");
}

#[test]
fn control_cli_reports_unavailable_socket() {
    let output = Process::new(env!("CARGO_BIN_EXE_lumend"))
        .args(["control", "/nonexistent/lumen.sock", "status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("socket"));
}
