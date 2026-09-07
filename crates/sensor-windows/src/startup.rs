//! Explicit per-user Windows sign-in startup. Not pre-login/service access.
use std::{path::Path, ptr};
use windows_sys::Win32::System::Registry::*;
const SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "SENSORRemote";
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn command(exe: &Path) -> Result<String, String> {
    let path = exe.to_str().ok_or("Executable path is not valid Unicode")?;
    if !exe.is_absolute()
        || !exe.is_file()
        || !exe
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        || path.chars().any(|c| c.is_control() || c == '"')
    {
        return Err("Startup requires an existing absolute executable path".into());
    }
    Ok(format!("\"{path}\""))
}
fn current() -> Result<Option<String>, String> {
    let mut bytes = 8192;
    let mut text = [0u16; 4096];
    // Bounded UTF-16 REG_SZ read; no environment expansion or shell parsing.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(SUBKEY).as_ptr(),
            wide(VALUE).as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            text.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if result == 2 {
        return Ok(None);
    }
    if result != 0 {
        return Err(format!(
            "Cannot read SENSOR startup value: Windows error {result}"
        ));
    }
    if bytes < 2 || bytes as usize > text.len() * 2 || bytes % 2 != 0 {
        return Err("Malformed SENSOR startup value".into());
    }
    let length = bytes as usize / 2;
    if text[length - 1] != 0 || text[..length - 1].contains(&0) {
        return Err("Malformed SENSOR startup command".into());
    }
    Ok(Some(
        String::from_utf16(&text[..length - 1]).map_err(|_| "Malformed startup text")?,
    ))
}
pub fn enabled(exe: &Path) -> Result<bool, String> {
    let expected = command(exe)?;
    Ok(current()?.is_some_and(|v| v == expected))
}
pub fn configure(exe: &Path, enabled: bool) -> Result<(), String> {
    let expected = command(exe)?;
    if current()?.is_some_and(|value| value != expected) {
        return Err(
            "A different SENSOR startup command already exists; it was not overwritten".into(),
        );
    }
    let mut key = ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(SUBKEY).as_ptr(),
            0,
            ptr::null(),
            0,
            KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    };
    if result != 0 {
        return Err(format!(
            "Cannot open per-user startup: Windows error {result}"
        ));
    }
    let text = wide(&expected);
    let name = wide(VALUE);
    let result = unsafe {
        if enabled {
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                text.as_ptr().cast(),
                (text.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(key, name.as_ptr())
        }
    };
    unsafe { RegCloseKey(key) };
    if result != 0 && !(result == 2 && !enabled) {
        return Err(format!(
            "Cannot change per-user startup: Windows error {result}"
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_command_has_no_shell_or_extra_arguments() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("SENSOR Remote.exe");
        std::fs::write(&exe, b"fixture").unwrap();
        assert_eq!(command(&exe).unwrap(), format!("\"{}\"", exe.display()));
        assert!(command(Path::new("relative.exe")).is_err());
        assert!(command(root.path()).is_err());
    }
}
