//! Kwai LIVE TCP framing, reimplemented from observed traffic in weteta/kwai-live.
//!
//! ```text
//! 0      4               12      16
//! | magic | reserved (0) | len   | payload (protobuf SocketMessage, `len` bytes)
//! ```
//! `magic` = 0x011A2B3C. All observed frames carry zeros in bytes 4..12; the reference client
//! split frames by searching the hex stream for this header, we parse lengths instead.
use bytes::{Buf, BufMut, Bytes, BytesMut};

pub const MAGIC: u32 = 0x011A_2B3C;
pub const HEADER_LEN: usize = 16;
pub const MAX_PAYLOAD: usize = 4 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    BadMagic(u32),
    TooLarge(usize),
}

pub fn encode(payload: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(HEADER_LEN + payload.len());
    out.put_u32(MAGIC);
    out.put_u64(0);
    out.put_u32(payload.len() as u32);
    out.put_slice(payload);
    out.freeze()
}

/// Pop one complete frame payload from `buf`, or `Ok(None)` if more bytes are needed.
pub fn decode(buf: &mut BytesMut) -> Result<Option<Bytes>, FrameError> {
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }
    let magic = u32::from_be_bytes(buf[0..4].try_into().unwrap());
    if magic != MAGIC {
        return Err(FrameError::BadMagic(magic));
    }
    let len = u32::from_be_bytes(buf[12..16].try_into().unwrap()) as usize;
    if len > MAX_PAYLOAD {
        return Err(FrameError::TooLarge(len));
    }
    if buf.len() < HEADER_LEN + len {
        return Ok(None);
    }
    buf.advance(HEADER_LEN);
    Ok(Some(buf.split_to(len).freeze()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_matches_reference_bytes_and_streams_split() {
        // Reference heartbeat header "011A2B3C00000000000000000000001A" for a 26-byte payload.
        let f = encode(&[0u8; 26]);
        assert_eq!(&f[..16], &[0x01, 0x1A, 0x2B, 0x3C, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1A]);
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&encode(b"one"));
        buf.extend_from_slice(&encode(b"two")[..10]);
        assert_eq!(decode(&mut buf).unwrap().unwrap(), Bytes::from_static(b"one"));
        assert_eq!(decode(&mut buf).unwrap(), None);
        buf.extend_from_slice(&encode(b"two")[10..]);
        assert_eq!(decode(&mut buf).unwrap().unwrap(), Bytes::from_static(b"two"));
        let mut bad = BytesMut::from(&[0u8; 16][..]);
        assert_eq!(decode(&mut bad), Err(FrameError::BadMagic(0)));
        let mut huge = BytesMut::from(&encode(b"")[..]);
        huge[12..16].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(decode(&mut huge), Err(FrameError::TooLarge(_))));
    }
}
