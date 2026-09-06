# Security architecture — development status

## Implemented controls

- Explicit out-of-band peer ID/public-key pins; strict Ed25519 signature checks.
- Signed target binding, ephemeral X25519, transcript-bound directional HKDF
  keys, mutual encrypted Finished messages, ChaCha20-Poly1305 ordered records.
- Windows current-user DPAPI protects the identity seed and authenticates
  its public metadata as optional entropy. No plaintext fallback.
- Locked initialization, bounded identity reads and atomic same-directory save.
- Visible local consent, separate chat/file permissions, no automatic listener
  on launch, and local socket shutdown for Stop/window close.
- Exact framed lengths, bounded network allocation, per-record deadlines,
  no application messages before key confirmation.
- Capability-scoped file directory, restricted Windows names, per-chunk and
  whole-file checksums, peer-bound resume and no-clobber publication.
- Signed, chained, exclusive-writer local audit, with narrow metadata-only
  events. File content, chat content and secret keys are not audit fields.

## Trust and remaining gaps

The operator must verify the **full public key**, not merely the numeric ID.
Relay payload encryption does not conceal handshake/routing metadata.
DPAPI does not defend against code already executing as the same Windows user.
Audit tail/whole-log deletion requires externally checkpointed chain heads to
detect; the app currently displays a head but does not provide that service.

No unattended password feature, MFA, organization ACL service, SSO, privileged
Windows service, signed updater or central audit retention exists.
Current incoming-session audit is not a complete enterprise event history.

The cryptographic primitives come from established crates, but this application's
composition has not been independently reviewed. No production security claim
or audited-protocol claim is made. Do not expose this development host publicly.

## Primary implementation references

[Microsoft CryptProtectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)
and [CryptUnprotectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata)
document user scope, optional entropy and buffer ownership.
[cap-std Dir](https://docs.rs/cap-std/latest/cap_std/fs/struct.Dir.html)
documents directory-scoped filesystem operations.

Production needs independent review, fuzz/DoS campaigns, OS-account ACL tests,
key recovery design, artifact signing and the complete product release gate.
