# Gap Analysis

## Blocking gaps before an end-to-end remote session

1. A real transport abstraction and network I/O are missing.
2. Rendezvous, NAT traversal, direct-path selection, relay fallback, and proxy
   support are missing.
3. The identity store needs a Windows DPAPI/credential-isolation provider and
   an explicit migration/backup policy.
4. Endpoint authorization, permission profiles, attended consent, and session
   lifecycle policy are missing.
5. Windows capture, hardware encoding, GPU rendering, and input injection are
   missing.

## Product gaps

File transfer, clipboard, audio, printing, Auto Print, recording, chat,
whiteboard, Wake-on-LAN, privacy mode, tunnels, VPN, address books, discovery,
history, management, RBAC, SSO, API, installers, updates, and cross-platform
clients are all not started.

## Quality and release gaps

The local Rust format, unit/doc-test, and strict lint suites pass. The checked-in
CI workflow has not yet run on a hosted runner. There are no two-machine,
VM-matrix, fuzz, long-duration, installer, performance, or security-review
results. These are release blockers, not optional follow-up work.
