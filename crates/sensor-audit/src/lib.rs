//! Signed, hash-linked metadata log. Content is deliberately absent from the schema.
//! Detecting deletion of the tail or entire log needs a separately trusted head.
use sensor_core::DeviceId;
use sensor_crypto::IdentityKeypair;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Seek, SeekFrom, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

const MAX_LINE: usize = 4096;
const DOMAIN: &[u8] = b"SENSOR/audit/v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Action {
    ConnectionRequested,
    SessionAccepted,
    SessionRejected,
    SessionDisconnected,
    PermissionChanged,
    FileOffered,
    FileCompleted,
    FileCancelled,
    FileFailed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    Success,
    Denied,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub actor: DeviceId,
    pub target: DeviceId,
    pub action: Action,
    pub outcome: Outcome,
    pub bytes: u64,
}

#[derive(Serialize, Deserialize)]
struct Body {
    version: u16,
    sequence: u64,
    timestamp_ms: u64,
    previous: [u8; 32],
    event: Event,
}
#[derive(Serialize, Deserialize)]
struct Record {
    body: Body,
    signature: Vec<u8>,
}

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq)]
pub struct AuditHead {
    pub records: u64,
    pub hash: [u8; 32],
}

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("audit I/O: {0}")]
    Io(#[from] io::Error),
    #[error("audit serialization: {0}")]
    Json(#[from] serde_json::Error),
    #[error("audit chain or signature is invalid")]
    Invalid,
}

fn signable(body: &Body) -> Result<Vec<u8>, AuditError> {
    let mut bytes = DOMAIN.to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(body)?);
    Ok(bytes)
}
fn record_hash(record: &Record) -> Result<[u8; 32], AuditError> {
    let mut hash = Sha256::new();
    hash.update(signable(&record.body)?);
    hash.update(&record.signature);
    Ok(hash.finalize().into())
}

fn verify(mut reader: impl BufRead, signer: &[u8; 32]) -> Result<AuditHead, AuditError> {
    let mut head = AuditHead::default();
    loop {
        let mut line = Vec::new();
        // Bound each allocation even if a damaged log has no newline.
        loop {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                break;
            }
            let length = available
                .iter()
                .position(|b| *b == b'\n')
                .map_or(available.len(), |n| n + 1);
            if line.len() + length > MAX_LINE {
                return Err(AuditError::Invalid);
            }
            line.extend_from_slice(&available[..length]);
            reader.consume(length);
            if line.last() == Some(&b'\n') {
                break;
            }
        }
        if line.is_empty() {
            break;
        }
        if line.last() != Some(&b'\n') {
            return Err(AuditError::Invalid);
        }
        let record: Record = serde_json::from_slice(&line)?;
        if record.body.version != 1
            || record.body.sequence != head.records
            || record.body.previous != head.hash
        {
            return Err(AuditError::Invalid);
        }
        IdentityKeypair::verify(signer, &signable(&record.body)?, &record.signature)
            .map_err(|_| AuditError::Invalid)?;
        head = AuditHead {
            records: head.records.checked_add(1).ok_or(AuditError::Invalid)?,
            hash: record_hash(&record)?,
        };
    }
    Ok(head)
}

pub fn verify_file(path: &Path, signer: &[u8; 32]) -> Result<AuditHead, AuditError> {
    verify(BufReader::new(File::open(path)?), signer)
}

pub struct AuditLog {
    file: File,
    identity: IdentityKeypair,
    head: AuditHead,
    failed: bool,
}

impl AuditLog {
    pub fn open(path: &Path, identity: &IdentityKeypair) -> Result<Self, AuditError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        fs2::FileExt::try_lock_exclusive(&file)?;
        let head = verify(BufReader::new(&file), &identity.public_key())?;
        file.seek(SeekFrom::End(0))?;
        Ok(Self {
            file,
            identity: identity.clone(),
            head,
            failed: false,
        })
    }
    pub fn append(&mut self, event: Event) -> Result<AuditHead, AuditError> {
        if self.failed {
            return Err(AuditError::Invalid);
        }
        let next = self
            .head
            .records
            .checked_add(1)
            .ok_or(AuditError::Invalid)?;
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuditError::Invalid)?
            .as_millis()
            .try_into()
            .map_err(|_| AuditError::Invalid)?;
        let body = Body {
            version: 1,
            sequence: self.head.records,
            timestamp_ms,
            previous: self.head.hash,
            event,
        };
        let signature = self.identity.sign(&signable(&body)?);
        let record = Record { body, signature };
        let mut bytes = serde_json::to_vec(&record)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_LINE {
            return Err(AuditError::Invalid);
        }
        let hash = record_hash(&record)?;
        if let Err(error) = self
            .file
            .write_all(&bytes)
            .and_then(|_| self.file.sync_data())
        {
            self.failed = true;
            return Err(error.into());
        }
        self.head = AuditHead {
            records: next,
            hash,
        };
        Ok(self.head)
    }
    pub fn head(&self) -> AuditHead {
        self.head
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event() -> Event {
        Event {
            actor: DeviceId::new(123_456_789).unwrap(),
            target: DeviceId::new(987_654_321).unwrap(),
            action: Action::SessionAccepted,
            outcome: Outcome::Success,
            bytes: 0,
        }
    }
    #[test]
    fn signed_log_reopens_and_rejects_wrong_signer_and_tampering() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("audit.jsonl");
        let identity = IdentityKeypair::generate();
        {
            let mut log = AuditLog::open(&path, &identity).unwrap();
            log.append(event()).unwrap();
        }
        {
            let mut log = AuditLog::open(&path, &identity).unwrap();
            assert_eq!(log.append(event()).unwrap().records, 2);
        }
        assert_eq!(
            verify_file(&path, &identity.public_key()).unwrap().records,
            2
        );
        assert!(verify_file(&path, &IdentityKeypair::generate().public_key()).is_err());
        let data = std::fs::read_to_string(&path)
            .unwrap()
            .replace("SessionAccepted", "SessionRejected");
        std::fs::write(&path, data).unwrap();
        assert!(AuditLog::open(&path, &identity).is_err());
    }
    #[test]
    fn partial_tail_and_removed_middle_record_are_detected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("audit.jsonl");
        let identity = IdentityKeypair::generate();
        {
            let mut log = AuditLog::open(&path, &identity).unwrap();
            for _ in 0..3 {
                log.append(event()).unwrap();
            }
        }
        let data = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = data.lines().collect();
        std::fs::write(&path, format!("{}\n{}\n", lines[0], lines[2])).unwrap();
        assert!(verify_file(&path, &identity.public_key()).is_err());
        std::fs::write(&path, &data[..data.len() - 1]).unwrap();
        assert!(verify_file(&path, &identity.public_key()).is_err());
    }
}
