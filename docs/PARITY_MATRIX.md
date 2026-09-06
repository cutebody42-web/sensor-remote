# SENSOR Remote Access Parity Matrix

This is an internal engineering document. The benchmark product is used only
as a behavioral reference; no proprietary code, private protocol, key, or
graphic asset is used.

Status values:

- `NOT STARTED`
- `IN PROGRESS`
- `IMPLEMENTED`
- `TESTED`
- `PRODUCTION READY`

No row is marked `IMPLEMENTED` unless the behavior is present in code. No row
is marked `PRODUCTION READY` without the release evidence defined in the test
plan.

| Benchmark capability | SENSOR equivalent | Architecture | Platform support | Implementation status | Test status | Known gaps |
| --- | --- | --- | --- | --- | --- | --- |
| Persistent device identity | SENSOR Device ID and alias | `sensor-core`, `sensor-identity` | Core; OS protector pending | IN PROGRESS | Unit tests | Windows DPAPI provider and migration policy |
| Authenticated connection | Signed hello and ephemeral session keys | `sensor-session`, `sensor-crypto` | Core | TESTED | Handshake and tamper tests | Trust/authorization service |
| Direct and relay connectivity | Direct-first transport with relay fallback | `sensor-transport`, future rendezvous/relay services | Not started | NOT STARTED | None | ICE/STUN/QUIC/TCP/proxy implementation |
| Remote desktop | Capture, encode, decode, GPU render | Future `sensor-windows`, `sensor-media`, clients | Not started | NOT STARTED | None | Entire media path |
| Remote input | Input event channel and platform backend | Future `sensor-input`, `sensor-windows` | Not started | NOT STARTED | None | Secure desktop and permission enforcement |
| Unattended access | Explicit service-backed unattended profiles | Future `sensor-service` | Windows priority | NOT STARTED | None | Service, login screen, 2FA, ACL |
| File transfer | Independent and in-session dual-pane transfer | Future `sensor-files` | Not started | NOT STARTED | None | Resumability, integrity, policy |
| Clipboard | Permission-controlled bidirectional clipboard | Future `sensor-session`, platform backends | Not started | NOT STARTED | None | Text/image/file policies |
| Remote audio | Low-latency WASAPI/Opus path | Future `sensor-audio` | Windows priority | NOT STARTED | None | Capture, sync, device selection |
| Remote printing | SENSOR Remote Printer | Future `sensor-print`, Windows service | Windows priority | NOT STARTED | None | Driver, spooler, local printer queue |
| Auto Print | Explicit rule engine and job queue | Future `sensor-print` and management API | Windows priority | NOT STARTED | None | Watchers, retries, audit |
| Recording | Consent-aware session/screen recorder | Future `sensor-media`, `sensor-session` | Not started | NOT STARTED | None | Codec/container, metadata, storage |
| Chat/whiteboard | Encrypted session collaboration | Future protocol modules | Not started | NOT STARTED | None | UI, history, permissions |
| Wake-on-LAN | LAN helper and topology registry | Future rendezvous/management | Not started | NOT STARTED | None | Helper discovery and authorization |
| Privacy mode | Visible authorized blanking mode | Future Windows backend/service | Windows priority | NOT STARTED | None | Supported display/driver mechanisms |
| Address books/discovery | Organization and LAN device directories | Management backend | Not started | NOT STARTED | None | Sync, ACL, privacy controls |
| Tunnels/VPN | Explicit encrypted tunnel and point-to-point VPN | Future `sensor-tunnel`, `sensor-vpn` | Not started | NOT STARTED | None | Adapter, routing, policy |
| Management/RBAC/SSO/API | SENSOR Management Console and REST API | Future API/auth services | Not started | NOT STARTED | None | PostgreSQL, OIDC, Entra, LDAP, audit |
| Deployment/updates | Signed installers, service, staged updates | Future deployment services | Windows priority | NOT STARTED | None | MSI, ADMX/ADML, signing infrastructure |

Public benchmark research captured on 2026-09-06 is summarized in
[`BENCHMARK_RESEARCH.md`](BENCHMARK_RESEARCH.md). New capabilities discovered
during future research must be added here before implementation claims are
made.
