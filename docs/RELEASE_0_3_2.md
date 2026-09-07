# SENSOR 0.3.2 acceptance record

This remains an unsigned attended release, not production certification.

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

## Pending execution gates

Native cloud-to-laptop video, final build/tests/CI, updated installer and
installed-app verification must be recorded after they actually complete.

## Not delivered

Unattended boot/login/UAC Windows service, signed automatic updates,
ICE/STUN/TURN/direct-first routing, durable production hosting, advanced
printing/audio/enterprise features and extended cross-device stability and
security certification remain incomplete. See KNOWN_LIMITATIONS.md.
