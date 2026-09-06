# Protocol

Protocol version 1 currently defines a bounded postcard envelope and a
mutually authenticated hello exchange.

## Handshake

1. Initiator sends its device ID, Ed25519 public key, X25519 ephemeral public
   key, nonce, and Ed25519 signature over the canonical unsigned fields.
2. Responder verifies the initiator signature, then sends its device ID,
   Ed25519 public key, X25519 ephemeral public key, the echoed initiator nonce,
   its own nonce, and a signature over both hello messages.
3. Both sides derive the same transcript and X25519 shared secret.
4. HKDF-SHA-256 derives independent initiator-to-responder and
   responder-to-initiator keys.
5. Application records are encrypted and authenticated before any remote
   control capability is enabled.

## Framing

The transport framing uses a 32-bit big-endian payload length followed by one
versioned frame. Lengths are bounded by 16 MiB and decoding is incremental.

## Compatibility

Future versions must negotiate capabilities explicitly and must not silently
interpret unknown security-sensitive fields. Protocol changes require updated
test vectors and a review of signed transcript coverage.

