//! Desktop Duplication for the interactive user desktop, never secure-desktop bypass.
use sensor_media::{pixels, BgraFrame, Display};
use sensor_session::permissions::{Consent, Permission};
use thiserror::Error;
use windows::{
    core::Interface,
    Win32::{
        Foundation::{HANDLE, HMODULE},
        Graphics::{
            Direct3D::*,
            Direct3D11::*,
            Dxgi::{Common::*, *},
        },
        System::StationsAndDesktops::*,
        UI::WindowsAndMessaging::*,
    },
};

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
    unsafe {
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

pub struct Capture {
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
impl Capture {
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
