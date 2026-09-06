//! Versioned wire types for SENSOR rendezvous and session establishment.
//!
//! The handshake messages are canonicalized before signing. Any field added
//! in a future protocol version must either be included in the signed form or
//! be explicitly declared non-security-sensitive.

use postcard::{from_bytes, to_allocvec};
use sensor_core::DeviceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

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
        if payload.len() > MAX_FRAME_SIZE {
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
        if self.payload.len() > MAX_FRAME_SIZE {
            return Err(ProtocolError::FrameTooLarge);
        }
        Ok(to_allocvec(self)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let frame: Self = from_bytes(bytes)?;
        if frame.version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(frame.version));
        }
        if frame.payload.len() > MAX_FRAME_SIZE {
            return Err(ProtocolError::FrameTooLarge);
        }
        Ok(frame)
    }

    pub fn decode_payload<T: for<'de> Deserialize<'de>>(&self) -> Result<T, ProtocolError> {
        Ok(from_bytes(&self.payload)?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClientHello {
    pub version: u16,
    pub device_id: DeviceId,
    pub identity_public_key: [u8; 32],
    pub ephemeral_public_key: [u8; 32],
    pub nonce: [u8; 32],
    pub signature: Vec<u8>,
}

#[derive(Serialize)]
struct ClientHelloUnsigned<'a> {
    version: u16,
    device_id: DeviceId,
    identity_public_key: &'a [u8; 32],
    ephemeral_public_key: &'a [u8; 32],
    nonce: &'a [u8; 32],
}

impl ClientHello {
    pub fn unsigned_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        Ok(to_allocvec(&ClientHelloUnsigned {
            version: self.version,
            device_id: self.device_id,
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
