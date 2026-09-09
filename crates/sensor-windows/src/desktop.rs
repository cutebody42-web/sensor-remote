//! Desktop Duplication for the interactive user desktop, never secure-desktop bypass.
use sensor_media::{pixels, BgraFrame, Display};
use sensor_session::permissions::{Consent, Permission};
use thiserror::Error;
use windows::{
    core::{Interface, PWSTR},
    Win32::{
        Foundation::{HANDLE, HMODULE},
        Graphics::{
            Direct3D::*,
            Direct3D11::*,
            Dxgi::{Common::*, *},
        },
        System::{RemoteDesktop::*, StationsAndDesktops::*},
        UI::WindowsAndMessaging::*,
    },
};

/// RtlGetVersion is independent of application compatibility manifest shims.
/// This detector does not imply that the modern GUI/toolchain supports Win7.
pub fn os_version() -> Result<(u32, u32, u32), DesktopError> {
    #[repr(C)]
    struct Version {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(version: *mut Version) -> i32;
    }
    let mut version = Version {
        size: std::mem::size_of::<Version>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    if unsafe { RtlGetVersion(&mut version) } != 0 {
        return Err(DesktopError::Monitor);
    }
    Ok((version.major, version.minor, version.build))
}

#[derive(Debug, Error)]
pub enum DesktopError {
    #[error("Windows desktop: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("desktop permission: {0}")]
    Permission(#[from] sensor_session::permissions::PermissionError),
    #[error("desktop frame: {0}")]
    Media(#[from] sensor_media::MediaError),
    #[error("Desktop locked, switched or unavailable; no capture/input is permitted")]
    Locked,
    #[error("selected monitor is unavailable")]
    Monitor,
    #[error("display mode changed; reconnect to refresh display configuration")]
    Changed,
}

/// Opening READOBJECTS never switches, unlocks or attaches to another desktop.
pub fn interactive_desktop() -> Result<(), DesktopError> {
    let version = os_version()?;
    unsafe {
        // The Default desktop can remain accessible behind LockApp. Check the
        // current Windows session too, without switching desktops or unlocking.
        let mut buffer = PWSTR::null();
        let mut bytes = 0;
        WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        )
        .map_err(|_| DesktopError::Locked)?;
        let unlocked = if !buffer.is_null() && bytes as usize >= std::mem::size_of::<WTSINFOEXW>() {
            let info = std::ptr::read_unaligned(buffer.0.cast::<WTSINFOEXW>());
            info.Level == 1
                && info.Data.WTSInfoExLevel1.SessionState == WTSActive
                && info.Data.WTSInfoExLevel1.SessionFlags
                    == if (version.0, version.1) == (6, 1) {
                        // Documented Windows 7 / Server 2008 R2 reversed flags.
                        WTS_SESSIONSTATE_LOCK as i32
                    } else {
                        WTS_SESSIONSTATE_UNLOCK as i32
                    }
        } else {
            false
        };
        WTSFreeMemory(buffer.0.cast());
        if !unlocked {
            return Err(DesktopError::Locked);
        }
        let desktop = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS)
            .map_err(|_| DesktopError::Locked)?;
        let mut name = [0_u16; 128];
        let result = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            std::mem::size_of_val(&name) as u32,
            None,
        );
        let _ = CloseDesktop(desktop);
        result.map_err(|_| DesktopError::Locked)?;
        let length = name
            .iter()
            .position(|c| *c == 0)
            .ok_or(DesktopError::Locked)?;
        if String::from_utf16_lossy(&name[..length]) != "Default" {
            return Err(DesktopError::Locked);
        }
        Ok(())
    }
}

fn outputs() -> Result<Vec<(Display, IDXGIAdapter1, IDXGIOutput)>, DesktopError> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut result = Vec::new();
    for a in 0..16 {
        let adapter = match unsafe { factory.EnumAdapters1(a) } {
            Ok(a) => a,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => return Err(e.into()),
        };
        for o in 0..32 {
            let output = match unsafe { adapter.EnumOutputs(o) } {
                Ok(o) => o,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(e.into()),
            };
            let desc = unsafe { output.GetDesc()? };
            if !desc.AttachedToDesktop.as_bool() {
                continue;
            }
            let width = desc.DesktopCoordinates.right - desc.DesktopCoordinates.left;
            let height = desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top;
            if width <= 0 || height <= 0 || result.len() >= 64 {
                continue;
            }
            let length = desc
                .DeviceName
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(desc.DeviceName.len());
            let display = Display {
                index: result.len() as u32,
                name: String::from_utf16_lossy(&desc.DeviceName[..length]),
                left: desc.DesktopCoordinates.left,
                top: desc.DesktopCoordinates.top,
                width: width as u32,
                height: height as u32,
            };
            display.validate()?;
            result.push((display, adapter.clone(), output));
        }
    }
    Ok(result)
}
pub fn displays(consent: &Consent) -> Result<Vec<Display>, DesktopError> {
    consent.require(Permission::ViewDesktop)?;
    interactive_desktop()?;
    Ok(outputs()?
        .into_iter()
        .map(|(display, _, _)| display)
        .collect())
}

