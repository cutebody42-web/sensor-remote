//! Length-delimited transport framing.
//!
//! Socket establishment, ICE/STUN, QUIC, relay selection, and proxy support
//! are intentionally separate layers. This crate only handles bounded,
//! incremental framing so a future transport cannot accidentally allocate
//! from an attacker-controlled length.

use sensor_protocol::{Frame, ProtocolError, MAX_FRAME_SIZE};
use thiserror::Error;

const LENGTH_PREFIX_SIZE: usize = 4;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("transport frame exceeds maximum size")]
    FrameTooLarge,
}

pub fn encode_length_delimited(frame: &Frame) -> Result<Vec<u8>, TransportError> {
    let payload = frame.encode()?;
    if payload.len() > MAX_FRAME_SIZE {
        return Err(TransportError::FrameTooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| TransportError::FrameTooLarge)?;
    let mut output = Vec::with_capacity(LENGTH_PREFIX_SIZE + payload.len());
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(&payload);
    Ok(output)
}

/// Attempts to decode one frame from an incrementally filled buffer.
///
/// The consumed bytes are removed only after a complete, valid frame is
/// available. This makes it safe to call after every socket read.
pub fn decode_next(buffer: &mut Vec<u8>) -> Result<Option<Frame>, TransportError> {
    if buffer.len() < LENGTH_PREFIX_SIZE {
        return Ok(None);
    }
    let length = u32::from_be_bytes(buffer[..LENGTH_PREFIX_SIZE].try_into().unwrap()) as usize;
    if length > MAX_FRAME_SIZE {
        return Err(TransportError::FrameTooLarge);
    }
    if buffer.len() < LENGTH_PREFIX_SIZE + length {
        return Ok(None);
    }
    let payload = buffer[LENGTH_PREFIX_SIZE..LENGTH_PREFIX_SIZE + length].to_vec();
    buffer.drain(..LENGTH_PREFIX_SIZE + length);
    Ok(Some(Frame::decode(&payload)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sensor_protocol::{Frame, MessageType};

    #[derive(serde::Serialize, serde::Deserialize, Debug, Eq, PartialEq)]
    struct Ping {
        nonce: u32,
    }

    #[test]
    fn decoder_handles_partial_reads() {
        let frame = Frame::new(MessageType::Control, &Ping { nonce: 42 }).unwrap();
        let encoded = encode_length_delimited(&frame).unwrap();
        let mut buffer = Vec::new();
        for byte in encoded {
            buffer.push(byte);
            if buffer.len() < 4 + frame.encode().unwrap().len() {
                assert!(decode_next(&mut buffer).unwrap().is_none());
            }
        }
        let decoded = decode_next(&mut buffer).unwrap().unwrap();
        assert_eq!(
            decoded.decode_payload::<Ping>().unwrap(),
            Ping { nonce: 42 }
        );
        assert!(buffer.is_empty());
    }
}
