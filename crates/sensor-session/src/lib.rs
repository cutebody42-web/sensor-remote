//! Mutual-authentication handshake and secure session state.
pub mod permissions;

use rand_core::{OsRng, RngCore};
use sensor_core::DeviceId;
use sensor_crypto::{
    derive_session_keys, transcript_hash, CryptoError, EphemeralKeypair, IdentityKeypair,
    SecureChannel,
};
use sensor_protocol::{
    client_signature_bytes, handshake_transcript, server_signature_bytes, ClientHello,
    ProtocolError, ServerHello, PROTOCOL_VERSION,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("cryptographic error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("handshake message has an unsupported version")]
    UnsupportedVersion,
    #[error("handshake nonce does not match")]
    NonceMismatch,
    #[error("peer does not match the expected device and pinned public key")]
    PeerMismatch,
}

/// Obtained from an explicitly trusted registration or out-of-band exchange.
/// A key supplied by the peer itself is not a trust decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpectedPeer {
    pub device_id: DeviceId,
    pub public_key: [u8; 32],
}

pub struct InitiatorState {
    expected: ExpectedPeer,
    ephemeral: EphemeralKeypair,
    client_hello: ClientHello,
}

pub struct ResponderState {
    ephemeral: EphemeralKeypair,
    client_hello: ClientHello,
    server_hello: ServerHello,
}

pub struct SecureSession {
    peer_device_id: DeviceId,
    peer_identity_key: [u8; 32],
    transcript_hash: [u8; 32],
    channel: SecureChannel,
}

impl SecureSession {
    pub fn peer_device_id(&self) -> DeviceId {
        self.peer_device_id
    }

    pub fn peer_identity_key(&self) -> [u8; 32] {
        self.peer_identity_key
    }

    pub fn transcript_hash(&self) -> [u8; 32] {
        self.transcript_hash
    }

