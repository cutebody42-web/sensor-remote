//! Cryptographic building blocks for the SENSOR session protocol.
//!
//! This crate deliberately exposes composed primitives rather than allowing
//! callers to assemble ad-hoc cryptography. Session negotiation and identity
//! policy remain in higher-level crates.

use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use thiserror::Error;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const SESSION_INFO: &[u8] = b"SENSOR Remote Access/session/v1";
const INITIATOR_TO_RESPONDER_INFO: &[u8] = b"SENSOR Remote Access/session/v1/i2r";
const RESPONDER_TO_INITIATOR_INFO: &[u8] = b"SENSOR Remote Access/session/v1/r2i";

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid Ed25519 public key")]
    InvalidPublicKey,
    #[error("invalid Ed25519 signature")]
    InvalidSignature,
    #[error("invalid or non-contributory X25519 peer key")]
    InvalidPeerKey,
    #[error("signature verification failed")]
    SignatureVerification,
    #[error("key derivation failed")]
    KeyDerivation,
    #[error("authenticated encryption failed")]
    Encryption,
    #[error("replayed or out-of-order record: {0}")]
    Replay(u64),
    #[error("record sequence overflow")]
    SequenceOverflow,
}

/// Long-term Ed25519 identity key used to authenticate a device.
#[derive(Clone)]
pub struct IdentityKeypair(SigningKey);

impl IdentityKeypair {
    pub fn generate() -> Self {
        Self(SigningKey::generate(&mut OsRng))
    }

    pub fn from_seed(mut seed: [u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(&seed);
        seed.zeroize();
        Self(signing_key)
    }

    pub fn seed(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.0.verifying_key().to_bytes()
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.0.sign(message).to_bytes().to_vec()
    }

    pub fn verify(
        public_key: &[u8; 32],
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), CryptoError> {
        let verifying_key =
            VerifyingKey::from_bytes(public_key).map_err(|_| CryptoError::InvalidPublicKey)?;
        let signature =
            Signature::try_from(signature).map_err(|_| CryptoError::InvalidSignature)?;
        verifying_key
            .verify_strict(message, &signature)
            .map_err(|_| CryptoError::SignatureVerification)
    }
}

/// Ephemeral X25519 key used for forward-secret session establishment.
pub struct EphemeralKeypair(StaticSecret);

impl EphemeralKeypair {
    pub fn generate() -> Self {
        Self(StaticSecret::random_from_rng(OsRng))
    }

    pub fn public_key(&self) -> [u8; 32] {
        X25519PublicKey::from(&self.0).to_bytes()
    }

