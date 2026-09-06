# Verification Evidence

Date: 2026-09-06

Environment:

- Rust 1.98.1 (`x86_64-pc-windows-msvc`)
- MSVC toolset 14.44.35207
- Windows SDK 10.0.26100.0

Results:

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --workspace --locked` | PASS: 12 unit tests; all doc-test targets |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |

The tests cover device ID/alias invariants, identity persistence and metadata
binding, strict Ed25519 verification, non-contributory X25519 key rejection,
directional key agreement, authenticated encryption, replay rejection, signed
mutual handshake, and fragmented bounded framing.

This evidence applies only to the foundation crates. It is not evidence of a
working remote desktop session or production readiness.
