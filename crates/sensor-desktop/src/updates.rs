//! Publisher-signed, explicitly selected offline updates. No unsigned fallback,
//! arbitrary download URLs, shell arguments, or automatic installer execution.
use sensor_crypto::IdentityKeypair;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const DOMAIN: &[u8] = b"SENSOR/publisher-update/v1\0";
const MAX_MANIFEST: u64 = 16 * 1024;
pub const MAX_INSTALLER: u64 = 256 * 1024 * 1024;
pub const TARGET: &str = "x86_64-pc-windows-msvc";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub schema: u32,
    pub product: String,
    pub target: String,
    pub version: [u32; 3],
    pub issued_unix: u64,
    pub expires_unix: u64,
    pub installer_bytes: u64,
    pub installer_sha256: [u8; 32],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: Vec<u8>,
    signature: Vec<u8>,
}

pub fn version(text: &str) -> Result<[u32; 3], String> {
    let parts: Vec<_> = text.split('.').collect();
    if parts.len() != 3 {
        return Err("Expected a three-part release version".into());
    }
    let mut result = [0; 3];
    for (out, text) in result.iter_mut().zip(parts) {
        if text.is_empty()
            || !text.bytes().all(|b| b.is_ascii_digit())
            || (text.len() > 1 && text.starts_with('0'))
        {
            return Err("Invalid release version".into());
        }
        *out = text.parse().map_err(|_| "Invalid release version")?;
    }
    Ok(result)
}
pub fn publisher_key() -> Result<[u8; 32], String> {
    let key = crate::parse_hex(include_str!("../../../assets/update-public-key.txt"))?;
    if key == [0; 32] {
        return Err("Publisher update key has not been provisioned".into());
    }
    Ok(key)
}
pub fn now() -> Result<u64, String> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "Invalid system clock")?
        .as_secs())
}
pub fn sign(release: &Release, key: &IdentityKeypair) -> Result<Vec<u8>, String> {
    let payload = serde_json::to_vec(release).map_err(|e| e.to_string())?;
    let signature = key.sign(&[DOMAIN, &payload].concat());
    serde_json::to_vec(&Envelope { payload, signature }).map_err(|e| e.to_string())
}
pub fn verify(
    bytes: &[u8],
    key: &[u8; 32],
    installed: [u32; 3],
    time: u64,
) -> Result<Release, String> {
    if bytes.len() as u64 > MAX_MANIFEST || key == &[0; 32] {
        return Err("Invalid update manifest size or publisher key".into());
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if envelope.payload.len() > 4096 || envelope.signature.len() != 64 {
        return Err("Invalid signed envelope".into());
    }
    IdentityKeypair::verify(
        key,
        &[DOMAIN, &envelope.payload].concat(),
        &envelope.signature,
    )
    .map_err(|_| "Publisher signature verification failed")?;
    let release: Release = serde_json::from_slice(&envelope.payload).map_err(|e| e.to_string())?;
    if release.schema != 1 || release.product != "SENSOR Remote Access" || release.target != TARGET
    {
        return Err("Update is for another product, platform or schema".into());
    }
    if release.version <= installed {
        return Err("Update would reinstall or downgrade the installed version".into());
    }
    if release.issued_unix > time
        || release.expires_unix <= time
        || release.expires_unix <= release.issued_unix
        || release.expires_unix - release.issued_unix > 90 * 86400
    {
        return Err("Update manifest is expired, not yet valid or exceeds 90 days".into());
    }
    if release.installer_bytes == 0 || release.installer_bytes > MAX_INSTALLER {
        return Err("Installer exceeds its permitted size".into());
    }
    Ok(release)
}
pub fn digest(reader: impl Read) -> Result<(u64, [u8; 32]), String> {
    let mut reader = reader.take(MAX_INSTALLER + 1);
    let mut buffer = [0; 65536];
    let mut count = 0;
    let mut hash = Sha256::new();
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        count += n as u64;
        hash.update(&buffer[..n]);
    }
    if count == 0 || count > MAX_INSTALLER {
        return Err("Invalid installer size".into());
    }
    Ok((count, hash.finalize().into()))
}

