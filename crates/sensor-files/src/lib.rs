//! Capability-confined file reception, bounded chunks, resume and integrity checks.
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use rand_core::{OsRng, RngCore};
use sensor_session::permissions::{Consent, Permission, PermissionError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
use thiserror::Error;

pub const CHUNK_SIZE: usize = 64 * 1024;
pub const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024 * 1024;
const METADATA_LIMIT: u64 = 4096;

#[derive(Debug, Error)]
pub enum FileError {
    #[error("file permission: {0}")]
    Permission(#[from] PermissionError),
    #[error("file I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid file or transfer metadata")]
    Invalid,
    #[error("chunk offset or length is invalid")]
    InvalidChunk,
    #[error("file integrity verification failed")]
    Integrity,
    #[error("transfer belongs to another peer")]
    WrongPeer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

fn valid_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 180 || name.starts_with('.') || name.ends_with(['.', ' ']) {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_control() || ['/', '\\', ':', '<', '>', '"', '|', '?', '*'].contains(&c))
    {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ![
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "COM¹",
        "COM²", "COM³", "LPT¹", "LPT²", "LPT³",
    ]
    .contains(&stem.as_str())
}

fn hash_reader(reader: &mut impl Read) -> Result<[u8; 32], FileError> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0; CHUNK_SIZE];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().into())
}

