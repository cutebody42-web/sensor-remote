# SENSOR 0.3.1 release evidence

Status: **unsigned attended development release, not production ready**.
Evidence below is measured on 2026-09-07, not a claim of AnyDesk parity.

## Completed checks

- `cargo test --workspace --release --locked`: all default tests passed;
  live desktop, deployed-service and OS clipboard tests require explicit opt-in.
- Real native test fixture through the public Railway WSS endpoint: DXGI
  capture, Intel Quick Sync hardware H.264, native decoding, click, Unicode
  text `SENSOR QA مرحبا 123`, wheel and release-all passed. No real user text
  or clipboard contents were logged, and no screen recording was saved.
- Optimized repeat: 1280x704 stream, 15 fps cap, first frame 1886 ms,
  120 decoded frames / 1,311,167 encoded bytes during 12,001 ms, last measured
  RTT 171.8 ms. Both workers stopped in 141 ms. These are single-run
  observations, not general performance guarantees. Desktop endpoints used
  separate identities on the same unlocked laptop through the public server.
- Actual Windows clipboard Unicode write/read, baseline/echo suppression,
  disable/re-enable and original OLE clipboard restoration test passed.
- Windows C runtime statically linked; `dumpbin /DEPENDENTS` confirms the
  release app does not depend on VCRUNTIME140.dll.
- Owner-provided JPEG embedded unchanged and checked by SHA-256.

## Important fixes

Async hardware encoder output is polled on static screens; MFT stream-change
events are handled; Internet dimensions remain macroblock-aligned. Locked
Windows sessions fail closed. Input generation and monitor coordinates are
updated together. A saturated input/control queue aborts without blocking
shutdown. A peer disconnect is distinct from a local stop, allowing the
listener to register again. Clipboard is off unless explicitly requested and
granted. Duplicate decoded-frame uploads are avoided in the native viewer.

ID-only Internet connections consult saved contact keys. Unknown first
contacts still require owner approval; ID lookup is not a substitute for
out-of-band public-key verification. Environment, profile and packaged server
configuration are resolved in order and fail closed if the selected source
is invalid.

## Separate-network and packaging gates

The manual `cross-network.yml` workflow starts a bounded Windows cloud
fixture which accepts only a synthetic file in a temporary directory. The
laptop pins its public key and sends 8,388,625 deterministic bytes through
public WSS; both ends verify the full SHA-256. It does not expose the runner's
desktop, user files, shell or credentials. Record the actual workflow result
before claiming this gate passed.

Installer lifecycle, final strict CI and final deployed server version are
release gates; generated output paths and their results must be recorded
after execution, not inferred from source code.

## Outstanding product work

Unattended Windows service, boot/login/UAC control, ICE/STUN/TURN and
direct-first failover, durable production infrastructure, signed automatic
updates, signing certificate, remote audio/printing, advanced file manager,
enterprise controls and extended impairment/security/stability certification
are not delivered. Display hot-plug/lock or encoder runtime failure currently
ends the session safely; automatic capture recovery is not certified.
The Railway trial is finite, not a production availability commitment.
