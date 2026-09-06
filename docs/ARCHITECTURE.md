# Architecture

SENSOR is a native Windows desktop product. `sensor-desktop` owns the visible
egui/wgpu window, never a web view. It embeds the original supplied logo.
No website or administrative web console was built in this iteration.

## Implemented module boundaries

| Crate | Responsibility |
| --- | --- |
| sensor-desktop | Native window; bounded UI/worker channels; contacts; visible consent |
| sensor-client | Shared attended endpoint operations and SENSOR-CLI |
| sensor-core | Validated nine-digit IDs and aliases |
| sensor-identity | Locked atomic protected identity persistence |
| sensor-windows | Actual current-user DPAPI implementation |
| sensor-crypto | Ed25519/X25519/HKDF/ChaCha record primitives and zeroization |
| sensor-protocol | Bounded version-2 frames and signed wire types |
| sensor-session | Pinned handshake; consent and permission state |
| sensor-transport | Deadline-bound TCP, mutual key confirmation, abort handle |
| sensor-files | Root-confined resumable checksummed transfer, no overwrite |
| sensor-audit | Signed chained local metadata audit |
| sensor-relay | Preauthorized pair forwarding library; not integrated into UI |

## Current application flow

Explicit Start -> direct TCP -> mutual pinned authentication/key confirmation
-> local Accept/Reject -> chat OR file transfer -> close -> incoming audit.

Workers own sockets and operation state. UI owns only a cancellation handle,
bounded events and one-shot consent/reply senders. Closing a prompt/window
cannot implicitly grant access. Network I/O does not run on the render loop.
One profile has one native window and one active job. No privileged service runs.

## Still planned

Rendezvous/NAT/proxy/direct-first fallback, Windows capture/video/input/audio,
service/UAC/unattended operation, printing, VPN, enterprise control plane,
signed packaging/update and other platforms remain missing. Existing documents
for those systems are design plans, not implemented modules or release claims.
