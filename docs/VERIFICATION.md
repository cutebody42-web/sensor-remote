# Verification evidence — 0.3.0

Date: 2026-09-06. Local Windows x86_64 MSVC development environment.
Rust 1.98.1, MSVC 14.44.35207, Windows SDK 10.0.26100.0.

## Executed results

| Check | Result |
| --- | --- |
| cargo test --workspace --release --locked | PASS: 59 tests, zero failed; doc-tests pass |
| cargo fmt --all -- --check | PASS after 0.3 changes |
| cargo build --workspace --release --locked | PASS: graphical, console, relay and rendezvous binaries |
| cargo clippy --workspace --all-targets --locked -- -D warnings | PASS after 0.3 changes |
| Render rendezvous integration | PASS: two native clients exchanged opaque bytes through the WSS profile |
| git diff --check | PASS at verification |
| Supplied JPEG versus embedded source | Identical SHA-256; automated exact-asset regression |
| Native GUI launch | Window and real accessibility controls observed |
| Windows PE metadata | x64 (0x8664), graphical subsystem 2, SENSOR company/product/version resources |
| Artifact signing/runtime | Unsigned; imports VCRUNTIME140.dll; no bundled runtime installer |
| Pixel-level/interactive GUI QA | BLOCKED: desktop lock screen; inspection stopped |

The earlier 0.2.0 unoptimized workspace suite passed 48 tests. The 0.3.0
changes add remote media, input, full-duplex transport and Render Internet
coverage; the current workspace suite is 59 tests with zero failures.

## Test groups (scoped counts listed)

- Core invariants and deserialization: 4.
- Crypto, replay/gaps/AAD/tamper and redacted key Debug: 5.
- Identity serialization/protection context: 2.
- Actual Windows DPAPI and concurrent identity initialization: 3.
- Protocol framing, malformed versions/lengths, mutation corpus: 3.
- Pinned handshake and consent/permission state: 5.
- Real encrypted TCP, impostor and oversized-frame rejection: 4.
- File confinement, checksum, resume, revocation and concurrent-writer lock: 4.
- Signed audit chain integrity and partial/corrupt log rejection: 2.
- Endpoint socket-level file/chat/rejection tests: 3.
- Compiled CLI subprocess identity/argument behavior: 2.
- Desktop worker/model tests: 4.
- Desktop worker socket-level accepted/rejected file and bidirectional chat: 3.
- Actual native graphics backend feature and exact owner-supplied logo: 2.
- Actual Windows software H.264 encode/decode color-preservation test: 1.
- Full-duplex encrypted reader/writer transport test: 1.
- Media fragment/pixel/input validation: 3.
- Authenticated pair relay round trip and wrong relay key: 2.
- Render registration and two-client opaque WSS relay: 2.

Tests use real local TCP sockets and actual Windows DPAPI, with generated
test data and temporary profiles. They are not two-physical-machine or
Internet/NAT tests. The Render test uses a local service process, not the
external Render account. User consent in worker tests is a test-provided response;
the native Accept/Reject buttons were not clicked while Windows was locked.

## Startup issue found and fixed

The first native launch panicked because eframe default features were disabled
and no native wgpu backend was enabled. Explicit DX12/WGSL features fixed it.
The corrected build exposed a native SENSOR Remote Access window containing
the actual device ID, original-logo image control, peer/key inputs, disabled
connection buttons until trust is configured, and Offline state.

The returned screenshot showed Windows locked, so visual inspection stopped
as required. No screenshot of a finished app layout was verified. No actions
were performed through the lock screen. Final visual QA needs an unlocked desktop.

## What this does not establish

The remote desktop implementation is present, but no unlocked two-device,
UIPI/elevated-app, Internet failover, service/UAC, printing, signed
installer/updater, OS/GPU compatibility matrix, hosted CI, extended fuzzing,
independent security review, or 8+ hour session evidence exists. The complete
product is not production ready.
