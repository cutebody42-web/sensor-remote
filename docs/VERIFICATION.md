# Verification evidence — 0.2.0

Date: 2026-09-06. Local Windows x86_64 MSVC development environment.
Rust 1.98.1, MSVC 14.44.35207, Windows SDK 10.0.26100.0.

## Executed results

| Check | Result |
| --- | --- |
| scripts/verify.ps1 -Release -ReleaseTests | PASS |
| cargo fmt --all -- --check | PASS |
| cargo test --workspace --release --locked | PASS: 48 tests, zero failed; doc-test targets pass |
| cargo clippy --workspace --all-targets --locked -- -D warnings | PASS |
| cargo build --workspace --release --locked | PASS: graphical and console Windows executables |
| git diff --check | PASS at verification |
| Supplied JPEG versus embedded source | Identical SHA-256; automated exact-asset regression |
| Native GUI launch | Window and real accessibility controls observed |
| Pixel-level/interactive GUI QA | BLOCKED: desktop lock screen; inspection stopped |

The earlier unoptimized workspace suite passed 42 tests before the final six
regression/binary tests were added. The complete final 48-test suite was run
in optimized release mode. Do not call the release-mode result a debug run.

## Test groups (48 total)

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
- Authenticated pair relay round trip and wrong relay key: 2.

Tests use real local TCP sockets and actual Windows DPAPI, with generated
test data and temporary profiles. They are not two-physical-machine or
Internet/NAT tests. User consent in worker tests is a test-provided response;
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

No complete remote desktop, video/input/codecs, service/UAC, printing, signed
installer/updater, OS/GPU compatibility matrix, hosted CI, extended fuzzing,
independent security review, Internet failover, or 8+ hour session evidence.
The requested full product is not finished or production ready.
