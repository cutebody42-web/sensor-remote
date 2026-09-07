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

The manual `cross-network.yml` workflow starts a Windows cloud sender; the
laptop runs a bounded fixture accepting only a synthetic file in a temporary
directory. The cloud sender pins the laptop fixture's public key and sends
8,388,625 deterministic bytes through public WSS; both ends verify the full
SHA-256. It does not expose the runner's
desktop, user files, shell or credentials. Record the actual workflow result
before claiming this gate passed.

The per-user NSIS install/reinstall/uninstall lifecycle passed. A synthetic
unrelated file survived; the pre-existing real user profile was hash-checked
and unchanged. Add/Remove Programs command quoting was verified. The
CycloneDX 1.6 SBOM passed its official JSON schema and all 281 graph references
were checked. Strict Windows and Ubuntu CI passed for application commit
52c79767ac944f522be9d2365782ce8c01ec12c2. Public `/health` now reports 0.3.1.

The initial cloud-fixture attempt produced an empty log and a misleading
workflow success; it is NOT transfer evidence. The workflow now runs its
endpoint in the foreground and requires a nonempty positive result plus a
zero exit code. The next attempt correctly failed when a service deployment
restarted registration. The bounded laptop fixture now retries registration
with the same temporary identity while waiting for the cloud build.

## Outstanding product work

Unattended Windows service, boot/login/UAC control, ICE/STUN/TURN and
direct-first failover, durable production infrastructure, signed automatic
updates, signing certificate, remote audio/printing, advanced file manager,
enterprise controls and extended impairment/security/stability certification
are not delivered. Display hot-plug/lock or encoder runtime failure currently
ends the session safely; automatic capture recovery is not certified.
The Railway trial is finite, not a production availability commitment.
