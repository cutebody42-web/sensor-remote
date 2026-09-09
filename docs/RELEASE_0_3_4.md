# SENSOR 0.3.4 — adaptive engineering build

Includes the [0.3.3 ID-only/security fixes](RELEASE_0_3_3.md).

The installed 0.3.3 GUI reached a separate cloud Windows host by ID on
2026-09-08 (run 34230426363), proving the former handshake timeout was fixed.
However its RTT grew to 30 seconds, making input unacceptable; the two-computer
input gate failed and is not counted as a pass.

The original fixed-profile 0.3.4 build has been superseded by the adaptive
engineering build on the same release line. Install matching builds on both
PCs: identical 0.3.4 version labels do not make the older wire protocol compatible.
See [adaptive architecture and measured evidence](ADAPTIVE-0.3.4.md) for current
limits, source/encoded/display measurements, fullscreen controls, native MFT
fallback and 1080 aperture correction. Actual 1080p60 remote presentation is
not yet certified; target FPS is never presented as measured FPS.

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
