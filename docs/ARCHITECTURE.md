# Architecture

SENSOR is a native Windows desktop product. `sensor-desktop` owns the visible
egui/wgpu window, never a web view. It embeds the original supplied logo.
No client website or administrative web console was built. The temporary
Render transport is a native Rust Web Service for HTTPS health and WSS relay.

## Implemented module boundaries

| Crate | Responsibility |
| --- | --- |
| sensor-desktop | Native window; bounded UI/worker channels; contacts; visible consent; remote viewport |
| sensor-client | Shared attended endpoint operations and SENSOR-CLI |
| sensor-core | Validated nine-digit IDs and aliases |
| sensor-identity | Locked atomic protected identity persistence |
| sensor-windows | Actual current-user DPAPI implementation |
| sensor-media | Validated remote display/input/video wire types and bounded pixel transforms |
| sensor-crypto | Ed25519/X25519/HKDF/ChaCha record primitives and zeroization |
| sensor-protocol | Bounded version-2 frames and signed wire types |
| sensor-session | Pinned handshake; consent and permission state |
| sensor-transport | Deadline-bound TCP, mutual key confirmation, abort handle, full-duplex split |
| sensor-files | Root-confined resumable checksummed transfer, no overwrite |
| sensor-audit | Signed chained local metadata audit |
| sensor-relay | Preauthorized pair forwarding library; not integrated into UI |
| sensor-render | Client-side signed registration, WSS pairing, heartbeat and opaque relay bridge |
| sensor-rendezvous | Render Web Service: in-memory presence, pairing, health and opaque binary relay |

## Current application flow

Explicit Start -> direct TCP, provisioned relay, or Render WSS -> mutual pinned
authentication/key confirmation -> local Accept/Reject -> chat, file transfer,
view or control -> close -> incoming audit. In Render mode, signed device
registration and ephemeral presence pair the two WebSocket connections before
the existing endpoint handshake runs. Remote control adds DXGI capture, Media
Foundation H.264 and native viewport/input only after the local permission
decision.

Workers own sockets and operation state. UI owns only cancellation handles,
bounded events and one-shot consent/reply senders. Closing a prompt/window
cannot implicitly grant access. Network I/O does not run on the render loop.
One profile has one foreground session plus an optional Render presence
listener. No privileged service runs.

## Still planned

Durable rendezvous/account storage, NAT/proxy/direct-first fallback, H.265/AV1/
audio, service/UAC/unattended operation, printing, VPN, enterprise control
plane, signed packaging/update and other platforms remain missing. Existing
documents for those systems are design plans, not implemented modules or
release claims.
