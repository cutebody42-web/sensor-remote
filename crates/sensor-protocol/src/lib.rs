//! Versioned wire types for SENSOR rendezvous and session establishment.
//!
//! The handshake messages are canonicalized before signing. Any field added
//! in a future protocol version must either be included in the signed form or
//! be explicitly declared non-security-sensitive.

use postcard::to_allocvec;
use sensor_core::DeviceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PROTOCOL_VERSION: u16 = 2;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;
const HEADER_SIZE: usize = 7;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("serialization failed: {0}")]
    Serialization(#[from] postcard::Error),
    #[error("unsupported protocol version: {0}")]
    UnsupportedVersion(u16),
    #[error("frame exceeds maximum size")]
    FrameTooLarge,
    #[error("invalid message type")]
    InvalidMessageType,
    #[error("malformed frame or trailing bytes")]
    Malformed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageType {
    ClientHello = 1,
    ServerHello = 2,
    Control = 3,
    FileTransfer = 4,
    Close = 5,
}

impl TryFrom<u8> for MessageType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::ClientHello),
            2 => Ok(Self::ServerHello),
            3 => Ok(Self::Control),
            4 => Ok(Self::FileTransfer),
            5 => Ok(Self::Close),
            _ => Err(ProtocolError::InvalidMessageType),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub version: u16,
    pub message_type: MessageType,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new<T: Serialize>(
        message_type: MessageType,
        message: &T,
    ) -> Result<Self, ProtocolError> {
        let payload = to_allocvec(message)?;
        if payload.len() > MAX_FRAME_SIZE - HEADER_SIZE {
            return Err(ProtocolError::FrameTooLarge);
        }
        Ok(Self {
            version: PROTOCOL_VERSION,
            message_type,
            payload,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(self.version));
        }
        if self.payload.len() > MAX_FRAME_SIZE - HEADER_SIZE {
            return Err(ProtocolError::FrameTooLarge);
        }
        let mut bytes = Vec::with_capacity(HEADER_SIZE + self.payload.len());
        bytes.extend_from_slice(&self.version.to_be_bytes());
        bytes.push(self.message_type as u8);
        bytes.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_FRAME_SIZE {
            return Err(ProtocolError::FrameTooLarge);
        }
        if bytes.len() < HEADER_SIZE {
            return Err(ProtocolError::Malformed);
        }
        let version = u16::from_be_bytes([bytes[0], bytes[1]]);
        if version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(version));
        }
        let length = u32::from_be_bytes(
            bytes[3..7]
                .try_into()
                .map_err(|_| ProtocolError::Malformed)?,
        ) as usize;
        if length != bytes.len() - HEADER_SIZE {
            return Err(ProtocolError::Malformed);
        }
        Ok(Self {
            version,
            message_type: bytes[2].try_into()?,
            payload: bytes[HEADER_SIZE..].to_vec(),
        })
    }

    pub fn decode_payload<T: for<'de> Deserialize<'de>>(&self) -> Result<T, ProtocolError> {
        let (value, rest) = postcard::take_from_bytes(&self.payload)?;
        if !rest.is_empty() {
            return Err(ProtocolError::Malformed);
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClientHello {
    pub version: u16,
    pub device_id: DeviceId,
    pub target_device_id: DeviceId,
    pub target_identity_key: [u8; 32],
    pub identity_public_key: [u8; 32],
    pub ephemeral_public_key: [u8; 32],
    pub nonce: [u8; 32],
    pub signature: Vec<u8>,
}

#[derive(Serialize)]
struct ClientHelloUnsigned<'a> {
    version: u16,
    device_id: DeviceId,
    target_device_id: DeviceId,
    target_identity_key: &'a [u8; 32],
    identity_public_key: &'a [u8; 32],
    ephemeral_public_key: &'a [u8; 32],
    nonce: &'a [u8; 32],
}

impl ClientHello {
    pub fn unsigned_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        Ok(to_allocvec(&ClientHelloUnsigned {
            version: self.version,
            device_id: self.device_id,
            target_device_id: self.target_device_id,
            target_identity_key: &self.target_identity_key,
            identity_public_key: &self.identity_public_key,
            ephemeral_public_key: &self.ephemeral_public_key,
            nonce: &self.nonce,
        })?)
    }
}

