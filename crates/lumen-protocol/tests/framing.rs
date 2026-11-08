use lumen_protocol::{Command, Envelope, MAX_FRAME_BYTES, read_frame, write_frame};
use std::io::Cursor;
#[test]
fn messagepack_roundtrip() {
    let message = Envelope::command(Command::Pause);
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &message).unwrap();
    assert_eq!(read_frame(&mut Cursor::new(bytes)).unwrap(), message);
}
#[test]
fn oversized_header_rejected_before_payload_read() {
    let bytes = ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes();
    assert_eq!(
        read_frame(&mut Cursor::new(bytes)).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}
#[test]
fn malformed_truncated_and_unknown_version_rejected() {
    assert!(read_frame(&mut Cursor::new(vec![0, 0, 0, 2, 0xc1, 0xc1])).is_err());
    assert!(read_frame(&mut Cursor::new(vec![0, 0, 0, 8, 1])).is_err());
    let mut message = Envelope::command(Command::EmergencyStop);
    message.version = 9;
    assert!(write_frame(&mut Vec::new(), &message).is_err());
}
#[test]
fn unix_socket_transport() {
    let (mut left, mut right) = std::os::unix::net::UnixStream::pair().unwrap();
    let message = Envelope::command(Command::MuteAll);
    write_frame(&mut left, &message).unwrap();
    assert_eq!(read_frame(&mut right).unwrap(), message);
}
