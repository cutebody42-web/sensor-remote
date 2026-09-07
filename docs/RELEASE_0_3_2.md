# SENSOR 0.3.2 acceptance record

This is a development release without Windows Authenticode, not production certification.

## Verified separate-network transfer

On 2026-09-07 the GitHub Windows cloud fixture sent 8,388,625 deterministic
bytes through the public Railway WSS service to the Windows laptop. Both
machines verified SHA-256:

`485a584410e070b9d289cb2a75ee695b20860585e15736e6871364f3680e6526`

Cloud sender duration: 50,028 ms. Laptop receiver recorded five signed audit
entries. Both processes exited successfully. GitHub run:
https://github.com/cutebody42-web/sensor-remote/actions/runs/34127554857

This verifies real two-machine encrypted file transport on separate networks;
it is not a remote-desktop or input parity claim. Earlier empty-log and
interrupted attempts are not counted as passes.

## Changes

- Always-visible local Stop control and prominent ID-based view/control actions.
- Incoming contacts are checked against saved keys before showing consent.
- Duplicate contact IDs and zero-key contacts fail closed without replacing
  the existing contact store.
- Worker-level rejection of automatic acceptance for wildcard/unpinned peers,
  before opening a receiver or listening socket.
- Temporary cross-network fixture keeps its identity across waiting retries.
- Explicit native two-machine view-only fixture using a pinned viewer; a
  headless/locked Windows runner must fail, never substitute synthetic capture.
- Installer version is derived from the packaged SBOM, avoiding stale metadata.

## Real Windows video/input and reconnect tests

The final two-minute same-laptop/public-WSS input run passed with 1,191 decoded
frames, 15,113,593 encoded bytes, first frame 1,923 ms, final RTT 220.2 ms and
122 ms bounded worker stop. It verified real mouse clicks, Unicode typing,
wheel input, monitor reconfiguration and stale-generation input rejection.
These are one run's measurements, not advertised performance guarantees.

Monitor reconfiguration originally failed with DXGI 0x80070057. Releasing the
old output duplication before creating its replacement fixed the live test.
A separate run with no focused QA window timed out and is not counted as a pass.

Cloud video run 34128582892 attempt 2 decoded 82 real frames on the laptop,
but its host reported a close failure. It is NOT counted as a full successful
two-computer session. Dropping the reader before the close acknowledgement
was identified and fixed. A strengthened local public-WSS close test passed
with successful results from both workers (226 frames, 236 ms close).

Three actual public-WSS reconnect cycles reloaded the same DPAPI identity and
persisted unattended grant, automatically authorized only the pinned peer,
received real video and closed cleanly. Revocation persisted successfully.
This restarted host workers, NOT Windows; OS reboot remains untested.

Unattended grants exclude clipboard, files and chat; expire after 30 days;
reject mismatched keys/IDs; and are bound by DPAPI to the current user and
local identity. Tamper, cross-profile, expiry and revocation tests passed.

Publisher-signed update policy rejects wrong keys, changed payload/signature
bytes, wrong platform/product/schema, same/older versions, expiry and size/hash
mismatch. Failed staging leaves no executable; the selected source is untouched.

## Pending execution gates

Native cloud-to-laptop video, final build/tests/CI, updated installer and
installed-app verification must be recorded after they actually complete.

## Not delivered

Unattended pre-login/UAC Windows service, OS reboot certification, automatic update delivery,
ICE/STUN/TURN/direct-first routing, durable production hosting, advanced
printing/audio/enterprise features and extended cross-device stability and
security certification remain incomplete. See KNOWN_LIMITATIONS.md.
