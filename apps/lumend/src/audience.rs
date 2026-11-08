//! Bounded fixture replay through exactly the same ingress used by live connectors.
use lumen_protocol::{AudienceInput, Command};
use std::{
    io::{BufRead, Read},
    path::Path,
    time::Duration,
};
pub fn replay(socket: &Path, path: &Path) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::new(file);
    let mut accepted = 0u64;
    let mut rejected = 0u64;
    let mut line_number = 0u64;
    loop {
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take(8193)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        line_number += 1;
        if count > 8192 {
            return Err(format!("comment line {line_number} exceeds wire limit"));
        }
        let input: AudienceInput = serde_json::from_slice(&line)
            .map_err(|_| format!("invalid comment JSON at line {line_number}"))?;
        let response =
            lumen_protocol::request(socket, Command::Audience(input), Duration::from_secs(2))
                .map_err(|e| format!("audience ingress: {e}"))?;
        if response.error.is_some() {
            rejected += 1;
        } else {
            accepted += 1;
        }
    }
    println!("accepted={accepted} rejected={rejected}");
    Ok(())
}
