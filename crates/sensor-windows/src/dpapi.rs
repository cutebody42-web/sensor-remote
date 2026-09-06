use sensor_identity::{IdentityError, KeyProtector};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    },
};
use zeroize::Zeroize;

/// Windows current-user DPAPI; never uses machine-wide decryption scope.
/// A future service must own its own service-account identity and migration.
#[derive(Clone, Copy, Default)]
pub struct UserDpapi;

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, IdentityError> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| IdentityError::Malformed)?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

struct OwnedBlob(CRYPT_INTEGER_BLOB);
impl Drop for OwnedBlob {
    fn drop(&mut self) {
        if !self.0.pbData.is_null() {
            // SAFETY: DPAPI allocated this buffer with cbData writable bytes.
            // Wipe OS-owned plaintext as well as the Rust copy; free exactly once.
            unsafe {
                std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize).zeroize();
                LocalFree(self.0.pbData.cast());
            }
        }
    }
}

impl UserDpapi {
    fn transform(
        &self,
        input: &[u8],
        context: &[u8],
        decrypt: bool,
    ) -> Result<Vec<u8>, IdentityError> {
        let input = blob(input)?;
        let entropy = blob(context)?;
        let mut output = OwnedBlob(CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        });
        // SAFETY: input/entropy slices remain alive and valid throughout the call.
        // No UI, description allocation, prompt struct or reserved data is used.
        let result = unsafe {
            if decrypt {
                CryptUnprotectData(
                    &input,
                    std::ptr::null_mut(),
                    &entropy,
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )
            } else {
                CryptProtectData(
                    &input,
                    std::ptr::null(),
                    &entropy,
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )
            }
        };
        if result == 0 {
            return Err(IdentityError::Protection(format!(
                "Windows DPAPI: {}",
                std::io::Error::last_os_error()
            )));
        }
        if output.0.pbData.is_null() {
            return Err(IdentityError::Malformed);
        }
        // SAFETY: successful DPAPI call returned cbData readable bytes owned by output.
        Ok(
            unsafe { std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize) }
                .to_vec(),
        )
    }
}

impl KeyProtector for UserDpapi {
    fn protect(&self, plaintext: &[u8], context: &[u8]) -> Result<Vec<u8>, IdentityError> {
        self.transform(plaintext, context, false)
    }
    fn unprotect(&self, ciphertext: &[u8], context: &[u8]) -> Result<Vec<u8>, IdentityError> {
        self.transform(ciphertext, context, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sensor_identity::IdentityFileStore;

    #[test]
    fn actual_windows_dpapi_rejects_wrong_context_and_corruption() {
        let ciphertext = UserDpapi
            .protect(b"private identity seed", b"device metadata")
            .unwrap();
        assert_eq!(
            UserDpapi
                .unprotect(&ciphertext, b"device metadata")
                .unwrap(),
            b"private identity seed"
        );
        assert!(UserDpapi
            .unprotect(&ciphertext, b"changed metadata")
            .is_err());
        let mut corrupt = ciphertext;
        let index = corrupt.len() / 2;
        corrupt[index] ^= 1;
        assert!(UserDpapi.unprotect(&corrupt, b"device metadata").is_err());
    }

    #[test]
    fn identity_survives_reopen_and_atomic_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.bin");
        let store = IdentityFileStore::new(&path, UserDpapi);
        let mut original = store.load_or_create().unwrap();
        original.set_alias(Some("workstation@sensor")).unwrap();
        store.save(&original).unwrap();
        let loaded = IdentityFileStore::new(path, UserDpapi)
            .load_or_create()
            .unwrap();
        assert_eq!(loaded.device_id(), original.device_id());
        assert_eq!(
            loaded.keypair().public_key(),
            original.keypair().public_key()
        );
        assert_eq!(loaded.alias(), original.alias());
        assert!(!std::fs::read(store.path())
            .unwrap()
            .windows(32)
            .any(|bytes| bytes == original.keypair().seed()));
    }

    #[test]
    fn concurrent_initialization_keeps_one_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.bin");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    IdentityFileStore::new(path, UserDpapi)
                        .load_or_create()
                        .unwrap()
                        .keypair()
                        .public_key()
                })
            })
            .collect();
        let keys: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(keys.iter().all(|key| key == &keys[0]));
    }
}
