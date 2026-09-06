# Relay — implemented library, not deployed service

`sensor-relay` authenticates one explicitly provisioned pair using pinned
Ed25519 keys. A server-signed fresh challenge is verified by each endpoint;
the endpoint signs the challenge plus its own identity. Unknown keys, wrong
relay pins and duplicate pair roles are rejected.

After both endpoints authenticate, the relay joins their streams. The normal
endpoint-to-endpoint pinned handshake and encrypted record protocol then run
unchanged through that stream. The relay does not decrypt app records.

Pairing has a deadline, authentication is bounded, rejection count is bounded,
copy buffers are fixed-size and backpressure uses blocking socket I/O.
Both directions have an idle timeout. Clean EOF half-closes the opposite writer
without discarding the other direction; I/O failures abort the pair.

Tests run a genuine TCP/endpoint-encrypted round trip through this relay and
reject an incorrect relay pin. Forwarded byte counts are asserted.

This is **not integrated into SENSOR-Remote or SENSOR-CLI** and is not an
independently deployable service. It has no concurrent rooms, TLS metadata
protection, regional routing, persistent provisioning, metrics, production
quotas or automatic direct-first selection. Internet exposure is not advised.
