# One SENSOR app for Windows 7 / 10 / 11 - compatibility candidate

Status on 2026-09-12: **not certified across all three operating systems**.
The owner requires one application, not a second Legacy product. The prior
separate-frontend scaffold was withdrawn; no second frontend is shipped.

## What is actually built

`scripts/build-unified-windows.ps1` builds the real `SENSOR-Remote.exe` with
the existing complete egui UI, original logo, shared DPAPI identity format,
same authenticated Internet protocol and all existing permission checks.
There are no OS-specific app downloads or subprocess-renderer executables.
Both graphics backends are compiled into **one x64 EXE**:

| Detected operating system | UI rendering | Desktop capture | Acceptance |
|---|---|---|---|
| Windows 7 SP1 x64, build 7601+ | OpenGL / glow | GDI | Actual OS test NOT RUN |
| Windows 10 x64 | wgpu / DX12 | DXGI duplication | Actual OS test NOT RUN |
| Windows 11 x64 | wgpu / DX12 | DXGI duplication | Unified GUI launched, identity reopened and online; both graphics paths observed |

`RtlGetVersion` detects the real OS, independent of manifest compatibility
shims. Unit tests cover all three selections, rejection of Win7 without SP1,
unsupported OS versions and attempts to label a normal Win10-runtime build as
Win7 compatible. `SENSOR_UI_RENDERER=glow` is an explicit presentation-only
diagnostic; it does **not** fake the OS, force GDI on modern Windows, grant
consent, or bypass locked/secure desktops.

Win7 needs a functioning compatible OpenGL driver and Media Foundation H.264.
No graphics driver, OS component, root certificate or security setting is
silently installed or disabled. Missing capabilities must fail with an error.
No minimum real Win7 performance or successful Win7 connection is claimed.

## Runtime / PE evidence

The pinned `nightly-2026-09-08` compiler rebuilds standard library for
`x86_64-win7-windows-msvc`; the existing stable Rust1.98.1 modern build remains
available for ordinary CI. The unified executable statically links the CRT.
The documented `windows_slim_errors` configuration avoids unnecessary WinRT
error-info imports while retaining HRESULT error reporting.

The previous installed modern binary imported `ProcessPrng`, `WaitOnAddress`,
`WakeByAddress*`, `GetSystemTimePreciseAsFileTime` and a newer API-set DLL.
The first successfully built unified executable removed those loader blockers.
The actual PE had 339 parsed imports, x64 machine type, subsystem6.00 and
DEP/ASLR enabled. The negative audit correctly rejected the modern executable.

`scripts/audit-windows7-imports.ps1` fails on known post-Win7 imports,
unreviewed DLLs, high subsystem versions or missing DEP/ASLR. It generates a
hash-bound import inventory. **This known-blocker audit is not a complete
static API certification, dynamic-API audit, or successful Windows 7 run.**

## Build and package

```powershell
rustup toolchain install nightly-2026-09-08 --profile minimal --component rust-src
.\scripts\build-unified-windows.ps1
.\scripts\package.ps1 -UnifiedCandidate -OutputDirectory C:\path\to\fresh-package
.\scripts\installer.ps1 -PackageDirectory C:\path\to\fresh-package -Compiler C:\path\to\makensis.exe -OutputFile C:\path\to\fresh-setup.exe
```

This produces **one installer with the same `SENSOR-Remote.exe` payload for
all three OS paths**, not separate Legacy/Modern applications. GUI and CLI
payloads receive import audits. `WINDOWS-BUILD.json` and the two import reports
record the actual hashes and explicitly retain `runtime_verified=false` for
Windows7. The installer validates the package and hash before enabling the
candidate Win7 path. It refuses Win7 without SP1 and silent Win7 installation;
interactive Win7 testing requires accepting the explicit unverified-build
warning. Ordinary modern packages retain the Win10 minimum guard.

## Observed testing and remaining gates

