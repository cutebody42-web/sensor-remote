# Test Plan

## Current automated coverage

- Device ID and alias invariants
- Identity persistence format and public/private-key consistency
- Handshake signature and nonce validation
- Transcript-bound directional key agreement
- Authenticated encryption, tamper detection, and replay rejection
- Incremental bounded frame decoding

Baseline result on 2026-09-06: 12 unit tests passed, all doc-test targets
passed, formatting passed, and strict Clippy completed with warnings denied.

## Required future coverage

Unit, integration, end-to-end, fuzz, security, installer, codec, file
integrity, reconnect, and two-machine tests are required. The Windows VM
matrix must include Windows 10, Windows 11, and supported Windows Server
editions. Scenarios must cover LAN, NAT, relay, loss from 0.1% through 10%,
RTT from 20 ms through 500 ms, slow bandwidth, multi-monitor, UAC, lock screen,
reboot/reconnect, and multi-hour stability.

No release claim is valid without evidence attached to the relevant parity row.
