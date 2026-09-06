# SENSOR Remote Access

SENSOR TECHNOLOGY · Designed by ENG Mohamed Sayed.

Version **0.3.0 — Windows attended remote-desktop build with Render Internet
transport**. The Render service is a temporary proof-of-concept deployment;
this is not yet a production AnyDesk replacement.

## Windows app, not a website

`SENSOR-Remote.exe` is a native Rust/egui Windows application using wgpu's
Direct3D 12 backend. It embeds the product owner's original SENSOR logo.
There is no HTML interface, browser, WebView, local web server, or hosted site.

Open the EXE to see the persistent device ID, copy its public key, configure
an explicitly trusted peer, and start attended chat, file transfer, or remote
desktop viewing/control. With `SENSOR_MODE=RENDER_TEST` and
`SENSOR_SERVER=https://<your-service>.onrender.com`, the native app keeps an
Internet WSS listener registered while it is open. It remains a Windows app:
there is no HTML interface, browser dependency, or hosted website endpoint on
the client.

## Implemented and testable

- Windows user-DPAPI identity storage, atomic replacement and initialization locking.
- Direct TCP with mutually pinned Ed25519 identities, ephemeral X25519,
  HKDF-SHA-256, ChaCha20-Poly1305 and mutual encrypted key confirmation.
- Explicit local Accept/Reject, mode-scoped endpoint permission enforcement,
  visible session status and local Stop/disconnect.
- Attended, turn-based encrypted chat; messages remain in memory.
- Integrity-checked file send/receive, 64 KiB chunks, explicit reconnect/resume,
  no-clobber publication and confinement to the chosen receive folder.
- Attended remote desktop sessions: DXGI Desktop Duplication capture on the
  ordinary interactive desktop, Windows Media Foundation H.264 encoding and
  decoding, bounded encrypted frame fragments, monitor selection, pointer
  updates, and permission-checked mouse/keyboard/text/wheel input. The GUI
  viewport is native egui/wgpu; it is not an HTML canvas or JPEG stream.
- Local verified contacts, persistent alias, signed incoming-session audit.
- A provisioned two-peer authenticated relay path. `sensor-relay.exe` is a
  self-hostable single-pair forwarder; the GUI can use it when both endpoints
  are configured with its address and public key. Endpoint application records
  remain encrypted end-to-end.
- Temporary Render Internet transport: signed device registration, ephemeral
  online presence lookup, token-authenticated pairing, heartbeat, reconnect
  backoff, HTTPS health endpoint, and opaque binary WSS relay. The service is
  implemented by the native `sensor-rendezvous` binary in
  `crates/sensor-rendezvous`.

## Important remaining work

The attended screen/control path, provisioned relay path, and temporary Render
Internet path are implemented, but the complete product requested in the brief
is not finished. Installed unattended service/login/UAC, audio, clipboard,
printing/Auto Print, recording, VPN/tunnels, durable accounts/device
directory, NAT traversal, direct/relay failover, enterprise administration,
installers, signed updates, and cross-machine/Internet certification remain.

Actual DXGI capture and SendInput testing must be performed on an unlocked
interactive desktop. The current locked test session has verified the code's
permission gates and codec path, but not a real two-computer session.
See [parity](docs/PARITY_MATRIX.md), [deployment](docs/DEPLOYMENT.md), and
[limitations](docs/KNOWN_LIMITATIONS.md).

## Run and build

See [Windows guide](docs/WINDOWS_APP.md). The portable development executables
are `SENSOR-Remote.exe` (desktop), `SENSOR-CLI.exe` (console endpoint), and
`sensor-relay.exe` (self-hosted provisioned relay). `sensor-rendezvous` is the
Render/Linux Web Service binary built by `deployment/render/Dockerfile`.
They are unsigned: there is no supplied publisher signing certificate.
Running this development build requires Direct3D 12 and the Visual C++ x64
runtime (VCRUNTIME140.dll). Neither an installer nor a runtime installer is bundled.

With Rust 1.98.1, MSVC build tools and Windows SDK installed:

```powershell
.\scripts\verify.ps1 -Release
```

Sources live under `crates/`; the exact dependency graph is in `Cargo.lock`.
[Verification](docs/VERIFICATION.md) records actual checks, not release promises.

## Temporary Internet setup

The repository includes [render.yaml](render.yaml), the Render Dockerfile, and
an [Internet deployment guide](deployment/render/README.md). Deploy the
Blueprint on Render Free, copy its HTTPS service URL, and set `SENSOR_SERVER`
to that URL on both Windows PCs. In the Windows package,
`Start-SENSOR-Internet.cmd` prompts for that URL and starts the native app with
the correct environment. The first connection still requires verified
device keys and visible local approval. Render Free may cold-start and its
in-memory presence directory is intentionally non-durable; this is documented
and is not a substitute for production account infrastructure.
