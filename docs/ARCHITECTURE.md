# Architecture

## Trust boundaries

1. **Endpoint**: the customer device and its local consent/permission policy.
2. **Session transport**: direct or relayed encrypted bytes; relays must not
   receive plaintext session content.
3. **Control plane**: rendezvous, authentication, management, policy, and
   audit metadata.
4. **Operator client**: UI, renderer, input, files, and local printer access.

The control plane may coordinate a connection, but endpoint authorization and
session permissions must be enforced at the endpoint as well.

## Planned workspace

```text
sensor-core          domain types and invariants
sensor-identity      device identity and OS-protected persistence
sensor-crypto        composed cryptographic primitives
sensor-protocol      versioned messages and capability negotiation
sensor-transport     QUIC/TCP/proxy-independent framing and state
sensor-rendezvous    registration and direct-path coordination
sensor-relay         encrypted byte forwarding only
sensor-media         capture, codecs, audio, recording
sensor-files         resumable, integrity-checked transfer
sensor-print         virtual printer and Auto Print
sensor-input         policy-aware input messages
sensor-session       consent, permissions, lifecycle, telemetry
sensor-platform      cross-platform traits
sensor-windows       Windows capture/service/secure-desktop backends
sensor-service       privileged Windows service boundary
sensor-client        desktop/mobile client shell
sensor-management-api REST/API and policy service
```

The first six crates are present. The remaining crates are planned boundaries,
not claims of implemented functionality.

## Connection lifecycle

```text
register -> rendezvous -> direct-path candidates -> authenticated handshake
         -> endpoint consent/ACL -> encrypted session -> telemetry/reconnect
         -> explicit close -> audit record
```

Relay selection is a transport decision and must be exposed in connection
diagnostics. A relay cannot be described as a direct connection.

