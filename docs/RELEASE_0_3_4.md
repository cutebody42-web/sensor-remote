# SENSOR 0.3.4 — nonblocking desktop transport

Includes the [0.3.3 ID-only/security fixes](RELEASE_0_3_3.md).

The installed 0.3.3 GUI reached a separate cloud Windows host by ID on
2026-09-08 (run 34230426363), proving the former handshake timeout was fixed.
However its RTT grew to 30 seconds, making input unacceptable; the two-computer
input gate failed and is not counted as a pass.

Full screen is now a clearly labelled button (Ctrl+Alt+F). The UI reports
measured decoder fps separately from the encoder's target. Video profiles are
Balanced (up to 720p/30 fps), Sharp text (up to 1080p/15 fps), and Smooth (up to
720p/60 fps). Profile switching requires 0.3.4 on both PCs, releases held input
and creates a new authenticated display generation. Profiles never upscale
the source display. The existing Internet video budget remains 1.5 Mbps;
high-motion 1080p60 quality is not promised on the current 2 Mbps relay.

0.3.4 replaces serial blocking TCP/WebSocket bridge polling with nonblocking
duplex I/O. Pending writes are bounded, partial writes preserved, and stalled
writes time out. Regression tests cover a 64-roundtrip latency deadline and
4 MiB bidirectional payloads with delayed readers.

The active remote screen now owns the main working area, with compact controls
and the safety disconnect always visible. Disconnecting an outbound session
does not disable the independent incoming listener. A fresh two-computer native
GUI input gate remains required to verify Internet latency and native input.

No production reliability percentage, login/UAC support or Authenticode signing
is claimed. Both PCs must run SENSOR while awake, signed in and unlocked, with
explicit attended approval or an owner-configured verified unattended grant.
