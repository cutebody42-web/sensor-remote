# Implementation Roadmap

The roadmap is ordered by dependency and release risk. Every milestone ends
with build, tests, defect fixes, and a documented status update.

| Milestone | Scope | Exit gate | Status |
| --- | --- | --- | --- |
| M0 | Workspace, CI, security model, protocol decisions, audit documents | Reproducible build and reviewable boundaries | IN PROGRESS |
| M1 | Device identity, rendezvous contract, authenticated handshake | Two peers authenticate and establish tested directional keys | IN PROGRESS |
| M2 | Direct/relay transport, NAT traversal, reconnect | Direct-first selection and tested relay recovery | IN PROGRESS: direct TCP and isolated relay library; no selection/NAT |
| M3 | Windows capture, codecs, GPU render, input | Real interactive Windows session on supported matrix | NOT STARTED |
| M4 | Windows service, unattended access, secure desktop workflows | Reboot/login/lock/UAC authorization tests pass | NOT STARTED |
| M5 | Clipboard, files, multi-monitor, audio | Permission-enforced transfer and media tests pass | IN PROGRESS: attended flat-file transfer only |
| M6 | SENSOR Remote Printer and Auto Print | Printed jobs are transported, queued, logged, and retryable | NOT STARTED |
| M7 | Recording, chat, whiteboard, Wake-on-LAN, privacy | Consent, indicators, metadata, and recovery tests pass | IN PROGRESS: attended turn-based chat only |
| M8 | Tunnels, VPN, address books, aliases, discovery | Explicit authorization and route isolation are verified | IN PROGRESS: local contacts and alias only |
| M9 | Management, RBAC, SSO, REST API, audit | Organization policy and audit tests pass | NOT STARTED |
| M10 | Installers, MSI, Group Policy, custom clients, updates | Signed installation/update/recovery matrix passes | NOT STARTED |
| M11 | macOS/Linux | Platform-specific capability matrix is documented and tested | NOT STARTED |
| M12 | Android/iOS/iPadOS/ChromeOS-compatible client | Mobile OS restrictions are respected and tested | NOT STARTED |
| M13 | Performance, security hardening, certification | Release gate in product brief is fully evidenced | NOT STARTED |

Current work includes a native Windows app with the supplied logo, real direct
attended chat/files and scoped security tests. This is not M1/M2 completion and
does not satisfy M3 remote desktop or any production release gate.
