# Security Architecture

## Current guarantees

- Long-term device authentication uses Ed25519 signatures.
- Session establishment uses ephemeral X25519 keys.
- HKDF-SHA-256 derives separate directional keys from the authenticated
  transcript.
- Session records use ChaCha20-Poly1305 authenticated encryption.
- Sequence numbers are bound into nonces and ordered replay is rejected.
- Protocol frames have a maximum size and incremental decoding.
- The identity file format requires a platform `KeyProtector`; it has no
  plaintext-seed fallback. Public identity metadata is supplied as associated
  context so a production protector must authenticate it with the seed.

## Not yet guaranteed

The current crate-level code does not yet provide endpoint ACLs, attended
consent, password/2FA handling, certificate/trust policy, secure update
verification, relay deployment, or Windows OS secret storage. The application
must not advertise these capabilities until their implementations and tests
exist.

## Key handling rules

- Private identity material is zeroized where the dependency supports it.
- Passwords, session secrets, private keys, clipboard content, and document
  content must never enter logs.
- Session transcript hashes and public fingerprints may be logged only under
  an explicit diagnostics policy.
- Any future password-based feature must use a memory-hard KDF and rate
  limiting; passwords must not become long-term transport keys.

## Review gates

Every new protocol field requires a signed-form decision. Every privileged
Windows action requires a threat-model update and an end-to-end authorization
test. Production release requires independent internal security review and
passing fuzz/security suites.