struct DxgiCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplicate: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    pub display: Display,
    pub rotation: u32,
}
struct FrameLease<'a>(&'a IDXGIOutputDuplication);
impl Drop for FrameLease<'_> {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.ReleaseFrame();
        }
    }
}
struct Mapping<'a>(&'a ID3D11DeviceContext, &'a ID3D11Texture2D);
impl Drop for Mapping<'_> {
    fn drop(&mut self) {
        unsafe {
            self.0.Unmap(self.1, 0);
        }
    }
}
impl DxgiCapture {
    pub fn new(index: u32, consent: &Consent) -> Result<Self, DesktopError> {
        consent.require(Permission::ViewDesktop)?;
        interactive_desktop()?;
        let (display, adapter, output) = outputs()?
            .into_iter()
            .find(|(d, _, _)| d.index == index)
            .ok_or(DesktopError::Monitor)?;
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.ok_or(DesktopError::Monitor)?;
        let context = context.ok_or(DesktopError::Monitor)?;
        let output: IDXGIOutput1 = output.cast()?;
        let duplicate = unsafe { output.DuplicateOutput(&device)? };
        let desc = unsafe { duplicate.GetDesc() };
        let rotation = match desc.Rotation {
            DXGI_MODE_ROTATION_ROTATE90 => 90,
            DXGI_MODE_ROTATION_ROTATE180 => 180,
            DXGI_MODE_ROTATION_ROTATE270 => 270,
            _ => 0,
        };
        Ok(Self {
            device,
            context,
            duplicate,
            staging: None,
            display,
            rotation,
        })
    }
    pub fn next(&mut self, consent: &Consent) -> Result<Option<BgraFrame>, DesktopError> {
        consent.require(Permission::ViewDesktop)?;
        interactive_desktop()?;
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        match unsafe {
            self.duplicate
                .AcquireNextFrame(10, &mut info, &mut resource)
        } {
            Ok(()) => (),
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => return Err(DesktopError::Changed),
            Err(e) => return Err(e.into()),
        }
        let _lease = FrameLease(&self.duplicate);
        // Pointer-only updates must not trigger full-frame video encoding.
        if info.LastPresentTime == 0 {
            return Ok(None);
        }
        let source: ID3D11Texture2D = resource.ok_or(DesktopError::Monitor)?.cast()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut desc) };
        let count = pixels(desc.Width, desc.Height)?;
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(DesktopError::Changed);
        }
        let (expected_w, expected_h) = if self.rotation == 90 || self.rotation == 270 {
            (self.display.height, self.display.width)
        } else {
            (self.display.width, self.display.height)
        };
        if desc.Width != expected_w || desc.Height != expected_h {
            return Err(DesktopError::Changed);
        }
        if self.staging.is_none() {
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            unsafe {
                self.device
                    .CreateTexture2D(&desc, None, Some(&mut self.staging))?;
            }
        }
        let staging = self.staging.as_ref().ok_or(DesktopError::Monitor)?;
        unsafe { self.context.CopyResource(staging, &source) };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        }
        let _mapping = Mapping(&self.context, staging);
        if mapped.pData.is_null() || mapped.RowPitch < desc.Width * 4 {
            return Err(DesktopError::Monitor);
        }
        let mut bytes = vec![0; count * 4];
        for y in 0..desc.Height as usize {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                    bytes.as_mut_ptr().add(y * desc.Width as usize * 4),
                    desc.Width as usize * 4,
                );
            }
        }
        // A switch during acquisition must not expose the new desktop.
        interactive_desktop()?;
        Ok(Some(BgraFrame {
            width: desc.Width,
            height: desc.Height,
            bytes,
        }))
    }
    pub fn cursor(&self) -> Result<(i32, i32, bool), DesktopError> {
        interactive_desktop()?;
        let mut info = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            GetCursorInfo(&mut info)?;
        }
        let x = info.ptScreenPos.x - self.display.left;
        let y = info.ptScreenPos.y - self.display.top;
        Ok((
            x,
            y,
            info.flags == CURSOR_SHOWING
                && x >= 0
                && y >= 0
                && x < self.display.width as i32
                && y < self.display.height as i32,
        ))
    }
}

enum Backend {
    Modern(DxgiCapture),
    Legacy(LegacyCapture),
}

