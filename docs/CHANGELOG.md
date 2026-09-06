# Changelog

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
