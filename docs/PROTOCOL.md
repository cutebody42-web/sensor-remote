# Protocol — wire version 2

Version 2 is intentionally incompatible with the earlier foundation envelope.

## Framing and limits

Each TCP message starts with a four-byte big-endian length. Its frame is:
two-byte big-endian version (=2), one-byte type, four-byte big-endian payload
length, then Postcard payload bytes. The length must match exactly.

Maximum total frame is 1 MiB. Handshake frames are limited to 4096 bytes.
Network lengths are checked before allocating. Schema decoding rejects trailing
bytes. Reads/writes use an overall per-record deadline, not a new unlimited
timeout after each fragment. Unknown versions/types are rejected.

## Authentication

1. Each endpoint is configured with the expected peer's ID and full Ed25519
   public key, verified outside this connection.
2. Client signs its version, ID, target ID/key, identity key, ephemeral X25519
   key and random nonce. Responder validates its own target binding and the
   configured peer pin, then verifies the signature.
3. Server signs the client hello and its own version, ID, identity/ephemeral
   keys, echoed client nonce and fresh server nonce. Client verifies the
   configured server pin, signature and nonce.
4. Non-contributory X25519 keys are rejected. HKDF-SHA-256 binds the signed
   transcript and produces separate directional keys.
5. Both sides exchange encrypted role-specific Finished records before any
   application messages. Self-signed unknown peers never establish an app channel.

Hello signature/transcript domain labels retain their v1 labels; the signed
version field is 2. Ordered record and Finished domains explicitly use v2.

## Records and authorization

Control frames carry an eight-byte big-endian sequence and AEAD ciphertext.
The encrypted inner frame contains the actual typed application message.
ChaCha20-Poly1305 binds fixed protocol AAD and the sequence nonce. The receiver
requires the exact next sequence: duplicate, skipped and reordered records fail.
Malformed, tampered or I/O-failed connections are poisoned and closed.

A confirmed channel is authenticated, **not yet authorized**. The first operation
requests Chat, FileTransfer, ScreenView or RemoteControl. Local acceptance
grants exactly that mode.
Every operation requires its relevant permission at the receiving endpoint.
Rejection or missing UI response grants nothing.

FileOffer/Resume, Ready, Chunk/Progress, Commit/Complete, Pause and Cancel are
typed operations. 64 KiB chunks and the complete manifest are SHA-256 checked.
A transfer owner is the authenticated peer key. The desktop UI exposes
reconnect/resume; Pause/Cancel are protocol operations, not GUI buttons yet.

Remote desktop messages carry validated display metadata, a negotiated H.264
format, bounded 128 KiB compressed-frame fragments, cursor metadata, monitor
selection, ping/pong and permission-checked input. Video and input use one
authenticated full-duplex channel: the reader and writer share record state
only while sealing/opening, never while doing socket I/O. A single incomplete
compressed frame is retained and discarded on any sequence or metadata error.

This custom composition has tests but has not received an independent protocol
security review. Do not equate it with a reviewed TLS or Noise implementation.
