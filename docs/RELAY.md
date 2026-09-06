# Relay

Relay services are not implemented yet.

The planned relay forwards opaque, endpoint-encrypted bytes and authenticated
connection metadata. It must not terminate the session encryption or claim a
direct path. Region, route, and relay identity will be visible in diagnostics.

The relay design must include authentication, abuse/rate controls, bounded
buffers, backpressure, connection quotas, health checks, regional failover,
metrics, and no-content logging.

