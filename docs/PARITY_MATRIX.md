# SENSOR Remote Access parity matrix — 0.3.0

Official behavioral research is recorded in [BENCHMARK_RESEARCH.md](BENCHMARK_RESEARCH.md).
This is clean-room work: no benchmark source code, private protocol or branding.
The supplied SENSOR logo is used with the owner's request.

Statuses: NOT STARTED, IN PROGRESS, IMPLEMENTED, TESTED, PRODUCTION READY.
TESTED applies only to the explicitly scoped behavior/test listed; it does not
mean complete parity. No row is PRODUCTION READY.

| Benchmark capability | SENSOR equivalent | Architecture | Platform | Status | Evidence | Remaining gap |
| --- | --- | --- | --- | --- | --- | --- |
| Native desktop client | Windows egui/wgpu EXE with supplied logo | sensor-desktop | Windows | IN PROGRESS | Compiles; native window/accessibility tree observed; remote page and worker E2E | Visual QA paused at locked desktop; OS/GPU/accessibility matrix |
| Persistent ID/alias | DPAPI-backed nine-digit identity, local alias | core/identity/windows | Windows | TESTED | Real DPAPI, atomic reopen, eight-way initialization race | Global registration/collisions, backup, cross-user migration, reboot |
| Authenticated sessions | Pinned signed target-bound handshake and key confirmation | crypto/session/transport | Core + Windows endpoint | TESTED | Real TCP, impostor/tamper/replay/size tests | Independent security review, directory trust lifecycle |
| Attended permissions | Visible Accept/Reject, chat or file scope, Stop | session/client/desktop | Windows | TESTED | Worker consent, rejection, revocation and cross-mode denial | Custom profiles UI, mixed operations, service policy |
| Direct transport | Explicit direct TCP socket | transport | Windows endpoint | TESTED | Real sockets and large encrypted records | QUIC/TLS/proxy, UDP candidates |
| Relay | Authenticated preconfigured pair forwarding | relay | Core | TESTED | Real relayed pinned handshake and data | Library only; not GUI/CLI integrated or deployed |
| NAT/failover/reconnect | Planned direct-first connection orchestration | transport/rendezvous | Planned | NOT STARTED | None | ICE/STUN, ID lookup, relay selection/failover |
| Remote desktop/video | DXGI Desktop Duplication + Media Foundation H.264 + native wgpu viewport | windows/media/desktop | Windows attended | IN PROGRESS | Codec encode/decode test, bounded fragment tests, permission-gated capture path, GUI integration | Unlocked two-device/long-run/GPU matrix; no H.265/AV1 yet |
| Remote keyboard/mouse | SendInput with consent, focus and held-state release | windows/input/desktop | Windows attended | IN PROGRESS | Permission-gate tests, bounded input protocol, GUI mapping | Unlocked two-device/UIPI/elevated-app matrix; no secure-desktop/UAC control |
| Unattended/login/UAC/CAD | Planned installed service | service/windows | Planned Windows | NOT STARTED | None | Explicit credentials/MFA, IPC, secure desktop, reboot |
| File transfer | Flat-file send/receive with verified resume | files/client/desktop | Windows | IN PROGRESS | Chunk/full hash, TCP worker E2E, root confinement, no-clobber | No dual-pane manager/folders/queue; 1 GiB offer cap; manual resume |
| Clipboard | Planned permission-controlled text/image/files | session/windows | Planned | NOT STARTED | None | Entire clipboard synchronization |
| Multi-monitor | DXGI display enumeration and attended remote monitor selection | media/windows/desktop | Windows attended | IN PROGRESS | Display wire types and selection/reconfigure path | Unlocked multi-monitor verification and orientation matrix |
| Audio | Planned WASAPI/Opus | audio/windows | Planned | NOT STARTED | None | Capture, playback, sync |
| Remote printing | SENSOR Remote Printer | print/service | Planned Windows | NOT STARTED | None | Driver, spooler, local printer queue |
| Auto Print | Explicit monitored rules and job queue | print | Planned Windows | NOT STARTED | None | Watchers, retries, physical printing and audit |
| Recording | Planned consent-aware recording | media/session | Planned | NOT STARTED | None | Codec/container/storage |
| Chat | Attended encrypted turn-based chat | client/desktop | Windows | IN PROGRESS | Bidirectional worker E2E; file operation denied in chat | Concurrent messaging, optional history/export |
| Whiteboard | Planned session collaboration | client/media | Planned | NOT STARTED | None | Entire drawing/synchronization behavior |
| Wake-on-LAN | Planned authorized LAN helper | rendezvous | Planned | NOT STARTED | None | Discovery/topology/helper |
| Privacy mode | Planned explicit blanking | windows/service | Planned Windows | NOT STARTED | None | OS display/control mechanism |
| Address book | Verified local contact pins | desktop | Windows | IN PROGRESS | Bounded atomic contact persistence/corruption test | Organization sync, groups, discovery, trust rotation UI |
| Session history/audit | Signed incoming operation log | audit/client | Windows/core | IN PROGRESS | Signature/hash-chain tamper tests and E2E records | Outgoing/full fields, CSV/search, central checkpoint/retention |
| Diagnostics | Actual job state, public identity, acknowledged bytes, remote format/codec | desktop | Windows | IN PROGRESS | Worker state events, native accessibility tree, remote format status | RTT/loss/bitrate telemetry and unlocked visual QA |
| Tunnels/VPN | Planned encrypted explicit routes | tunnel/vpn | Planned | NOT STARTED | None | Adapter/routing/permissions UI |
| Enterprise management/SSO/API | Planned organizational services | management/auth | Planned | NOT STARTED | None | All organization/RBAC/OIDC/Entra/LDAP/database/API behavior |
| Portable deployment | GUI and CLI Windows binaries | desktop/client | Windows | IN PROGRESS | Local builds; resource/icon manifest | Unsigned; no certified Windows support matrix |
| Installer/MSI/updates | Planned signed lifecycle | deployment/update | Planned | NOT STARTED | None | Signing key/infrastructure, installers, rollback |
| Cross-platform clients | Planned platform-specific ports | platform/client | Planned | NOT STARTED | Core library design only | Native clients and OS restrictions tests |
| Performance/release gate | Full product validation | all | Planned matrix | NOT STARTED | No release certification | Two-machine/VM/NAT/loss/codec/8h+ tests and independent review |

See [KNOWN_LIMITATIONS.md](KNOWN_LIMITATIONS.md) for exact restrictions.
The full product brief is not completed by these implemented subsets.
