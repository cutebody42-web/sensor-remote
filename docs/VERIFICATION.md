# Verification evidence — 0.3.0

Historical record. Current release evidence: [RELEASE_0_3_2.md](RELEASE_0_3_2.md).

## Latest verified Internet deployment — 2026-09-06

- Private repository source: https://github.com/cutebody42-web/sensor-remote.
- `cargo test --workspace --release --locked`: **62 passed, 0 failed, 1 ignored**; doc-tests passed. The ignored test requires a real deployed HTTPS URL.
- `cargo check --workspace --release`: passed after selecting the Rustls `ring` crypto provider for native WSS clients.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Added local integration evidence for delayed registration, server-challenge replay rejection, signed listener-role binding, invalid token rejection, concurrent listener/outgoing registration, cancellation, authenticated encrypted chat, a 1,048,737-byte file with SHA-256 verification, and denied file access.
- Fixed Internet registration timeouts, listener replacement, reconnect/cancellation, UI connection-state reporting, saved server configuration, bounded WebSocket messages/connections, Rustls provider selection, and relay write backpressure. Internet capture profile is now at most 1280x720, 15 FPS, 1.5 Mbit/s; aggregate relay cap is 2 Mbit/s by default.
- Railway deployment is live from `railway.json` and `deployment/render/Dockerfile` at `https://sensor-rendezvous-production.up.railway.app`.
- Public `GET /health` returned `status: ok`, `mode: RENDER_TEST`, `database: in_memory_presence`, and the configured 2 Mbit/s relay cap.
- `SENSOR_TEST_SERVER=https://sensor-rendezvous-production.up.railway.app cargo test -p sensor-rendezvous --test internet_security deployed_wss_encrypted_chat_file_and_denial --release --locked -- --ignored`: **1 passed, 0 failed**. This real public-WSS test exercised encrypted chat, a 1,048,737-byte encrypted file with SHA-256 verification, and denied file access.
- The Windows package now embeds `sensor-network.json` for the verified Railway URL and its launcher defaults to that service.

The remaining limitations below are product-scope limitations, not a claim that the public deployment is offline.

Date: 2026-09-06. Local Windows x86_64 MSVC development environment.
Rust 1.98.1, MSVC 14.44.35207, Windows SDK 10.0.26100.0.

## Executed results

| Check | Result |
| --- | --- |
| cargo test --workspace --release --locked | PASS: 62 passed, zero failed, one intentionally ignored deployment test; doc-tests pass |
| cargo fmt --all -- --check | PASS after 0.3 changes |
| cargo build --workspace --release --locked | PASS: graphical, console, relay and rendezvous binaries |
| cargo clippy --workspace --all-targets --locked -- -D warnings | PASS after 0.3 changes |
| Public Railway WSS integration | PASS: encrypted chat, encrypted file/hash, and denied file access through the live HTTPS/WSS service |
| git diff --check | PASS at verification |
| Supplied JPEG versus embedded source | Identical SHA-256; automated exact-asset regression |
| Native GUI launch | Window and real accessibility controls observed |
| Windows PE metadata | x64 (0x8664), graphical subsystem 2, SENSOR company/product/version resources |
| Artifact signing/runtime | Unsigned; imports VCRUNTIME140.dll; no bundled runtime installer |
| Pixel-level/interactive GUI QA | BLOCKED: desktop lock screen; inspection stopped |

The earlier 0.2.0 unoptimized workspace suite passed 48 tests. The 0.3.0
changes add remote media, input, full-duplex transport and Internet coverage;
the current workspace suite has 62 passing tests with zero failures plus one
explicitly ignored deployment test that was run separately against Railway.

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
test data and temporary profiles. The public WSS test uses the external
Railway service from this Windows machine, but it is not a two-physical-machine
or NAT/failover test. User consent in worker tests is a test-provided response;
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
