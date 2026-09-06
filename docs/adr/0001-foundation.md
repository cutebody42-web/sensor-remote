# ADR-0001: Rust workspace and security-first vertical slice

Date: 2026-09-06
Status: Accepted

## Context

The starting workspace had no existing implementation. The product requires a
high-performance Windows client, platform-specific privileged features, a
cross-platform core, and security claims that must be testable. A broad UI
without a real protocol would create false product signals.

## Decision

Use a Rust workspace with strict crate boundaries. Establish identity,
protocol, handshake, key derivation, authenticated records, and bounded
framing before implementing desktop capture or user-interface flows.

Use:

- Ed25519 for long-term device authentication.
- X25519 ephemeral keys for forward-secret session establishment.
- HKDF-SHA-256 for directional session keys.
- ChaCha20-Poly1305 for the initial authenticated record layer.
- Postcard for a bounded, versioned internal wire representation.

The host application must inject platform key protection. No fallback that
writes private identity material in plaintext is permitted.

## Alternatives considered

- A JavaScript-only client: rejected for the capture, input, service, and
  latency-sensitive requirements.
- A single large crate: rejected because it would blur trust boundaries and
  make platform-specific privilege hard to review.
- Building the UI first: rejected because it encourages fake states and dead
  controls, explicitly prohibited by the product brief.

## Consequences

The initial repository is useful and testable, but it is not yet a remote
desktop product. Each later milestone must add a working vertical capability,
tests, and documentation before its matrix status changes.