    pub fn seal(
        &mut self,
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<sensor_crypto::EncryptedRecord, SessionError> {
        Ok(self.channel.seal(aad, plaintext)?)
    }

    pub fn open(
        &mut self,
        record: &sensor_crypto::EncryptedRecord,
        aad: &[u8],
    ) -> Result<Vec<u8>, SessionError> {
        Ok(self.channel.open(record, aad)?)
    }
}

pub fn start_initiator(
    identity: &IdentityKeypair,
    device_id: DeviceId,
    expected: ExpectedPeer,
) -> (InitiatorState, ClientHello) {
    let ephemeral = EphemeralKeypair::generate();
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    let mut hello = ClientHello {
        version: PROTOCOL_VERSION,
        device_id,
        target_device_id: expected.device_id,
        target_identity_key: expected.public_key,
        identity_public_key: identity.public_key(),
        ephemeral_public_key: ephemeral.public_key(),
        nonce,
        signature: Vec::new(),
    };
    hello.signature =
        identity.sign(&client_signature_bytes(&hello).expect("hello fields are serializable"));
    let state = InitiatorState {
        expected,
        ephemeral,
        client_hello: hello.clone(),
    };
    (state, hello)
}

pub fn accept_hello(
    identity: &IdentityKeypair,
    device_id: DeviceId,
    client_hello: ClientHello,
    expected: ExpectedPeer,
) -> Result<(ResponderState, ServerHello), SessionError> {
    if client_hello.version != PROTOCOL_VERSION {
        return Err(SessionError::UnsupportedVersion);
    }
    if client_hello.device_id != expected.device_id
        || client_hello.identity_public_key != expected.public_key
        || client_hello.target_device_id != device_id
        || client_hello.target_identity_key != identity.public_key()
    {
        return Err(SessionError::PeerMismatch);
    }
    sensor_crypto::IdentityKeypair::verify(
        &client_hello.identity_public_key,
        &client_signature_bytes(&client_hello)?,
        &client_hello.signature,
    )?;

    let ephemeral = EphemeralKeypair::generate();
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    let mut server_hello = ServerHello {
        version: PROTOCOL_VERSION,
        device_id,
        identity_public_key: identity.public_key(),
        ephemeral_public_key: ephemeral.public_key(),
        client_nonce: client_hello.nonce,
        nonce,
        signature: Vec::new(),
    };
    server_hello.signature = identity.sign(&server_signature_bytes(&client_hello, &server_hello)?);
    let state = ResponderState {
        ephemeral,
        client_hello,
        server_hello: server_hello.clone(),
    };
    Ok((state, server_hello))
}

/// Accept a first connection when the endpoint owner has deliberately chosen
/// visible trust-on-first-use. The target device ID and target identity key
/// are still bound to this endpoint, the caller signature is still verified,
/// and the real peer key is retained in the resulting secure session so the UI
/// can display it in the consent prompt. This function must not be used for
/// unattended access without a separate authenticated policy.
pub fn accept_hello_unpinned(
    identity: &IdentityKeypair,
    device_id: DeviceId,
    client_hello: ClientHello,
) -> Result<(ResponderState, ServerHello), SessionError> {
    if client_hello.version != PROTOCOL_VERSION {
        return Err(SessionError::UnsupportedVersion);
    }
    if client_hello.target_device_id != device_id
        || (client_hello.target_identity_key != [0; 32]
            && client_hello.target_identity_key != identity.public_key())
    {
        return Err(SessionError::PeerMismatch);
    }
    sensor_crypto::IdentityKeypair::verify(
        &client_hello.identity_public_key,
        &client_signature_bytes(&client_hello)?,
        &client_hello.signature,
    )?;

    let ephemeral = EphemeralKeypair::generate();
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    let mut server_hello = ServerHello {
        version: PROTOCOL_VERSION,
        device_id,
        identity_public_key: identity.public_key(),
        ephemeral_public_key: ephemeral.public_key(),
        client_nonce: client_hello.nonce,
        nonce,
        signature: Vec::new(),
    };
    server_hello.signature = identity.sign(&server_signature_bytes(&client_hello, &server_hello)?);
    let state = ResponderState {
        ephemeral,
        client_hello,
        server_hello: server_hello.clone(),
    };
    Ok((state, server_hello))
}

pub fn finish_initiator(
    state: InitiatorState,
    server_hello: ServerHello,
) -> Result<SecureSession, SessionError> {
    if server_hello.version != PROTOCOL_VERSION {
        return Err(SessionError::UnsupportedVersion);
    }
    if server_hello.client_nonce != state.client_hello.nonce {
        return Err(SessionError::NonceMismatch);
    }
    if server_hello.device_id != state.expected.device_id
        || (state.expected.public_key != [0; 32]
            && server_hello.identity_public_key != state.expected.public_key)
    {
        return Err(SessionError::PeerMismatch);
    }
    sensor_crypto::IdentityKeypair::verify(
        &server_hello.identity_public_key,
        &server_signature_bytes(&state.client_hello, &server_hello)?,
        &server_hello.signature,
    )?;

    let transcript = handshake_transcript(&state.client_hello, &server_hello)?;
    let digest = transcript_hash(&transcript);
    let shared = state
        .ephemeral
        .diffie_hellman(&server_hello.ephemeral_public_key)?;
    let keys = derive_session_keys(shared, &transcript)?;
    Ok(SecureSession {
        peer_device_id: server_hello.device_id,
        peer_identity_key: server_hello.identity_public_key,
        transcript_hash: digest,
        channel: SecureChannel::initiator(&keys),
    })
}

pub fn finish_responder(state: ResponderState) -> Result<SecureSession, SessionError> {
    let transcript = handshake_transcript(&state.client_hello, &state.server_hello)?;
    let digest = transcript_hash(&transcript);
    let shared = state
        .ephemeral
        .diffie_hellman(&state.client_hello.ephemeral_public_key)?;
    let keys = derive_session_keys(shared, &transcript)?;
    Ok(SecureSession {
        peer_device_id: state.client_hello.device_id,
        peer_identity_key: state.client_hello.identity_public_key,
        transcript_hash: digest,
        channel: SecureChannel::responder(&keys),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sensor_core::DeviceId;

    #[test]
    fn authenticated_handshake_produces_interoperable_channels() {
        let initiator_identity = IdentityKeypair::from_seed([1u8; 32]);
        let responder_identity = IdentityKeypair::from_seed([2u8; 32]);
        let initiator_id = DeviceId::new(111_222_333).unwrap();
        let responder_id = DeviceId::new(444_555_666).unwrap();

        let (initiator_state, hello) = start_initiator(
            &initiator_identity,
            initiator_id,
            ExpectedPeer {
                device_id: responder_id,
                public_key: responder_identity.public_key(),
            },
        );
        let (responder_state, server_hello) = accept_hello(
            &responder_identity,
            responder_id,
            hello,
            ExpectedPeer {
                device_id: initiator_id,
                public_key: initiator_identity.public_key(),
            },
        )
        .unwrap();
        let mut initiator = finish_initiator(initiator_state, server_hello).unwrap();
        let mut responder = finish_responder(responder_state).unwrap();

        assert_eq!(initiator.peer_device_id(), responder_id);
        assert_eq!(responder.peer_device_id(), initiator_id);
        assert_eq!(initiator.transcript_hash(), responder.transcript_hash());

        let record = initiator.seal(b"control", b"move-pointer").unwrap();
        assert_eq!(
            responder.open(&record, b"control").unwrap(),
            b"move-pointer"
        );
    }

    #[test]
    fn altered_client_signature_is_rejected() {
        let identity = IdentityKeypair::from_seed([3u8; 32]);
        let other = IdentityKeypair::from_seed([4u8; 32]);
        let (_, mut hello) = start_initiator(
            &identity,
            DeviceId::new(111_222_333).unwrap(),
            ExpectedPeer {
                device_id: DeviceId::new(444_555_666).unwrap(),
                public_key: other.public_key(),
            },
        );
        hello.signature[0] ^= 1;
        assert!(matches!(
            accept_hello(
                &other,
                DeviceId::new(444_555_666).unwrap(),
                hello,
                ExpectedPeer {
                    device_id: DeviceId::new(111_222_333).unwrap(),
                    public_key: identity.public_key()
                }
            ),
            Err(SessionError::Crypto(CryptoError::SignatureVerification))
        ));
    }

    #[test]
    fn unpinned_first_connection_still_authenticates_both_signed_identities() {
        let initiator_identity = IdentityKeypair::from_seed([5u8; 32]);
        let responder_identity = IdentityKeypair::from_seed([6u8; 32]);
        let initiator_id = DeviceId::new(111_222_334).unwrap();
        let responder_id = DeviceId::new(444_555_667).unwrap();
        let (initiator_state, hello) = start_initiator(
            &initiator_identity,
            initiator_id,
            ExpectedPeer {
                device_id: responder_id,
                public_key: [0; 32],
            },
        );
        let (responder_state, server_hello) =
            accept_hello_unpinned(&responder_identity, responder_id, hello).unwrap();
        let initiator = finish_initiator(initiator_state, server_hello).unwrap();
        let responder = finish_responder(responder_state).unwrap();
        assert_eq!(initiator.peer_device_id(), responder_id);
        assert_eq!(
            initiator.peer_identity_key(),
            responder_identity.public_key()
        );
        assert_eq!(responder.peer_device_id(), initiator_id);
        assert_eq!(
            responder.peer_identity_key(),
            initiator_identity.public_key()
        );
        assert_eq!(initiator.transcript_hash(), responder.transcript_hash());
    }
}