/// Session code owns this interface, never platform-specific GDI/DXGI handles.
/// DXGI failures on modern Windows are NOT silently bypassed by GDI.
pub struct Capture {
    backend: Backend,
    pub display: Display,
    pub rotation: u32,
}
impl Capture {
    pub fn new(index: u32, consent: &Consent) -> Result<Self, DesktopError> {
        let (major, minor, _) = os_version()?;
        if (major, minor) == (6, 1) {
            return Self::legacy(index, consent);
        }
        let capture = DxgiCapture::new(index, consent)?;
        Ok(Self {
            display: capture.display.clone(),
            rotation: capture.rotation,
            backend: Backend::Modern(capture),
        })
    }
    /// Explicit diagnostics/legacy-build entrypoint; never an access-denied retry.
    pub fn legacy(index: u32, consent: &Consent) -> Result<Self, DesktopError> {
        let capture = LegacyCapture::new(index, consent)?;
        Ok(Self {
            display: capture.display.clone(),
            rotation: 0,
            backend: Backend::Legacy(capture),
        })
    }
    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            Backend::Modern(_) => "DXGI Desktop Duplication",
            Backend::Legacy(_) => "GDI legacy capture",
        }
    }
    pub fn next(&mut self, consent: &Consent) -> Result<Option<BgraFrame>, DesktopError> {
        match &mut self.backend {
            Backend::Modern(c) => c.next(consent),
            Backend::Legacy(c) => c.next(consent),
        }
    }
    pub fn cursor(&self) -> Result<(i32, i32, bool), DesktopError> {
        match &self.backend {
            Backend::Modern(c) => c.cursor(),
            Backend::Legacy(c) => c.cursor(),
        }
    }
}

use windows::Win32::Graphics::Gdi::*;
struct LegacyCapture {
    display: Display,
    screen: HDC,
    memory: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut std::ffi::c_void,
}
impl LegacyCapture {
    fn new(index: u32, consent: &Consent) -> Result<Self, DesktopError> {
        consent.require(Permission::ViewDesktop)?;
        interactive_desktop()?;
        let display = displays(consent)?
            .into_iter()
            .find(|d| d.index == index)
            .ok_or(DesktopError::Monitor)?;
        let mut capture = Self {
            display,
            screen: HDC::default(),
            memory: HDC::default(),
            bitmap: HBITMAP::default(),
            previous: HGDIOBJ::default(),
            bits: std::ptr::null_mut(),
        };
        unsafe {
            capture.screen = GetDC(None);
            if capture.screen.is_invalid() {
                return Err(DesktopError::Monitor);
            }
            capture.memory = CreateCompatibleDC(Some(capture.screen));
            if capture.memory.is_invalid() {
                return Err(DesktopError::Monitor);
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: capture.display.width as i32,
                    biHeight: -(capture.display.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            capture.bitmap = CreateDIBSection(
                Some(capture.screen),
                &info,
                DIB_RGB_COLORS,
                &mut capture.bits,
                None,
                0,
            )?;
            if capture.bits.is_null() {
                return Err(DesktopError::Monitor);
            }
            capture.previous = SelectObject(capture.memory, HGDIOBJ(capture.bitmap.0));
            if capture.previous.is_invalid() {
                return Err(DesktopError::Monitor);
            }
        }
        Ok(capture)
    }
    fn next(&mut self, consent: &Consent) -> Result<Option<BgraFrame>, DesktopError> {
        consent.require(Permission::ViewDesktop)?;
        interactive_desktop()?;
        let count = pixels(self.display.width, self.display.height)?;
        let bytes = unsafe {
            BitBlt(
                self.memory,
                0,
                0,
                self.display.width as i32,
                self.display.height as i32,
                Some(self.screen),
                self.display.left,
                self.display.top,
                SRCCOPY | CAPTUREBLT,
            )?;
            GdiFlush().ok()?;
            std::slice::from_raw_parts(self.bits.cast::<u8>(), count * 4).to_vec()
        };
        interactive_desktop()?;
        Ok(Some(BgraFrame {
            width: self.display.width,
            height: self.display.height,
            bytes,
        }))
    }
    fn cursor(&self) -> Result<(i32, i32, bool), DesktopError> {
        interactive_desktop()?;
        let mut info = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            GetCursorInfo(&mut info)?;
        }
        let x = info.ptScreenPos.x - self.display.left;
        let y = info.ptScreenPos.y - self.display.top;
        Ok((
            x,
            y,
            info.flags == CURSOR_SHOWING
                && x >= 0
                && y >= 0
                && x < self.display.width as i32
                && y < self.display.height as i32,
        ))
    }
}
impl Drop for LegacyCapture {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.is_invalid() && !self.memory.is_invalid() {
                SelectObject(self.memory, self.previous);
            }
            if !self.bitmap.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            }
            if !self.memory.is_invalid() {
                let _ = DeleteDC(self.memory);
            }
            if !self.screen.is_invalid() {
                ReleaseDC(None, self.screen);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sensor_session::permissions::Permissions;
    #[test]
    fn capture_and_display_enumeration_require_accepted_view_permission() {
        let pending = Consent::pending(Permissions::screen_sharing());
        assert!(matches!(
            Capture::new(0, &pending),
            Err(DesktopError::Permission(_))
        ));
        assert!(matches!(
            displays(&pending),
            Err(DesktopError::Permission(_))
        ));
        let mut files = Consent::pending(Permissions::file_transfer());
        files.accept(Permissions::file_transfer()).unwrap();
        assert!(matches!(
            Capture::new(0, &files),
            Err(DesktopError::Permission(_))
        ));
    }
}
