# Current Feature Inventory

Status values are deliberately conservative: `TESTED` means the behavior has
automated tests in this repository; it does not mean production readiness.

| Capability | Current implementation | Status |
| --- | --- | --- |
| Stable nine-digit device ID type | `sensor-core::DeviceId` validation, generation, and display formatting | TESTED |
| Alias validation | `sensor-core::DeviceAlias` rejects empty, whitespace, control characters, and oversized values | TESTED |
| Long-term device identity | Ed25519 keypair wrapper | TESTED |
| Identity persistence format | Versioned serialized record with protected seed boundary and public-key consistency check | TESTED |
| OS-protected identity storage | `KeyProtector` trait only; Windows DPAPI adapter is not implemented | NOT STARTED |
| Mutual-authentication handshake | Signed client/server hello messages with nonce binding and X25519 ephemeral exchange | TESTED |
| Forward-secret session key derivation | HKDF-SHA-256 with transcript binding and directional keys | TESTED |
| Authenticated session records | ChaCha20-Poly1305 with sequence-bound nonce and ordered replay rejection | TESTED |
| Bounded protocol framing | Versioned postcard frame plus incremental length-delimited decoder | TESTED |
| Internet transport | No socket, QUIC, TCP/TLS, NAT traversal, or relay implementation | NOT STARTED |
| Windows capture/input/rendering | No platform implementation exists | NOT STARTED |
| UI, installer, service, printing, audio, files, management | No implementation exists | NOT STARTED |