September12 verification supersedes the earlier local launch blocker below:
the exact unified EXE SHA256
`827863a554d0cfcdec167fc0441b817c3e84e240a74a21160104055583de87f9`
launched on Windows11 build26200. The native diagnostics UI verified DX12,
then after a clean close/restart the same EXE verified the explicit OpenGL
diagnostic path. Both reopened the existing DPAPI identity without changing
its ID/public key and reached Online/Ready. The OpenGL test still ran on
Windows11, not Windows7. Full workspace tests, formatting and clippy passed
again on September12. The HTTPS service reports20Mbps,60fps,1920x1080 limits;
those are configured limits, not measured remote performance.

The September12 unified installer was installed successfully on the owner's
Windows11 laptop (exit0). Its installed GUI hash matches the hash above and
the existing DPAPI identity file remained byte-for-byte unchanged. The package
source revision is `960a0a7de28386e4cc5bd01e26b55a0769a0e2be`.
The installer SHA256 is
`10b63087f2434eb2fd46a0b605182307e6712255a93259968d1e7e825f22f32e`.
The publisher-signed release manifest passed staging verification against
version0.3.3. It is not Windows Authenticode signing, and the updater still
rejects same-version/older releases; this0.3.4 engineering replacement was
installed manually, without weakening anti-rollback checks.

The Win7-target GDI capability probe also ran on Windows11: it captured and
software-H.264 round-tripped one real1280x720 frame. This is evidence for the
GDI/codec path only, not Win7 OS support, a GUI/network test or an FPS claim.

For a separately labeled native-engine two-computer test, the existing
`cross_desktop` fixture can be built with the same Win7 standard library and
passed to `run-two-computer-gate.ps1 -NativeViewer -ViewerExecutable ...`.
The script requires isolated public fixture metadata and logs the executable
path/hash with `gui_test=false`. It must not be reported as a GUI acceptance
test or distributed as a second application.

- Full stable debug workspace suite passed on the owner's actual Win11 laptop;
  opt-in interactive desktop/clipboard tests remain ignored, not passed.
- Three new OS/renderer selection tests passed; all24 desktop library tests
  passed. Formatting and workspace all-target clippy passed.
- The deployed HTTPS/WSS encrypted chat, file transfer and denied-operation
  test passed on September10 with synthetic test data.
- Unified EXE release build and known-loader-blocker audit passed locally.
- During the older September10 attempt, the GUI helper launched the installed Adaptive3 executable instead of the
  requested candidate. Process path and SHA256 exposed the mismatch. That
  window is not evidence that the unified build launched. A direct candidate
  launch was blocked by execution policy. No successful unified GUI, OpenGL,
  remote input or fullscreen test is claimed from this attempt.
- GUI two-computer run34390571379 expired without a viewer connection during
  window-control failures. It remains FAILED. The earlier successful native
  worker run34319516493 is historical evidence only, not a current GUI pass.
- The owner confirmed no Windows7 device, licensed ISO or VM is available.
  No operating system was changed and no third-party ISO was downloaded.

Before a supported release, launch **this same hash** on Win7SP1, Win10 and
Win11; verify identity reopening, validated TLS, attended permission denial and
acceptance, changing video, exact Unicode/keyboard/mouse/wheel, enabled clipboard,
clean disconnect and reconnect. Test all cross-OS pairs. No UAC/login-screen
service, Authenticode certificate, 1080p60 remote acceptance or numerical
reliability guarantee has been added. Version remains0.3.4.

Primary references: [Rust Win7 target](https://doc.rust-lang.org/rustc/platform-support/win7-windows-msvc.html),
[Rust modern Windows baseline](https://blog.rust-lang.org/2024/02/26/Windows-7/),
[windows-result slim errors](https://docs.rs/windows-result/0.3.4/windows_result/struct.Error.html),
[winit Windows runtime API discovery](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/windows/util.rs).
