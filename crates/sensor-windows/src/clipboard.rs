//! Text-only clipboard bridge. No content is logged or persisted.
use sensor_media::validate_clipboard;
use sensor_session::permissions::{Consent, Permission};
use sha2::{Digest, Sha256};
use windows::Win32::{
    Foundation::HGLOBAL,
    System::{DataExchange::*, Memory::*, Ole::CF_UNICODETEXT},
};

fn bounded_text() -> Option<String> {
    // Validate OS allocation size before copying; a huge local clipboard must
    // not bypass the protocol's 64 KiB cap through an unbounded get_text call.
    unsafe {
        OpenClipboard(None).ok()?;
        struct Open;
        impl Drop for Open {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseClipboard();
                }
            }
        }
        let _open = Open;
        let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        if !(2..=(sensor_media::MAX_CLIPBOARD_BYTES + 1) * 2).contains(&size)
            || !size.is_multiple_of(2)
        {
            return None;
        }
        let pointer = GlobalLock(global);
        if pointer.is_null() {
            return None;
        }
        struct Locked(HGLOBAL);
        impl Drop for Locked {
            fn drop(&mut self) {
                unsafe {
                    let _ = GlobalUnlock(self.0);
                }
            }
        }
        let _locked = Locked(global);
        let units = std::slice::from_raw_parts(pointer.cast::<u16>(), size / 2);
        let end = units.iter().position(|unit| *unit == 0)?;
        String::from_utf16(&units[..end]).ok()
    }
}

#[derive(Default)]
pub struct TextClipboard {
    clipboard: Option<arboard::Clipboard>,
    fingerprint: Option<[u8; 32]>,
}

impl TextClipboard {
    /// The first permitted observation is a baseline: pre-session clipboard
    /// contents are never sent merely because a session starts.
    pub fn poll(&mut self, enabled: bool, consent: &Consent) -> Option<String> {
        if !enabled || consent.require(Permission::ClipboardText).is_err() {
            self.fingerprint = None;
            return None;
        }
        let text = bounded_text()?;
        self.observe(text)
    }

    fn observe(&mut self, text: String) -> Option<String> {
        if validate_clipboard(&text).is_err() {
            return None;
        }
        let digest: [u8; 32] = Sha256::digest(text.as_bytes()).into();
        let previous = self.fingerprint.replace(digest);
        (previous.is_some() && previous != Some(digest)).then_some(text)
    }

    pub fn apply(&mut self, text: &str, enabled: bool, consent: &Consent) -> Result<bool, String> {
        // Permission is checked before touching the OS, even for malicious peers.
        consent
            .require(Permission::ClipboardText)
            .map_err(|e| e.to_string())?;
        validate_clipboard(text).map_err(|e| e.to_string())?;
        if !enabled {
            return Ok(false);
        }
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new().map_err(|_| "Clipboard unavailable")?);
        }
        let clipboard = self.clipboard.as_mut().ok_or("Clipboard unavailable")?;
        clipboard
            .set_text(text)
            .map_err(|_| "Clipboard busy or unavailable")?;
        self.fingerprint = Some(Sha256::digest(text.as_bytes()).into());
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sensor_session::permissions::Permissions;
    #[test]
    #[ignore = "temporarily exercises the real Windows clipboard; restores the original OLE data object"]
    fn actual_windows_clipboard_unicode_loop_prevention_and_disable() {
        use windows::Win32::System::{Com::IDataObject, Ole::*};
        unsafe {
            OleInitialize(None).unwrap();
        }
        struct Restore(Option<IDataObject>);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    if let Some(original) = &self.0 {
                        let _ = OleSetClipboard(original);
                        let _ = OleFlushClipboard();
                    }
                    self.0.take();
                    OleUninitialize();
                }
            }
        }
        let _restore = Restore(Some(unsafe {
            OleGetClipboard().expect("must preserve original clipboard before testing")
        }));
        let permissions = Permissions::of(&[Permission::ClipboardText]);
        let mut consent = Consent::pending(permissions);
        consent.accept(permissions).unwrap();
        let mut endpoint = TextClipboard::default();
        let mut user = arboard::Clipboard::new().unwrap();
        user.set_text("SENSOR baseline").unwrap();
        assert_eq!(endpoint.poll(true, &consent), None);
        user.set_text("SENSOR local مرحبا").unwrap();
        assert_eq!(
            endpoint.poll(true, &consent),
            Some("SENSOR local مرحبا".into())
        );
        assert_eq!(
            endpoint.apply("SENSOR remote 日本語", true, &consent),
            Ok(true)
        );
        assert_eq!(user.get_text().unwrap(), "SENSOR remote 日本語");
        assert_eq!(endpoint.poll(true, &consent), None);
        assert_eq!(endpoint.apply("SENSOR blocked", false, &consent), Ok(false));
        assert_eq!(user.get_text().unwrap(), "SENSOR remote 日本語");
        assert_eq!(endpoint.poll(false, &consent), None);
        user.set_text("SENSOR after disabled").unwrap();
        assert_eq!(endpoint.poll(true, &consent), None);
        println!("Actual Windows Unicode clipboard write/read, loop prevention and disabling passed; original OLE clipboard restored on exit.");
    }
    #[test]
    fn denied_or_disabled_clipboard_does_not_touch_windows() {
        let mut clipboard = TextClipboard::default();
        let mut consent = Consent::pending(Permissions::screen_sharing());
        assert!(clipboard.apply("blocked", true, &consent).is_err());
        consent.accept(Permissions::screen_sharing()).unwrap();
        assert!(clipboard.apply("blocked", true, &consent).is_err());
        let permissions = Permissions::of(&[Permission::ClipboardText]);
        let mut consent = Consent::pending(permissions);
        consent.accept(permissions).unwrap();
        assert_eq!(clipboard.apply("disabled", false, &consent), Ok(false));
        assert!(clipboard.clipboard.is_none());
        assert!(clipboard.apply("bad\0text", true, &consent).is_err());
        assert!(clipboard.apply(&"x".repeat(65537), true, &consent).is_err());
        assert!(clipboard.clipboard.is_none());
    }

    #[test]
    fn clipboard_baseline_duplicates_and_remote_echo_are_not_forwarded() {
        let mut clipboard = TextClipboard::default();
        assert_eq!(clipboard.observe("pre-session secret".into()), None);
        assert_eq!(clipboard.observe("pre-session secret".into()), None);
        assert_eq!(
            clipboard.observe("new text".into()),
            Some("new text".into())
        );
        clipboard.fingerprint = Some(Sha256::digest(b"remote text").into());
        assert_eq!(clipboard.observe("remote text".into()), None);
        assert_eq!(
            clipboard.observe("changed locally".into()),
            Some("changed locally".into())
        );
        let consent = Consent::pending(Permissions::NONE);
        assert_eq!(clipboard.poll(false, &consent), None);
        assert_eq!(clipboard.observe("after re-enable".into()), None);
    }
}
