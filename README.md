# SENSOR Remote Access

SENSOR Remote Access is the independent remote support and remote administration
platform for SENSOR TECHNOLOGY.

Designed by ENG Mohamed Sayed.

## Current status

This repository is at the M0/M1 foundation stage. It contains real, tested
building blocks for device identity, authenticated handshake, bounded wire
framing, forward-secret key derivation, authenticated encryption, and replay
rejection. It is not a production release and it does not yet include a
Windows desktop client, capture pipeline, service, relay, or management
console. Those gaps are recorded rather than hidden behind UI placeholders.

The current version is `0.1.0`.

## Workspace layout

| Crate | Responsibility |
| --- | --- |
| `sensor-core` | Stable device IDs and domain validation |
| `sensor-crypto` | Ed25519 identity, X25519 ephemeral keys, HKDF, ChaCha20-Poly1305, replay guard |
| `sensor-identity` | Durable identity file format with an explicit OS key-protection boundary |
| `sensor-protocol` | Versioned handshake messages and bounded protocol frames |
| `sensor-session` | Mutual-authentication handshake and secure channel construction |
| `sensor-transport` | Incremental length-delimited framing |

## Build and test

Install the stable Rust toolchain, then run:

```text
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The required toolchain is pinned by `rust-toolchain.toml`.

## Security posture

The cryptographic layer is intentionally small and composed from established
primitives. It does not claim transport security until identity trust policy,
key protection, endpoint authorization, direct/relay negotiation, and a full
security review are complete. See [`docs/SECURITY.md`](docs/SECURITY.md) and
[`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md).

## Status tracking

The implementation status is maintained in
[`docs/PARITY_MATRIX.md`](docs/PARITY_MATRIX.md). A feature is not marked
implemented merely because a type, screen, or button exists.

