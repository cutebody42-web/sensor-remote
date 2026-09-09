# Windows 7 engineering boundary — NOT SUPPORTED

The shipping 0.3.4 executable targets Windows 10/11 x64. It must not be relabeled
as Windows 7 compatible. No Win7 VM, successful legacy build, or Win7 launch has
been verified in this session.

## Implemented separation

`sensor-windows::desktop::Capture` dispatches to DXGI on modern Windows and a
separate GDI/DIB backend on detected 6.1. Modern DXGI access-denied failures do
**not** trigger a fallback. Both backends require granted view permission and
an unlocked Default desktop before and after capture. RtlGetVersion detects
the actual OS. The documented Win7 reversed WTS lock flags are handled.

This independent Cargo package omits eframe/wgpu/DX12 and exercises only local,
explicitly opted-in GDI capture and software H.264. It neither listens nor grants
remote permissions. `scripts/build-legacy-probe.ps1` selects the Tier-3
`x86_64-win7-windows-msvc` target with a pinned nightly and rebuilt std. The
probe is a first executable test of the shared backend, **not a legacy app**.

## Exact remaining blockers and next gates

| Area | Audit / required work |
|---|---|
| Rust/std | Default pc-windows-msvc has Windows 10 minimum. Win7 Tier 3 has no official prebuilt std; build-std toolchain >= current dependency MSRV required, then PE import audit. |
| GUI | Current eframe/wgpu 27 DX12 frontend is excluded. A native Win32 frontend or separately maintained compatible renderer is needed; merely switching capture is insufficient. |
| TLS | Current rustls/ring/webpki path must be built and exercised on Win7 with current trusted roots. Never disable certificate validation or downgrade TLS. |
| Windows bindings | Generated windows/windows-sys bindings do not guarantee OS compatibility. Audit final imports (incl. identity/arboard and UCRT) on the legacy target. |
| Media Foundation | Win7 supports H.264, but newer low-latency/rate-control properties may be unavailable. Code probes optional ICodecAPI properties and software initialization; actual Win7 codec gate remains required. |
| Capture | GDI uses top-down 32-bit DIB, BitBlt and explicit stride; no Desktop Duplication requirement in legacy backend. DPI/multi-monitor/mode-change behavior still needs Win7 tests. |
| DPAPI / input / clipboard | Underlying Windows APIs exist on Win7; dependency import audit, scoped consent, clipboard opt-in, release-on-close, Unicode and held-key tests are not yet run on Win7. |
| Installer / runtime | Separate legacy NSIS package with exact VC/UCRT prerequisites and OS guard required. Do not install the modern binary or ship an unverified runtime bundle. |
| Startup / updates | No service, login/UAC bypass or silent grant. Legacy updates need their own verified platform identifier and same signature checks. |

References: [Rust baseline](https://blog.rust-lang.org/2024/02/26/Windows-7/),
[Tier-3 build requirements](https://doc.rust-lang.org/rustc/platform-support/win7-windows-msvc.html),
[WTS lock flags](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ns-wtsapi32-wtsinfoex_level1_w),
[MF H.264 availability](https://learn.microsoft.com/en-us/windows/win32/medfound/h-264-video-encoder).
