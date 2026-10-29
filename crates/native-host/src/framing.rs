//! Chrome/Firefox native messaging framing.
//!
//! Each message is a 32-bit native-endian length followed by UTF-8 JSON. The
//! documented maximum message size is 1 MiB in both directions; we enforce it
//! before allocating so a malicious peer cannot exhaust memory.

use std::io::{self, Read, Write};

/// Maximum accepted frame size.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Read one frame. Returns `Ok(None)` on a clean EOF before a new frame.
pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut length_bytes = [0_u8; 4];
    match reader.read_exact(&mut length_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }

    let length = u32::from_ne_bytes(length_bytes) as usize;
    if length == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native message must not be empty",
        ));
    }
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("native message exceeds {MAX_FRAME_BYTES} bytes"),
        ));
    }

    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}

/// Write one frame and flush it.
pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    if payload.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native message must not be empty",
        ));
    }
    if payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("native message exceeds {MAX_FRAME_BYTES} bytes"),
        ));
    }
    let length = u32::try_from(payload.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "native message length overflow",
        )
    })?;
    writer.write_all(&length.to_ne_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_a_frame() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, br#"{"method":"ping"}"#).unwrap();
        let mut cursor = Cursor::new(buffer);
        let frame = read_frame(&mut cursor).unwrap().unwrap();
        assert_eq!(frame, br#"{"method":"ping"}"#);
    }

    #[test]
    fn clean_eof_returns_none() {
        let mut cursor = Cursor::new(Vec::new());
        assert!(read_frame(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn rejects_oversized_frame_without_allocating() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&u32::try_from(MAX_FRAME_BYTES + 1).unwrap().to_ne_bytes());
        let mut cursor = Cursor::new(buffer);
        let error = read_frame(&mut cursor).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_empty_frame() {
        let mut cursor = Cursor::new(0_u32.to_ne_bytes().to_vec());
        assert!(read_frame(&mut cursor).is_err());
        let mut output = Vec::new();
        assert!(write_frame(&mut output, b"").is_err());
    }

    #[test]
    fn truncated_payload_is_an_error() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&10_u32.to_ne_bytes());
        buffer.extend_from_slice(b"short");
        let mut cursor = Cursor::new(buffer);
        assert!(read_frame(&mut cursor).is_err());
    }
}