/// Copy to a fresh same-user staging directory, hash the copied bytes, and only
/// publish an .exe name after verification. Originals and installed files remain
/// untouched. No mutable source-path verify/execute race.
pub fn stage(
    manifest: &Path,
    installer: &Path,
    root: &Path,
    installed: [u32; 3],
) -> Result<(PathBuf, Release), String> {
    let mut bytes = Vec::new();
    File::open(manifest)
        .map_err(|e| e.to_string())?
        .take(MAX_MANIFEST + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let release = verify(&bytes, &publisher_key()?, installed, now()?)?;
    // Source opened once; changing it while copying produces a hash mismatch.
    let source = File::open(installer).map_err(|e| e.to_string())?;
    if !source.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Installer must be a regular file".into());
    }
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let directory = tempfile::Builder::new()
        .prefix("verified-")
        .tempdir_in(root)
        .map_err(|e| e.to_string())?;
    let mut staged =
        tempfile::NamedTempFile::new_in(directory.path()).map_err(|e| e.to_string())?;
    let copied = std::io::copy(
        &mut source.take(release.installer_bytes + 1),
        staged.as_file_mut(),
    )
    .map_err(|e| e.to_string())?;
    staged.as_file_mut().flush().map_err(|e| e.to_string())?;
    if copied != release.installer_bytes {
        return Err("Installer size does not match the signed release".into());
    }
    let (_, hash) = digest(File::open(staged.path()).map_err(|e| e.to_string())?)?;
    if hash != release.installer_sha256 {
        return Err("Installer hash does not match the signed release".into());
    }
    staged.as_file().sync_all().map_err(|e| e.to_string())?;
    let path = directory.path().join("SENSOR-Verified-Setup.exe");
    staged.persist_noclobber(&path).map_err(|e| e.to_string())?;
    let _ = directory.keep();
    Ok((path, release))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release() -> Release {
        Release {
            schema: 1,
            product: "SENSOR Remote Access".into(),
            target: TARGET.into(),
            version: [0, 3, 3],
            issued_unix: 100,
            expires_unix: 200,
            installer_bytes: 5,
            installer_sha256: Sha256::digest(b"hello").into(),
        }
    }
    #[test]
    fn valid_publisher_release() {
        let k = IdentityKeypair::generate();
        assert!(verify(
            &sign(&release(), &k).unwrap(),
            &k.public_key(),
            [0, 3, 2],
            150
        )
        .is_ok());
    }
    #[test]
    fn tampering_wrong_keys_and_unsigned_rejected() {
        let k = IdentityKeypair::generate();
        let data = sign(&release(), &k).unwrap();
        assert!(verify(
            &data,
            &IdentityKeypair::generate().public_key(),
            [0, 3, 2],
            150
        )
        .is_err());
        let mut e: Envelope = serde_json::from_slice(&data).unwrap();
        e.payload[1] ^= 1;
        assert!(verify(
            &serde_json::to_vec(&e).unwrap(),
            &k.public_key(),
            [0, 3, 2],
            150
        )
        .is_err());
        assert!(verify(
            &serde_json::to_vec(&release()).unwrap(),
            &k.public_key(),
            [0, 3, 2],
            150
        )
        .is_err());
    }
    #[test]
    fn every_authenticated_policy_is_enforced() {
        let k = IdentityKeypair::generate();
        for field in 0..10 {
            let mut r = release();
            match field {
                0 => r.schema = 2,
                1 => r.product = "OTHER".into(),
                2 => r.target = "linux".into(),
                3 => r.version = [0, 3, 2],
                4 => r.version = [0, 2, 0],
                5 => r.issued_unix = 151,
                6 => r.expires_unix = 150,
                7 => r.expires_unix = 10_000_000,
                8 => r.installer_bytes = 0,
                _ => r.installer_bytes = MAX_INSTALLER + 1,
            };
            assert!(
                verify(&sign(&r, &k).unwrap(), &k.public_key(), [0, 3, 2], 150).is_err(),
                "field {field}"
            );
        }
    }
    #[test]
    fn malformed_and_oversized_envelopes_rejected() {
        let k = IdentityKeypair::generate();
        for bytes in [
            vec![],
            vec![0; MAX_MANIFEST as usize + 1],
            b"{\"payload\":[],\"signature\":[]}".to_vec(),
        ] {
            assert!(verify(&bytes, &k.public_key(), [0, 3, 2], 150).is_err());
        }
    }
    #[test]
    fn strict_versions_and_bounded_digest() {
        for text in ["0.3", "0.3.2-beta", "0.03.2", "0.3.+2", "0.3.4294967296"] {
            assert!(version(text).is_err());
        }
        assert_eq!(version("0.3.2").unwrap(), [0, 3, 2]);
        assert_eq!(
            digest(&b"hello"[..]).unwrap(),
            (5, release().installer_sha256)
        );
        assert!(digest(&b""[..]).is_err());
    }
}
