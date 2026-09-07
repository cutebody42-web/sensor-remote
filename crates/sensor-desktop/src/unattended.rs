//! Revocable, expiring authority for one explicitly verified remote identity.
//! This is ordinary signed-in-user access, not a SYSTEM service or UAC bypass.
use sensor_client::Mode;
use sensor_identity::KeyProtector;
use sensor_session::ExpectedPeer;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
const CONTEXT: &[u8] = b"SENSOR/unattended-grant/v1/";
const MAX_BYTES: usize = 16384;
pub const MAX_LIFETIME: u64 = 30 * 86400;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub device_id: sensor_core::DeviceId,
    pub public_key: [u8; 32],
    pub issued_unix: u64,
    pub expires_unix: u64,
}
impl Grant {
    pub fn new(peer: ExpectedPeer, now: u64) -> Result<Self, String> {
        if peer.public_key == [0; 32] {
            return Err("Unattended access requires a verified nonzero public key".into());
        }
        Ok(Self {
            device_id: peer.device_id,
            public_key: peer.public_key,
            issued_unix: now,
            expires_unix: now.checked_add(MAX_LIFETIME).ok_or("Invalid grant date")?,
        })
    }
    pub fn permits(&self, peer: ExpectedPeer, mode: Mode, now: u64) -> bool {
        self.valid()
            && self.device_id == peer.device_id
            && self.public_key == peer.public_key
            && self.issued_unix <= now
            && now < self.expires_unix
            && matches!(mode, Mode::ScreenView | Mode::RemoteControl)
    }
    fn valid(&self) -> bool {
        self.public_key != [0; 32]
            && self.expires_unix > self.issued_unix
            && self.expires_unix - self.issued_unix <= MAX_LIFETIME
    }
}
pub fn load(
    path: &Path,
    protector: &impl KeyProtector,
    local_key: &[u8; 32],
) -> Result<Option<Grant>, String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Unattended settings exceed the size limit".into());
    }
    let data = protector
        .unprotect(&bytes, &[CONTEXT, local_key].concat())
        .map_err(|_| "Unattended authority could not be authenticated; access is disabled")?;
    if data.len() > MAX_BYTES {
        return Err("Unattended grant is malformed".into());
    }
    let grant: Option<Grant> = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    if grant.as_ref().is_some_and(|g| !g.valid()) {
        return Err("Invalid unattended grant".into());
    }
    Ok(grant)
}
pub fn save(
    path: &Path,
    grant: Option<&Grant>,
    protector: &impl KeyProtector,
    local_key: &[u8; 32],
) -> Result<(), String> {
    if grant.is_some_and(|g| !g.valid()) {
        return Err("Invalid unattended grant".into());
    }
    let data = serde_json::to_vec(&grant).map_err(|e| e.to_string())?;
    let protected = protector
        .protect(&data, &[CONTEXT, local_key].concat())
        .map_err(|e| e.to_string())?;
    if protected.len() > MAX_BYTES {
        return Err("Protected grant exceeds the size limit".into());
    }
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing grant directory")?)
        .map_err(|e| e.to_string())?;
    temp.write_all(&protected).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn peer() -> ExpectedPeer {
        ExpectedPeer {
            device_id: sensor_core::DeviceId::new(123456789).unwrap(),
            public_key: [1; 32],
        }
    }
    #[test]
    fn grants_are_identity_mode_and_time_scoped() {
        let p = peer();
        let g = Grant::new(p, 100).unwrap();
        assert!(g.permits(p, Mode::RemoteControl, 101));
        assert!(g.permits(p, Mode::ScreenView, 101));
        for m in [
            Mode::Chat,
            Mode::FileTransfer,
            Mode::ScreenViewClipboard,
            Mode::RemoteControlClipboard,
        ] {
            assert!(!g.permits(p, m, 101));
        }
        assert!(!g.permits(p, Mode::RemoteControl, 99));
        assert!(!g.permits(p, Mode::RemoteControl, g.expires_unix));
        let mut other = p;
        other.public_key = [2; 32];
        assert!(!g.permits(other, Mode::RemoteControl, 101));
        other = p;
        other.device_id = sensor_core::DeviceId::new(987654321).unwrap();
        assert!(!g.permits(other, Mode::RemoteControl, 101));
        other.public_key = [0; 32];
        assert!(Grant::new(other, 100).is_err());
        assert!(Grant::new(p, u64::MAX).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn real_dpapi_grant_roundtrip_revocation_tamper_and_profile_binding() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("grant.bin");
        let p = sensor_windows::UserDpapi;
        let g = Grant::new(peer(), 100).unwrap();
        save(&path, Some(&g), &p, &[3; 32]).unwrap();
        assert!(load(&path, &p, &[3; 32]).unwrap().unwrap().permits(
            peer(),
            Mode::RemoteControl,
            101
        ));
        assert!(load(&path, &p, &[4; 32]).is_err());
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(load(&path, &p, &[3; 32]).is_err());
        save(&path, None, &p, &[3; 32]).unwrap();
        assert!(load(&path, &p, &[3; 32]).unwrap().is_none());
    }
}