    pub fn diffie_hellman(&self, peer_public_key: &[u8; 32]) -> Result<[u8; 32], CryptoError> {
        let shared = self
            .0
            .diffie_hellman(&X25519PublicKey::from(*peer_public_key));
        if !shared.was_contributory() {
            return Err(CryptoError::InvalidPeerKey);
        }
        Ok(shared.to_bytes())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Zeroize, ZeroizeOnDrop)]
pub struct SessionKeys {
    pub initiator_to_responder: [u8; 32],
    pub responder_to_initiator: [u8; 32],
}

pub fn transcript_hash(transcript: &[u8]) -> [u8; 32] {
    Sha256::digest(transcript).into()
}

pub fn derive_session_keys(
    shared_secret: [u8; 32],
    transcript: &[u8],
) -> Result<SessionKeys, CryptoError> {
    let transcript_hash = transcript_hash(transcript);
    let shared_secret = Zeroizing::new(shared_secret);
    let hkdf = Hkdf::<Sha256>::new(Some(&transcript_hash), &*shared_secret);
    let mut session_prk = Zeroizing::new([0u8; 32]);
    hkdf.expand(SESSION_INFO, &mut *session_prk)
        .map_err(|_| CryptoError::KeyDerivation)?;

    let session_hkdf =
        Hkdf::<Sha256>::from_prk(&session_prk[..]).map_err(|_| CryptoError::KeyDerivation)?;
    let mut initiator_to_responder = [0u8; 32];
    let mut responder_to_initiator = [0u8; 32];
    session_hkdf
        .expand(INITIATOR_TO_RESPONDER_INFO, &mut initiator_to_responder)
        .map_err(|_| CryptoError::KeyDerivation)?;
    session_hkdf
        .expand(RESPONDER_TO_INITIATOR_INFO, &mut responder_to_initiator)
        .map_err(|_| CryptoError::KeyDerivation)?;

    Ok(SessionKeys {
        initiator_to_responder,
        responder_to_initiator,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedRecord {
    pub sequence: u64,
    pub ciphertext: Vec<u8>,
}

struct AeadCipher(ChaCha20Poly1305);

impl AeadCipher {
    fn new(key: &[u8; 32]) -> Self {
        Self(ChaCha20Poly1305::new_from_slice(key).expect("ChaCha20-Poly1305 accepts 32-byte keys"))
    }

    fn encrypt(&self, sequence: u64, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let nonce = make_nonce(sequence);
        self.0
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::Encryption)
    }

    fn decrypt(
        &self,
        sequence: u64,
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let nonce = make_nonce(sequence);
        self.0
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::Encryption)
    }
}

fn make_nonce(sequence: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(b"SNR1");
    nonce[4..].copy_from_slice(&sequence.to_be_bytes());
    nonce
}

/// A reliable ordered secure channel. Datagram transports must add a replay
/// window before using this abstraction for out-of-order delivery.
pub struct SecureChannel {
    send: AeadCipher,
    receive: AeadCipher,
    next_send_sequence: u64,
    highest_received: Option<u64>,
}

impl SecureChannel {
    pub fn initiator(keys: &SessionKeys) -> Self {
        Self {
            send: AeadCipher::new(&keys.initiator_to_responder),
            receive: AeadCipher::new(&keys.responder_to_initiator),
            next_send_sequence: 0,
            highest_received: None,
        }
    }

    pub fn responder(keys: &SessionKeys) -> Self {
        Self {
            send: AeadCipher::new(&keys.responder_to_initiator),
            receive: AeadCipher::new(&keys.initiator_to_responder),
            next_send_sequence: 0,
            highest_received: None,
        }
    }

    pub fn seal(&mut self, aad: &[u8], plaintext: &[u8]) -> Result<EncryptedRecord, CryptoError> {
        let sequence = self.next_send_sequence;
        self.next_send_sequence = self
            .next_send_sequence
            .checked_add(1)
            .ok_or(CryptoError::SequenceOverflow)?;
        Ok(EncryptedRecord {
            sequence,
            ciphertext: self.send.encrypt(sequence, aad, plaintext)?,
        })
    }

    pub fn open(&mut self, record: &EncryptedRecord, aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if self
            .highest_received
            .is_some_and(|highest| record.sequence <= highest)
        {
            return Err(CryptoError::Replay(record.sequence));
        }
        let plaintext = self
            .receive
            .decrypt(record.sequence, aad, &record.ciphertext)?;
        self.highest_received = Some(record.sequence);
        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keys_are_deterministic_for_same_transcript() {
        let left = derive_session_keys([7u8; 32], b"transcript").unwrap();
        let right = derive_session_keys([7u8; 32], b"transcript").unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn secure_channel_authenticates_and_rejects_replay() {
        let keys = derive_session_keys([3u8; 32], b"handshake").unwrap();
        let mut initiator = SecureChannel::initiator(&keys);
        let mut responder = SecureChannel::responder(&keys);

        let record = initiator.seal(b"control", b"hello").unwrap();
        assert_eq!(responder.open(&record, b"control").unwrap(), b"hello");
        assert!(matches!(
            responder.open(&record, b"control"),
            Err(CryptoError::Replay(0))
        ));
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let keys = derive_session_keys([9u8; 32], b"handshake").unwrap();
        let mut initiator = SecureChannel::initiator(&keys);
        let mut responder = SecureChannel::responder(&keys);
        let mut record = initiator.seal(b"control", b"hello").unwrap();
        record.ciphertext[0] ^= 1;
        assert!(matches!(
            responder.open(&record, b"control"),
            Err(CryptoError::Encryption)
        ));
    }

    #[test]
    fn non_contributory_peer_key_is_rejected() {
        let ephemeral = EphemeralKeypair::generate();
        assert!(matches!(
            ephemeral.diffie_hellman(&[0u8; 32]),
            Err(CryptoError::InvalidPeerKey)
        ));
    }
}
