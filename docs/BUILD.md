# Build

## Windows prerequisites

- Rust 1.98.1 x86_64-pc-windows-msvc, pinned in rust-toolchain.toml.
- Visual Studio C++ Build Tools with x64 MSVC and Windows SDK.
- Direct3D 12 capable graphics environment for the current native renderer.
- Microsoft Visual C++ x64 runtime providing VCRUNTIME140.dll on the machine
  running this build. The executable dependency table was inspected; this
  development package does not bundle or install that runtime.

Run from the repository:

```powershell
.\scripts\verify.ps1 -Release
```

Equivalent checks are `cargo fmt --all -- --check`,
`cargo test --workspace --locked`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`, and
`cargo build --workspace --release --locked`.

Artifacts: `target/release/SENSOR-Remote.exe` (Windows-subsystem native GUI),
`target/release/SENSOR-CLI.exe` (console endpoint), `target/release/sensor-relay.exe`
(provisioned relay), and `target/release/sensor-rendezvous.exe` (local
Internet-rendezvous Web Service binary).
The build embeds the supplied logo, Windows icon/version resources and a
PerMonitorV2, asInvoker manifest. It requests no administrator privileges.

Native rendering uses egui/eframe 0.33.3 with explicitly enabled wgpu DX12
features; merely enabling eframe's wgpu feature does not select a native
backend when default features are disabled.

See [verification](VERIFICATION.md) for actual results. Hosted CI and the
supported Windows VM matrix must still be run; local checks are not substitutes.

To test optimized binaries, use `scripts/verify.ps1 -Release -ReleaseTests`.
Packaging uses PowerShell 7: `scripts/package.ps1`. The destination must be new;
existing packages are never overwritten. It collects dependency license files,
source revision and executable SHA-256 sums. This is not publisher signing.