pub fn describe_file(path: &Path) -> Result<Manifest, FileError> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or(FileError::Invalid)?
        .to_owned();
    if !valid_name(&name) {
        return Err(FileError::Invalid);
    }
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_SIZE {
        return Err(FileError::Invalid);
    }
    Ok(Manifest {
        name,
        size: metadata.len(),
        sha256: hash_reader(&mut file)?,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TransferId(pub [u8; 16]);
impl TransferId {
    fn name(self, extension: &str) -> String {
        let id: String = self.0.iter().map(|b| format!("{b:02x}")).collect();
        format!(".sensor-transfer-{id}.{extension}")
    }
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    version: u16,
    owner: [u8; 32],
    manifest: Manifest,
}

/// Root is selected locally. Peer paths never gain ambient filesystem access.
pub struct Receiver {
    directory: Dir,
}

impl Receiver {
    pub fn open(root: &Path) -> Result<Self, FileError> {
        Ok(Self {
            directory: Dir::open_ambient_dir(root, ambient_authority())?,
        })
    }

    pub fn begin(
        &self,
        consent: &Consent,
        owner: [u8; 32],
        manifest: Manifest,
    ) -> Result<IncomingTransfer, FileError> {
        consent.require(Permission::FileManager)?;
        if !valid_name(&manifest.name) || manifest.size > MAX_FILE_SIZE {
            return Err(FileError::Invalid);
        }
        match self.directory.symlink_metadata(&manifest.name) {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "destination exists",
                )
                .into())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        let mut bytes = [0; 16];
        OsRng.fill_bytes(&mut bytes);
        let id = TransferId(bytes);
        let metadata = Metadata {
            version: 1,
            owner,
            manifest,
        };
        let encoded = postcard::to_allocvec(&metadata).map_err(|_| FileError::Invalid)?;
        let mut options = OpenOptions::new();
        options.create_new(true).read(true).write(true);
        let file = self
            .directory
            .open_with(id.name("part"), &options)?
            .into_std();
        fs2::FileExt::try_lock_exclusive(&file)?;
        let mut created_metadata = false;
        let write_metadata = (|| -> Result<(), FileError> {
            let mut record = self.directory.open_with(id.name("meta"), &options)?;
            created_metadata = true;
            record.write_all(&encoded)?;
            record.sync_all()?;
            Ok(())
        })();
        if let Err(error) = write_metadata {
            drop(file);
            let _ = self.directory.remove_file(id.name("part"));
            if created_metadata {
                let _ = self.directory.remove_file(id.name("meta"));
            }
            return Err(error);
        }
        Ok(IncomingTransfer {
            directory: self.directory.try_clone()?,
            file,
            metadata,
            id,
            offset: 0,
        })
    }

    pub fn resume(
        &self,
        consent: &Consent,
        owner: [u8; 32],
        id: TransferId,
    ) -> Result<IncomingTransfer, FileError> {
        consent.require(Permission::FileManager)?;
        let mut encoded = Vec::new();
        self.directory
            .open(id.name("meta"))?
            .take(METADATA_LIMIT + 1)
            .read_to_end(&mut encoded)?;
        if encoded.len() as u64 > METADATA_LIMIT {
            return Err(FileError::Invalid);
        }
        let (metadata, trailing): (Metadata, _) =
            postcard::take_from_bytes(&encoded).map_err(|_| FileError::Invalid)?;
        if !trailing.is_empty()
            || metadata.version != 1
            || !valid_name(&metadata.manifest.name)
            || metadata.manifest.size > MAX_FILE_SIZE
        {
            return Err(FileError::Invalid);
        }
        if metadata.owner != owner {
            return Err(FileError::WrongPeer);
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        let mut file = self
            .directory
            .open_with(id.name("part"), &options)?
            .into_std();
        fs2::FileExt::try_lock_exclusive(&file)?;
        let offset = file.metadata()?.len();
        if offset > metadata.manifest.size {
            return Err(FileError::Integrity);
        }
        file.seek(SeekFrom::Start(offset))?;
        Ok(IncomingTransfer {
            directory: self.directory.try_clone()?,
            file,
            metadata,
            id,
            offset,
        })
    }
}

/// A dropped transfer retains its explicitly named partial files for resume.
pub struct IncomingTransfer {
    directory: Dir,
    file: File,
    metadata: Metadata,
    id: TransferId,
    offset: u64,
}

impl IncomingTransfer {
    pub fn id(&self) -> TransferId {
        self.id
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn manifest(&self) -> &Manifest {
        &self.metadata.manifest
    }
    pub fn prefix_hash(&mut self) -> Result<[u8; 32], FileError> {
        self.file.seek(SeekFrom::Start(0))?;
        let hash = hash_reader(&mut self.file)?;
        self.file.seek(SeekFrom::Start(self.offset))?;
        Ok(hash)
    }
    pub fn write_chunk(
        &mut self,
        consent: &Consent,
        offset: u64,
        bytes: &[u8],
        checksum: [u8; 32],
    ) -> Result<(), FileError> {
        consent.require(Permission::FileManager)?;
        if offset != self.offset
            || bytes.is_empty()
            || bytes.len() > CHUNK_SIZE
            || offset
                .checked_add(bytes.len() as u64)
                .is_none_or(|n| n > self.metadata.manifest.size)
        {
            return Err(FileError::InvalidChunk);
        }
        if <[u8; 32]>::from(Sha256::digest(bytes)) != checksum {
            return Err(FileError::Integrity);
        }
        self.file.write_all(bytes)?;
        self.offset += bytes.len() as u64;
        Ok(())
    }
    pub fn pause(self) -> Result<TransferId, FileError> {
        self.file.sync_all()?;
        Ok(self.id)
    }
    pub fn cancel(self, consent: &Consent) -> Result<(), FileError> {
        consent.require(Permission::FileManager)?;
        drop(self.file);
        self.directory.remove_file(self.id.name("part"))?;
        self.directory.remove_file(self.id.name("meta"))?;
        Ok(())
    }
    pub fn finish(mut self, consent: &Consent) -> Result<Manifest, FileError> {
        consent.require(Permission::FileManager)?;
        if self.offset != self.metadata.manifest.size
            || self.prefix_hash()? != self.metadata.manifest.sha256
        {
            return Err(FileError::Integrity);
        }
        self.file.sync_all()?;
        // Hard-link publication is atomic and fails if the target exists. All
        // paths are relative to the same directory capability and filesystem.
        self.directory.hard_link(
            self.id.name("part"),
            &self.directory,
            &self.metadata.manifest.name,
        )?;
        self.directory.remove_file(self.id.name("part"))?;
        self.directory.remove_file(self.id.name("meta"))?;
        drop(self.file);
        Ok(self.metadata.manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sensor_session::permissions::Permissions;
    fn consent() -> Consent {
        let mut consent = Consent::pending(Permissions::file_transfer());
        consent.accept(Permissions::file_transfer()).unwrap();
        consent
    }
    fn manifest(data: &[u8]) -> Manifest {
        Manifest {
            name: "payload.bin".into(),
            size: data.len() as u64,
            sha256: Sha256::digest(data).into(),
        }
    }

    #[test]
    fn resume_after_reopen_verifies_whole_file_and_preserves_existing_files() {
        let root = tempfile::tempdir().unwrap();
        let data = vec![23; CHUNK_SIZE * 3 + 173];
        let policy = consent();
        let owner = [9; 32];
        let receiver = Receiver::open(root.path()).unwrap();
        let mut upload = receiver.begin(&policy, owner, manifest(&data)).unwrap();
        upload
            .write_chunk(
                &policy,
                0,
                &data[..CHUNK_SIZE],
                Sha256::digest(&data[..CHUNK_SIZE]).into(),
            )
            .unwrap();
        let id = upload.pause().unwrap();
        drop(receiver);
        let receiver = Receiver::open(root.path()).unwrap();
        assert!(matches!(
            receiver.resume(&policy, [8; 32], id),
            Err(FileError::WrongPeer)
        ));
        let mut upload = receiver.resume(&policy, owner, id).unwrap();
        assert_eq!(upload.offset(), CHUNK_SIZE as u64);
        for chunk in data[CHUNK_SIZE..].chunks(CHUNK_SIZE) {
            upload
                .write_chunk(
                    &policy,
                    upload.offset(),
                    chunk,
                    Sha256::digest(chunk).into(),
                )
                .unwrap();
        }
        upload.finish(&policy).unwrap();
        assert_eq!(
            std::fs::read(root.path().join("payload.bin")).unwrap(),
            data
        );
        assert!(receiver.begin(&policy, owner, manifest(&data)).is_err());
    }
    #[test]
    fn traversal_devices_alternate_streams_and_unaccepted_writes_fail() {
        let root = tempfile::tempdir().unwrap();
        let receiver = Receiver::open(root.path()).unwrap();
        let pending = Consent::pending(Permissions::file_transfer());
        assert!(receiver.begin(&pending, [1; 32], manifest(b"hi")).is_err());
        for name in [
            "../out",
            "C:\\out",
            "\\\\host\\share",
            "a:secret",
            "CON.txt",
            "COM¹.txt",
            "LPT².txt",
            "a/b",
            "..",
            "trail.",
        ] {
            let mut offer = manifest(b"hi");
            offer.name = name.into();
            assert!(
                receiver.begin(&consent(), [1; 32], offer).is_err(),
                "{name}"
            );
        }
    }
    #[test]
    fn corruption_bad_offsets_and_revocation_prevent_writes() {
        let root = tempfile::tempdir().unwrap();
        let receiver = Receiver::open(root.path()).unwrap();
        let mut policy = consent();
        let mut upload = receiver
            .begin(&policy, [1; 32], manifest(b"hello"))
            .unwrap();
        assert!(upload.write_chunk(&policy, 0, b"hello", [0; 32]).is_err());
        assert!(upload
            .write_chunk(&policy, 3, b"hello", Sha256::digest(b"hello").into())
            .is_err());
        policy.change_permissions(Permissions::NONE).unwrap();
        assert!(upload
            .write_chunk(&policy, 0, b"hello", Sha256::digest(b"hello").into())
            .is_err());
        assert!(!root.path().join("payload.bin").exists());
    }

    #[test]
    fn active_transfer_cannot_be_resumed_by_a_second_writer() {
        let root = tempfile::tempdir().unwrap();
        let receiver = Receiver::open(root.path()).unwrap();
        let policy = consent();
        let transfer = receiver
            .begin(&policy, [1; 32], manifest(b"hello"))
            .unwrap();
        let id = transfer.id();
        assert!(receiver.resume(&policy, [1; 32], id).is_err());
        drop(transfer);
        assert!(receiver.resume(&policy, [1; 32], id).is_ok());
    }
}
