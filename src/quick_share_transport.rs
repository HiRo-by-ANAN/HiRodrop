//! Length-prefixed TCP framing shared by Nearby Connections messages.

use std::io::{Read, Write};

pub const MAX_WIRE_FRAME_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameIoError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("wire frame length {0} is outside the permitted range")]
    InvalidLength(usize),
}

impl FrameIoError {
    /// Nearby clients are allowed to probe an advertised port and disconnect
    /// before sending a complete frame.  This is not a transfer failure.
    pub fn is_peer_disconnect(&self) -> bool {
        matches!(
            self,
            Self::Io(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                )
        )
    }
}

pub fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, FrameIoError> {
    let mut prefix = [0_u8; 4];
    reader.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_WIRE_FRAME_BYTES {
        return Err(FrameIoError::InvalidLength(length));
    }
    let mut frame = vec![0_u8; length];
    reader.read_exact(&mut frame)?;
    Ok(frame)
}

pub fn write_frame(writer: &mut impl Write, frame: &[u8]) -> Result<(), FrameIoError> {
    if frame.is_empty() || frame.len() > MAX_WIRE_FRAME_BYTES {
        return Err(FrameIoError::InvalidLength(frame.len()));
    }
    writer.write_all(&(frame.len() as u32).to_be_bytes())?;
    writer.write_all(frame)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_round_trip_is_big_endian() {
        let mut wire = Vec::new();
        write_frame(&mut wire, b"hello").unwrap();
        assert_eq!(&wire[..4], &[0, 0, 0, 5]);
        assert_eq!(read_frame(&mut Cursor::new(wire)).unwrap(), b"hello");
    }

    #[test]
    fn oversized_frame_is_rejected_before_allocation() {
        let wire = ((MAX_WIRE_FRAME_BYTES + 1) as u32).to_be_bytes();
        assert!(matches!(
            read_frame(&mut Cursor::new(wire)),
            Err(FrameIoError::InvalidLength(_))
        ));
    }
}
