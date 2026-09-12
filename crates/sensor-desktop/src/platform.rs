//! One application and protocol; OS-specific graphics are selected before any
//! identity, listener or capture is started. Selection is not OS acceptance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    Direct3D12,
    OpenGl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    pub os: (u32, u32, u32),
    pub presentation: Presentation,
    pub legacy_capture: bool,
    pub unified_build: bool,
}
impl Platform {
    pub fn select(
        os: (u32, u32, u32),
        unified_build: bool,
        renderer_override: Option<&str>,
    ) -> Result<Self, &'static str> {
        let legacy_capture = match os {
            (6, 1, build) if build >= 7601 && unified_build => true,
            (6, 1, build) if build < 7601 => return Err("Windows 7 requires Service Pack 1 (build 7601)."),
            (6, 1, _) => return Err("This build uses a Windows 10 runtime. Use the unified SENSOR build; relabeling a modern executable does not make it Windows 7 compatible."),
            (major, _, _) if major >= 10 => false,
            _ => return Err("SENSOR targets Windows 7 SP1 x64 and Windows 10/11 x64. This Windows version is outside the supported build paths."),
        };
        let presentation = match renderer_override.unwrap_or("auto") {
            "auto" if !legacy_capture => Presentation::Direct3D12,
            "auto" | "glow" => Presentation::OpenGl,
            _ => return Err("SENSOR_UI_RENDERER must be auto or glow. The glow option only tests presentation; it never changes the detected OS or capture security."),
        };
        Ok(Self {
            os,
            presentation,
            legacy_capture,
            unified_build,
        })
    }
    pub fn renderer_name(self) -> &'static str {
        match self.presentation {
            Presentation::Direct3D12 => "native egui / wgpu DX12",
            Presentation::OpenGl => "native egui / OpenGL (graphics driver required)",
        }
    }
    pub fn capture_name(self) -> &'static str {
        if self.legacy_capture {
            "GDI"
        } else {
            "DXGI Desktop Duplication"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_unified_build_selects_each_os_without_downgrading_modern_capture() {
        let win7 = Platform::select((6, 1, 7601), true, None).unwrap();
        assert_eq!(win7.presentation, Presentation::OpenGl);
        assert!(win7.legacy_capture);
        for os in [(10, 0, 19045), (10, 0, 26200)] {
            let modern = Platform::select(os, true, None).unwrap();
            assert_eq!(modern.presentation, Presentation::Direct3D12);
            assert!(!modern.legacy_capture);
        }
    }
    #[test]
    fn renderer_diagnostic_does_not_fake_os_or_bypass_capture() {
        let p = Platform::select((10, 0, 26200), true, Some("glow")).unwrap();
        assert_eq!(p.presentation, Presentation::OpenGl);
        assert!(!p.legacy_capture);
        assert_eq!(p.os, (10, 0, 26200));
        assert!(Platform::select((6, 1, 7601), true, Some("dx12")).is_err());
    }
    #[test]
    fn unsupported_os_and_modern_runtime_cannot_masquerade_as_win7() {
        assert!(Platform::select((6, 1, 7600), true, None).is_err());
        assert!(Platform::select((6, 1, 7601), false, None).is_err());
        assert!(Platform::select((6, 0, 6002), true, None).is_err());
        assert!(Platform::select((6, 3, 9600), true, None).is_err());
        assert!(Platform::select((10, 0, 19045), false, None).is_ok());
    }
}
