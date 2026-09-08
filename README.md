# SENSOR Remote Access

SENSOR TECHNOLOGY · Designed by ENG Mohamed Sayed.

Version **0.3.3 — Windows attended remote-desktop build with Internet
transport**. The shipped package is configured for the verified Railway
rendezvous service; this is still a development release without Authenticode,
not a complete AnyDesk-equivalent product.

## Windows app, not a website

`SENSOR-Remote.exe` is a native Rust/egui Windows application using wgpu's
Direct3D 12 backend. It embeds the product owner's original SENSOR logo.
There is no HTML interface, browser, WebView, local web server, or hosted site.

Open the EXE to see the persistent device ID, enter the other device's ID,
and request attended chat, file transfer, or remote desktop viewing/control.
Saved contact keys are enforced; an unknown device requires visible approval.
Verify and save the full public key for stronger first-contact protection.
With `SENSOR_MODE=RENDER_TEST` and
`SENSOR_SERVER=https://sensor-rendezvous-production.up.railway.app`, the native app keeps an
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
- Explicit, revocable 30-day unattended view/control grants for one verified
  key while Windows is signed in and unlocked. Windows user-DPAPI binds the
  grant to the local identity. Clipboard and files still require consent.
- Optional launch after Windows sign-in (not a pre-login service).
- Publisher Ed25519-signed offline update verification and staging, with
  compiled trust anchor, expiry, downgrade, size and SHA-256 checks.
  See [SIGNED_UPDATES.md](docs/SIGNED_UPDATES.md); Authenticode is separate.
- Explicitly granted text clipboard synchronization (64 KiB UTF-8 limit),
  with a live disable switch, initial-content suppression and echo prevention.
- Fit/actual-size viewing and fullscreen; native per-user installer source,
  static Windows C runtime, dependency inventory and CycloneDX SBOM generation.
- A provisioned two-peer authenticated relay path. `sensor-relay.exe` is a
  self-hostable single-pair forwarder; the GUI can use it when both endpoints
  are configured with its address and public key. Endpoint application records
  remain encrypted end-to-end.
- Internet rendezvous transport: signed device registration, ephemeral
  online presence lookup, token-authenticated pairing, heartbeat, reconnect
  backoff, HTTPS health endpoint, and opaque binary WSS relay. The service is
  implemented by the native `sensor-rendezvous` binary in
  `crates/sensor-rendezvous`.

## Important remaining work

The attended screen/control path, provisioned relay path, and Internet
path are implemented, but the complete product requested in the brief
is not finished. Installed unattended service/login/UAC, audio,
printing/Auto Print, recording, VPN/tunnels, durable accounts/device
directory, NAT traversal, direct/relay failover, enterprise administration,
Authenticode-signed installers, automatic update distribution, and broad cross-machine/Internet certification remain.

On 2026-09-07 the laptop and a separate cloud Windows computer passed real
H.264 screen capture/decoding, synthetic mouse click, Unicode keyboard and
wheel input through public WSS (GitHub run 34153021049, attempt 2). That used
the native worker test client, not the installed GUI. See
[0.3.3 fixes and current evidence](docs/RELEASE_0_3_3.md) for the subsequent
ID-only handshake and GUI input fixes. No numerical reliability guarantee
or login/UAC support is claimed.
See [parity](docs/PARITY_MATRIX.md), [deployment](docs/DEPLOYMENT.md), and
[limitations](docs/KNOWN_LIMITATIONS.md).

## Run and build

See [Windows guide](docs/WINDOWS_APP.md). The portable development executables
are `SENSOR-Remote.exe` (desktop), `SENSOR-CLI.exe` (console endpoint), and
`sensor-relay.exe` (self-hosted provisioned relay). `sensor-rendezvous` is the
Linux Web Service binary built by `deployment/render/Dockerfile`.
They are unsigned: there is no supplied publisher signing certificate.
This build requires 64-bit Windows 10/11, Direct3D 12 and Windows Media
Foundation. The C runtime is statically linked: no separate VCRUNTIME140.dll
installation is needed. Windows N editions need their Media Feature Pack.
`scripts/installer.ps1` builds an unsigned, current-user NSIS installer from
the output of `scripts/package.ps1`; it installs no service or firewall rule.

With Rust 1.98.1, MSVC build tools and Windows SDK installed:

```powershell
.\scripts\verify.ps1 -Release
```

Sources live under `crates/`; the exact dependency graph is in `Cargo.lock`.
[Verification](docs/VERIFICATION.md) records actual checks, not release promises.

## Internet setup

The repository includes the Railway Docker configuration and an [Internet
deployment guide](deployment/render/README.md). The verified service is:

`https://sensor-rendezvous-production.up.railway.app`

The Windows package includes `sensor-network.json` with that URL and
`Start-SENSOR-Internet.cmd` starts the native app with the Internet route. The
first unknown connection requires visible local approval. Device-ID lookup
alone is not out-of-band key verification; compare public keys for sensitive
work and save a trusted contact. The rendezvous presence directory is in memory, so a
service restart makes open clients register again; this is not durable account
infrastructure or an unattended-access service.
