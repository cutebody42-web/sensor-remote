# SENSOR 0.3.3 — ID-only connection and native input fixes

## Corrected

- A pinned-caller host now accepts an authenticated caller that knows only
  the host ID. The host still enforces its exact caller ID/key and signature;
  a supplied nonzero target key must match. Unknown callers are not authorized.
- Rejected handshakes close the retained cancellation socket immediately,
  instead of leaving the peer waiting for Windows timeout 10060.
- Remote connection failures identify the connection or identity-handshake stage.
- Rapid mouse down/up events in one GUI frame are preserved and mapped to
  native monitor coordinates. Clipped/covered presses are rejected; held
  buttons can be released outside the viewport.
- Raw wheel events are forwarded once with explicit unit conversion, instead
  of repeatedly amplifying egui's animated scroll. Local drag-to-pan is
  disabled during control; scrollbar panning remains available.
- Repeated navigation/editing key events are forwarded.
- GUI disconnect queues Close, blocks further input immediately, and gives
  the peer a bounded acknowledgement interval before terminating the socket.
- Parallel rendezvous tests use actual OS-assigned ports; the probe/rebind
  race that intermittently produced Windows address-in-use errors is removed.

## Verification

Automated release tests cover ID-only encrypted rendezvous sessions, consent
denial, wrong caller/target key and signatures, socket shutdown, rapid click
pairs, clipping, wheel conversion, and the existing security/reliability suite.
The actual command logs are retained outside the repository in the workspace
outputs directory. A fresh installed-GUI two-computer gate is required before
claiming that this build's GUI has passed end to end.

Previous separate-computer native-worker gate:
https://github.com/cutebody42-web/sensor-remote/actions/runs/34153021049/job/101848784180
(attempt 2, success, 2026-09-07). Earlier failed and cancelled runs remain visible;
they are not erased or counted as successful tests.

## Operating limits

Install SENSOR on both Windows computers. Both need Internet access, an awake,
signed-in, unlocked desktop, and SENSOR open/online. Enter the other computer's
ID, choose Control remote desktop, and approve on that computer unless its
owner has explicitly granted this verified controller unattended access.
An ID alone is not authorization. No pre-login/UAC control, asleep-device
wake-up guarantee, Authenticode certificate, or production uptime SLA is present.
