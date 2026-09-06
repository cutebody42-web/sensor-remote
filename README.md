# SENSOR Remote Access

SENSOR TECHNOLOGY · Designed by ENG Mohamed Sayed.

Version **0.2.0 — Windows development build**, not a production remote-desktop release.

## Windows app, not a website

`SENSOR-Remote.exe` is a native Rust/egui Windows application using wgpu's
Direct3D 12 backend. It embeds the product owner's original SENSOR logo.
There is no HTML interface, browser, WebView, local web server, or hosted site.

Open the EXE to see the persistent device ID, copy its public key, configure
an explicitly trusted peer, and start attended chat or file transfer.
The app starts offline. It does not install a service, open a listener,
modify the firewall, or configure unattended access on startup.

## Implemented and testable

- Windows user-DPAPI identity storage, atomic replacement and initialization locking.
- Direct TCP with mutually pinned Ed25519 identities, ephemeral X25519,
  HKDF-SHA-256, ChaCha20-Poly1305 and mutual encrypted key confirmation.
- Explicit local Accept/Reject, mode-scoped endpoint permission enforcement,
  visible session status and local Stop/disconnect.
- Attended, turn-based encrypted chat; messages remain in memory.
- Integrity-checked file send/receive, 64 KiB chunks, explicit reconnect/resume,
  no-clobber publication and confinement to the chosen receive folder.
- Local verified contacts, persistent alias, signed incoming-session audit.
- A separately tested two-peer authenticated relay library. **The app and CLI
  do not select it yet.** No claim of direct/relay failover.

## Important remaining work

**Remote screen viewing and keyboard/mouse control are not implemented.**
Neither are unattended service/login/UAC, capture/codecs, audio, clipboard,
multi-monitor, printing/Auto Print, recording, VPN/tunnels, discovery/Internet
ID lookup, NAT traversal, enterprise administration, installers or signed updates.
The complete product requested in the brief is therefore not finished.
See [parity](docs/PARITY_MATRIX.md) and [limitations](docs/KNOWN_LIMITATIONS.md).

## Run and build

See [Windows guide](docs/WINDOWS_APP.md). The portable development executables
are `SENSOR-Remote.exe` (desktop) and `SENSOR-CLI.exe` (console endpoint).
They are unsigned: there is no supplied publisher signing certificate.

With Rust 1.98.1, MSVC build tools and Windows SDK installed:

```powershell
.\scripts\verify.ps1 -Release
```

Sources live under `crates/`; the exact dependency graph is in `Cargo.lock`.
[Verification](docs/VERIFICATION.md) records actual checks, not release promises.
