# Changelog

## 0.2.0 - 2026-09-06 (Windows development build)

- Added native egui/wgpu Direct3D 12 Windows application and original owner-supplied logo.
- Separated graphical SENSOR-Remote from SENSOR-CLI; added native resources/manifest.
- Added actual user-DPAPI identity storage, atomic writes and initialization locking.
- Required explicit peer pins and signed target binding; bumped wire protocol to 2.
- Added real deadline-bound TCP transport and mutual encrypted key confirmation.
- Added visible consent, mode-scoped permission enforcement and local cancellation.
- Added checksummed/root-confined file transfer, manual resume and no-clobber finalization.
- Added encrypted turn-based chat, local verified contacts, alias and diagnostics.
- Added signed hash-chain incoming-session audit and authenticated pair relay library.
- Added actual socket/DPAPI/worker integration and malformed-protocol regression tests.
- Full remote desktop, unattended service, printing and enterprise release remain missing.

## 0.1.0 - 2026-09-06

Initial foundation slice:

- Created a Rust workspace with isolated core, identity, crypto, protocol,
  session, and transport crates.
- Added device ID/alias primitives.
- Added Ed25519 identity and X25519 ephemeral key wrappers.
- Added HKDF-derived directional session keys.
- Added ChaCha20-Poly1305 authenticated records with ordered replay rejection.
- Added signed mutual-authentication handshake.
- Added bounded versioned frame encoding and incremental decoding.
- Added repository audit, architecture decision, security, threat, roadmap,
  build, deployment, protocol, printing, testing, and parity documentation.
- Verified 12 unit tests, all doc-test targets, formatting, and strict Clippy
  with warnings denied on the Windows MSVC toolchain.