pub fn client_signature_bytes(client: &ClientHello) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = b"SENSOR Remote Access/client-hello/v1".to_vec();
    bytes.extend_from_slice(&client.unsigned_bytes()?);
    Ok(bytes)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServerHello {
    pub version: u16,
    pub device_id: DeviceId,
    pub identity_public_key: [u8; 32],
    pub ephemeral_public_key: [u8; 32],
    pub client_nonce: [u8; 32],
    pub nonce: [u8; 32],
    pub signature: Vec<u8>,
}

#[derive(Serialize)]
struct ServerHelloUnsigned<'a> {
    version: u16,
    device_id: DeviceId,
    identity_public_key: &'a [u8; 32],
    ephemeral_public_key: &'a [u8; 32],
    client_nonce: &'a [u8; 32],
    nonce: &'a [u8; 32],
}

impl ServerHello {
    pub fn unsigned_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        Ok(to_allocvec(&ServerHelloUnsigned {
            version: self.version,
            device_id: self.device_id,
            identity_public_key: &self.identity_public_key,
            ephemeral_public_key: &self.ephemeral_public_key,
            client_nonce: &self.client_nonce,
            nonce: &self.nonce,
        })?)
    }
}

pub fn server_signature_bytes(
    client: &ClientHello,
    server: &ServerHello,
) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = b"SENSOR Remote Access/server-hello/v1".to_vec();
    bytes.extend_from_slice(&client.unsigned_bytes()?);
    bytes.extend_from_slice(&client.signature);
    bytes.extend_from_slice(&server.unsigned_bytes()?);
    Ok(bytes)
}

pub fn handshake_transcript(
    client: &ClientHello,
    server: &ServerHello,
) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = b"SENSOR Remote Access/handshake/v1".to_vec();
    bytes.extend_from_slice(&client.unsigned_bytes()?);
    bytes.extend_from_slice(&client.signature);
    bytes.extend_from_slice(&server.unsigned_bytes()?);
    bytes.extend_from_slice(&server.signature);
    Ok(bytes)
}

pub fn transcript_digest(
    client: &ClientHello,
    server: &ServerHello,
) -> Result<[u8; 32], ProtocolError> {
    Ok(Sha256::digest(handshake_transcript(client, server)?).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_has_a_fixed_header_and_exact_payload() {
        let frame = Frame::new(MessageType::Control, &42_u8).unwrap();
        let bytes = frame.encode().unwrap();
        assert_eq!(bytes, [0, 2, 3, 0, 0, 0, 1, 42]);
        assert_eq!(
            Frame::decode(&bytes)
                .unwrap()
                .decode_payload::<u8>()
                .unwrap(),
            42
        );
        for length in 0..bytes.len() {
            assert!(Frame::decode(&bytes[..length]).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(Frame::decode(&trailing).is_err());
        let mut frame = frame;
        frame.payload.push(0);
        assert!(frame.decode_payload::<u8>().is_err());
    }
    #[test]
    fn rejects_unknown_types_versions_and_oversize_lengths() {
        assert!(matches!(
            Frame::decode(&[0, 1, 3, 0, 0, 0, 0]),
            Err(ProtocolError::UnsupportedVersion(1))
        ));
        assert!(matches!(
            Frame::decode(&[0, 2, 255, 0, 0, 0, 0]),
            Err(ProtocolError::InvalidMessageType)
        ));
        assert!(Frame::decode(&[0, 2, 3, 255, 255, 255, 255]).is_err());
        assert!(matches!(
            Frame::decode(&vec![0; MAX_FRAME_SIZE + 1]),
            Err(ProtocolError::FrameTooLarge)
        ));
    }
    #[test]
    fn deterministic_malformed_corpus_never_panics() {
        // Reproducible parser mutation regression, not a substitute for libFuzzer campaigns.
        let seed = Frame::new(MessageType::Control, &vec![0_u8; 128])
            .unwrap()
            .encode()
            .unwrap();
        for index in 0..seed.len() {
            for replacement in [0_u8, 1, 2, 127, 128, 255] {
                let mut input = seed.clone();
                input[index] = replacement;
                if let Ok(frame) = Frame::decode(&input) {
                    let _ = frame.decode_payload::<Vec<u8>>();
                }
            }
        }
    }
}
