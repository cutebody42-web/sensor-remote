//! Device identity lifecycle and persistence boundaries.
//!
//! The file store never writes a private key in plaintext. A platform
//! `KeyProtector` must be supplied by the host application; the Windows
//! implementation will be backed by DPAPI/credential isolation rather than a
//! user-entered password or a process environment variable.

use postcard::to_allocvec;
use rand_core::OsRng;
use sensor_core::{DeviceAlias, DeviceId};
use sensor_crypto::IdentityKeypair;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;
use zeroize::Zeroizing;

const IDENTITY_FILE_VERSION: u16 = 1;
const MAX_IDENTITY_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("identity serialization failed: {0}")]
    Serialization(#[from] postcard::Error),
    #[error("identity I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("key protection failed: {0}")]
    Protection(String),
    #[error("identity file is malformed")]
    Malformed,
    #[error("identity public key does not match protected private key")]
    KeyMismatch,
    #[error("invalid alias: {0}")]
    Alias(#[from] sensor_core::CoreError),
}

/// Platform boundary for protecting the identity seed and authenticating the
/// public metadata bound to it.
pub trait KeyProtector {
    fn protect(&self, plaintext: &[u8], context: &[u8]) -> Result<Vec<u8>, IdentityError>;
    fn unprotect(&self, ciphertext: &[u8], context: &[u8]) -> Result<Vec<u8>, IdentityError>;
}

#[derive(Clone)]
pub struct DeviceIdentity {
    device_id: DeviceId,
    alias: Option<DeviceAlias>,
    namespace: Option<DeviceAlias>,
    keypair: IdentityKeypair,
}

impl DeviceIdentity {
    pub fn generate() -> Self {
        let mut rng = OsRng;
        Self {
            device_id: DeviceId::generate(&mut rng),
            alias: None,
            namespace: None,
            keypair: IdentityKeypair::generate(),
        }
    }

    pub fn from_parts(
        device_id: DeviceId,
        keypair: IdentityKeypair,
        alias: Option<DeviceAlias>,
        namespace: Option<DeviceAlias>,
    ) -> Self {
        Self {
            device_id,
            alias,
            namespace,
            keypair,
        }
    }

    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    pub fn alias(&self) -> Option<&DeviceAlias> {
        self.alias.as_ref()
    }

    pub fn namespace(&self) -> Option<&DeviceAlias> {
        self.namespace.as_ref()
    }

    pub fn keypair(&self) -> &IdentityKeypair {
        &self.keypair
    }

    pub fn set_alias(&mut self, alias: Option<impl Into<String>>) -> Result<(), IdentityError> {
        self.alias = alias.map(DeviceAlias::new).transpose()?;
        Ok(())
    }

    pub fn set_namespace(
        &mut self,
        namespace: Option<impl Into<String>>,
    ) -> Result<(), IdentityError> {
        self.namespace = namespace.map(DeviceAlias::new).transpose()?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedIdentity {
    version: u16,
    device_id: DeviceId,
    alias: Option<DeviceAlias>,
    namespace: Option<DeviceAlias>,
    public_key: [u8; 32],
    protected_seed: Vec<u8>,
}

#[derive(Serialize)]
struct ProtectionContext<'a> {
    version: u16,
    device_id: DeviceId,
    alias: &'a Option<DeviceAlias>,
    namespace: &'a Option<DeviceAlias>,
    public_key: &'a [u8; 32],
}

fn protection_context(
    version: u16,
    device_id: DeviceId,
    alias: &Option<DeviceAlias>,
    namespace: &Option<DeviceAlias>,
    public_key: &[u8; 32],
) -> Result<Vec<u8>, IdentityError> {
    Ok(to_allocvec(&ProtectionContext {
        version,
        device_id,
        alias,
        namespace,
        public_key,
    })?)
}

/// Durable identity store with an explicit OS-secret integration point.
pub struct IdentityFileStore<P> {
    path: PathBuf,
    protector: P,
}

impl<P: KeyProtector> IdentityFileStore<P> {
    pub fn new(path: impl Into<PathBuf>, protector: P) -> Self {
        Self {
            path: path.into(),
            protector,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_or_create(&self) -> Result<DeviceIdentity, IdentityError> {
        let _guard = self.lock()?;
        match self.load() {
            Ok(identity) => Ok(identity),
            Err(IdentityError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                let identity = DeviceIdentity::generate();
                self.save_locked(&identity)?;
                Ok(identity)
            }
            Err(error) => Err(error),
        }
    }

    fn parent(&self) -> &Path {
        self.path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    }

    fn lock(&self) -> Result<File, IdentityError> {
        std::fs::create_dir_all(self.parent())?;
        let mut lock_path = self.path.as_os_str().to_owned();
        lock_path.push(".lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(file)
    }

    pub fn load(&self) -> Result<DeviceIdentity, IdentityError> {
        let mut bytes = Vec::new();
        File::open(&self.path)?
            .take((MAX_IDENTITY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_IDENTITY_BYTES {
            return Err(IdentityError::Malformed);
        }
        let (persisted, trailing): (PersistedIdentity, _) = postcard::take_from_bytes(&bytes)?;
        if !trailing.is_empty() {
            return Err(IdentityError::Malformed);
        }
        if persisted.version != IDENTITY_FILE_VERSION || persisted.protected_seed.is_empty() {
            return Err(IdentityError::Malformed);
        }
        let context = protection_context(
            persisted.version,
            persisted.device_id,
            &persisted.alias,
            &persisted.namespace,
            &persisted.public_key,
        )?;
        let seed = Zeroizing::new(
            self.protector
                .unprotect(&persisted.protected_seed, &context)?,
        );
        let seed: [u8; 32] = seed
            .as_slice()
            .try_into()
            .map_err(|_| IdentityError::Malformed)?;
        let keypair = IdentityKeypair::from_seed(seed);
        if keypair.public_key() != persisted.public_key {
            return Err(IdentityError::KeyMismatch);
        }
        Ok(DeviceIdentity::from_parts(
            persisted.device_id,
            keypair,
            persisted.alias,
            persisted.namespace,
        ))
    }

    pub fn save(&self, identity: &DeviceIdentity) -> Result<(), IdentityError> {
        let _guard = self.lock()?;
        self.save_locked(identity)
    }

    fn save_locked(&self, identity: &DeviceIdentity) -> Result<(), IdentityError> {
        let public_key = identity.keypair.public_key();
        let context = protection_context(
            IDENTITY_FILE_VERSION,
            identity.device_id,
            &identity.alias,
            &identity.namespace,
            &public_key,
        )?;
        let seed = Zeroizing::new(identity.keypair.seed());
        let persisted = PersistedIdentity {
            version: IDENTITY_FILE_VERSION,
            device_id: identity.device_id,
            alias: identity.alias.clone(),
            namespace: identity.namespace.clone(),
            public_key,
            protected_seed: self.protector.protect(&*seed, &context)?,
        };
        let bytes = to_allocvec(&persisted)?;
        if bytes.len() > MAX_IDENTITY_BYTES {
            return Err(IdentityError::Malformed);
        }
        let mut temporary = tempfile::NamedTempFile::new_in(self.parent())?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&self.path)
            .map_err(|e| IdentityError::Io(e.error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestProtector;

    impl KeyProtector for TestProtector {
        fn protect(&self, plaintext: &[u8], context: &[u8]) -> Result<Vec<u8>, IdentityError> {
            let mut out = context.to_vec();
            out.extend(plaintext.iter().rev());
            Ok(out)
        }

        fn unprotect(&self, ciphertext: &[u8], context: &[u8]) -> Result<Vec<u8>, IdentityError> {
            let mut out = ciphertext
                .strip_prefix(context)
                .ok_or_else(|| IdentityError::Protection("metadata authentication failed".into()))?
                .to_vec();
            out.reverse();
            Ok(out)
        }
    }

    #[test]
    fn identity_round_trips_without_plaintext_seed() {
        let path = std::env::temp_dir().join(format!("sensor-identity-{}.bin", std::process::id()));
        let store = IdentityFileStore::new(&path, TestProtector);
        let mut identity = DeviceIdentity::generate();
        identity.set_alias(Some("lab-pc")).unwrap();
        store.save(&identity).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.device_id(), identity.device_id());
        assert_eq!(loaded.alias().unwrap().as_str(), "lab-pc");
        let seed = identity.keypair().seed();
        assert!(!std::fs::read(&path)
            .unwrap()
            .windows(32)
            .any(|w| w == seed.as_slice()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn identity_metadata_is_bound_to_the_protected_seed() {
        let path = std::env::temp_dir().join(format!(
            "sensor-identity-metadata-{}.bin",
            std::process::id()
        ));
        let store = IdentityFileStore::new(&path, TestProtector);
        let identity = DeviceIdentity::generate();
        store.save(&identity).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let mut persisted: PersistedIdentity = postcard::from_bytes(&bytes).unwrap();
        persisted.alias = Some(DeviceAlias::new("tampered").unwrap());
        std::fs::write(&path, to_allocvec(&persisted).unwrap()).unwrap();

        assert!(matches!(store.load(), Err(IdentityError::Protection(_))));
        let _ = std::fs::remove_file(path);
    }
}
