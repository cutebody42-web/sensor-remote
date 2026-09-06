//! Endpoint operation handling. Network authentication precedes local consent.
use sensor_audit::{Action, AuditError, AuditLog, Event, Outcome};
use sensor_core::DeviceId;
use sensor_files::{
    describe_file, FileError, IncomingTransfer, Manifest, Receiver, TransferId, CHUNK_SIZE,
};
use sensor_session::{
    permissions::{Consent, Permission, PermissionError, Permissions},
    ExpectedPeer,
};
use sensor_transport::connection::{ConnectionError, SecureConnection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::{Duration, Instant},
};
use thiserror::Error;

pub const MAX_CHAT_BYTES: usize = 8192;
pub const DEFAULT_RECEIVE_LIMIT: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Mode {
    Chat,
    FileTransfer,
}
impl Mode {
    pub fn permissions(self) -> Permissions {
        match self {
            Self::Chat => Permissions::of(&[Permission::Chat]),
            Self::FileTransfer => Permissions::file_transfer(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum Message {
    Request(Mode),
    Accepted(Permissions),
    Rejected,
    Chat(String),
    FileOffer(Manifest),
    Resume(TransferId),
    Ready {
        id: TransferId,
        offset: u64,
        prefix_hash: [u8; 32],
        manifest: Manifest,
    },
    Chunk {
        offset: u64,
        bytes: Vec<u8>,
        checksum: [u8; 32],
    },
    Progress(u64),
    Commit,
    Complete(Manifest),
    Pause,
    Paused(TransferId),
    Cancel,
    Cancelled,
    Close,
}

#[derive(Debug, Error)]
pub enum EndpointError {
    #[error("connection: {0}")]
    Connection(#[from] ConnectionError),
    #[error("permission: {0}")]
    Permission(#[from] PermissionError),
    #[error("file: {0}")]
    File(#[from] FileError),
    #[error("audit: {0}")]
    Audit(#[from] AuditError),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("endpoint rejected the session")]
    Rejected,
    #[error("unexpected or oversized operation")]
    Invalid,
}

/// Implemented by the visible local UI. Returning true is an explicit user action.
pub trait LocalInteraction {
    fn accept(&mut self, peer: ExpectedPeer, mode: Mode) -> bool;
    fn chat_reply(&mut self, text: &str) -> Option<String>;
    fn transfer_progress(&mut self, _received: u64, _total: u64) {}
}

fn audit(
    log: &mut AuditLog,
    peer: DeviceId,
    local: DeviceId,
    action: Action,
    outcome: Outcome,
    bytes: u64,
) -> Result<(), EndpointError> {
    log.append(Event {
        actor: peer,
        target: local,
        action,
        outcome,
        bytes,
    })?;
    Ok(())
}

pub fn serve(
    mut connection: SecureConnection,
    local: DeviceId,
    receiver: &Receiver,
    log: &mut AuditLog,
    interaction: &mut impl LocalInteraction,
) -> Result<(), EndpointError> {
    let peer = connection.peer();
    let request = connection.receive()?;
    let Message::Request(mode) = request else {
        return Err(EndpointError::Invalid);
    };
    audit(
        log,
        peer.device_id,
        local,
        Action::ConnectionRequested,
        Outcome::Success,
        0,
    )?;
    let mut consent = Consent::pending(mode.permissions());
    if !interaction.accept(peer, mode) {
        consent.reject()?;
        audit(
            log,
            peer.device_id,
            local,
            Action::SessionRejected,
            Outcome::Denied,
            0,
        )?;
        connection.send(&Message::Rejected)?;
        return Ok(());
    }
    consent.accept(mode.permissions())?;
    audit(
        log,
        peer.device_id,
        local,
        Action::SessionAccepted,
        Outcome::Success,
        0,
    )?;
    connection.send(&Message::Accepted(consent.granted()))?;
    let result = operate(&mut connection, &consent, local, receiver, log, interaction);
    consent.close();
    connection.close();
    audit(
        log,
        peer.device_id,
        local,
        Action::SessionDisconnected,
        if result.is_ok() {
            Outcome::Success
        } else {
            Outcome::Failed
        },
        0,
    )?;
    result
}

fn operate(
    connection: &mut SecureConnection,
    consent: &Consent,
    local: DeviceId,
    receiver: &Receiver,
    log: &mut AuditLog,
    interaction: &mut impl LocalInteraction,
) -> Result<(), EndpointError> {
    let peer = connection.peer();
    let mut incoming: Option<IncomingTransfer> = None;
    let mut operation_window = Instant::now();
    let mut operations = 0;
    loop {
        let message = connection.receive::<Message>()?;
        if operation_window.elapsed() > Duration::from_secs(60) {
            operation_window = Instant::now();
            operations = 0;
        }
        operations += 1;
        // Bound tiny-message floods while permitting >1 GiB/min of 64 KiB chunks.
        if operations > 100_000 {
            return Err(EndpointError::Invalid);
        }
        match message {
            Message::Close => return Ok(()),
            Message::Chat(text) => {
                consent.require(Permission::Chat)?;
                if text.len() > MAX_CHAT_BYTES {
                    return Err(EndpointError::Invalid);
                }
                let Some(reply) = interaction.chat_reply(&text) else {
                    connection.send(&Message::Close)?;
                    return Ok(());
                };
                if reply.len() > MAX_CHAT_BYTES {
                    return Err(EndpointError::Invalid);
                }
                connection.send(&Message::Chat(reply))?;
            }
            Message::FileOffer(manifest) => {
                consent.require(Permission::FileManager)?;
                if incoming.is_some() || manifest.size > DEFAULT_RECEIVE_LIMIT {
                    return Err(EndpointError::Invalid);
                }
                audit(
                    log,
                    peer.device_id,
                    local,
                    Action::FileOffered,
                    Outcome::Success,
                    manifest.size,
                )?;
                let mut transfer = receiver.begin(consent, peer.public_key, manifest)?;
                connection.send(&Message::Ready {
                    id: transfer.id(),
                    offset: transfer.offset(),
                    prefix_hash: transfer.prefix_hash()?,
                    manifest: transfer.manifest().clone(),
                })?;
                incoming = Some(transfer);
            }
            Message::Resume(id) => {
                consent.require(Permission::FileManager)?;
                if incoming.is_some() {
                    return Err(EndpointError::Invalid);
                }
                let mut transfer = receiver.resume(consent, peer.public_key, id)?;
                if transfer.manifest().size > DEFAULT_RECEIVE_LIMIT {
                    return Err(EndpointError::Invalid);
                }
                connection.send(&Message::Ready {
                    id,
                    offset: transfer.offset(),
                    prefix_hash: transfer.prefix_hash()?,
                    manifest: transfer.manifest().clone(),
                })?;
                incoming = Some(transfer);
            }
            Message::Chunk {
                offset,
                bytes,
                checksum,
            } => {
                consent.require(Permission::FileManager)?;
                let transfer = incoming.as_mut().ok_or(EndpointError::Invalid)?;
                transfer.write_chunk(consent, offset, &bytes, checksum)?;
                interaction.transfer_progress(transfer.offset(), transfer.manifest().size);
                connection.send(&Message::Progress(transfer.offset()))?;
            }
            Message::Commit => {
                consent.require(Permission::FileManager)?;
                let transfer = incoming.take().ok_or(EndpointError::Invalid)?;
                let manifest = transfer.finish(consent)?;
                audit(
                    log,
                    peer.device_id,
                    local,
                    Action::FileCompleted,
                    Outcome::Success,
                    manifest.size,
                )?;
                connection.send(&Message::Complete(manifest))?;
                return Ok(());
            }
            Message::Pause => {
                consent.require(Permission::FileManager)?;
                let id = incoming.take().ok_or(EndpointError::Invalid)?.pause()?;
                connection.send(&Message::Paused(id))?;
                return Ok(());
            }
            Message::Cancel => {
                consent.require(Permission::FileManager)?;
                incoming
                    .take()
                    .ok_or(EndpointError::Invalid)?
                    .cancel(consent)?;
                audit(
                    log,
                    peer.device_id,
                    local,
                    Action::FileCancelled,
                    Outcome::Success,
                    0,
                )?;
                connection.send(&Message::Cancelled)?;
                return Ok(());
            }
            _ => return Err(EndpointError::Invalid),
        }
    }
}

pub fn request(connection: &mut SecureConnection, mode: Mode) -> Result<(), EndpointError> {
    connection.send(&Message::Request(mode))?;
    match connection.receive()? {
        Message::Accepted(granted) if granted == mode.permissions() => Ok(()),
        Message::Rejected => Err(EndpointError::Rejected),
        _ => Err(EndpointError::Invalid),
    }
}

pub fn send_file(
    connection: &mut SecureConnection,
    path: &Path,
    resume: Option<TransferId>,
    mut progress: impl FnMut(TransferId, u64, u64),
) -> Result<Manifest, EndpointError> {
    let manifest = describe_file(path)?;
    request(connection, Mode::FileTransfer)?;
    match resume {
        Some(id) => connection.send(&Message::Resume(id))?,
        None => connection.send(&Message::FileOffer(manifest.clone()))?,
    }
    let Message::Ready {
        id,
        mut offset,
        prefix_hash,
        manifest: remote_manifest,
    } = connection.receive()?
    else {
        return Err(EndpointError::Invalid);
    };
    if manifest != remote_manifest || offset > manifest.size {
        return Err(EndpointError::Invalid);
    }
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; CHUNK_SIZE];
    let mut prefix_remaining = offset;
    while prefix_remaining > 0 {
        let length = usize::try_from(prefix_remaining.min(CHUNK_SIZE as u64))
            .map_err(|_| EndpointError::Invalid)?;
        file.read_exact(&mut buffer[..length])?;
        hash.update(&buffer[..length]);
        prefix_remaining -= length as u64;
    }
    if <[u8; 32]>::from(hash.finalize()) != prefix_hash {
        return Err(FileError::Integrity.into());
    }
    file.seek(SeekFrom::Start(offset))?;
    progress(id, offset, manifest.size);
    while offset < manifest.size {
        let length = ((manifest.size - offset).min(CHUNK_SIZE as u64)) as usize;
        file.read_exact(&mut buffer[..length])?;
        connection.send(&Message::Chunk {
            offset,
            bytes: buffer[..length].to_vec(),
            checksum: Sha256::digest(&buffer[..length]).into(),
        })?;
        offset += length as u64;
        match connection.receive()? {
            Message::Progress(value) if value == offset => (),
            _ => return Err(EndpointError::Invalid),
        }
        progress(id, offset, manifest.size);
    }
    connection.send(&Message::Commit)?;
    match connection.receive()? {
        Message::Complete(completed) if completed == manifest => Ok(completed),
        _ => Err(EndpointError::Invalid),
    }
}
