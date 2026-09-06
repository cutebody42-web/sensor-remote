# SENSOR Internet transport on Render Free

This directory contains the temporary `RENDER_TEST` deployment described in
the SENSOR Internet transport brief. The deployed process is a native Rust
Web Service, not a website. Render terminates HTTPS/WSS at the edge and sends
ordinary HTTP/WebSocket traffic to `sensor-rendezvous`, which listens on
`0.0.0.0:$PORT`.

## What it does

The service provides:

- `GET /health` for Render health checks and operator diagnostics;
- `GET /api/v1/lookup/<device-id>` for ephemeral online presence;
- `wss://<service>.onrender.com/ws` for signed device registration,
  token-authenticated pairing, heartbeat, and an opaque binary relay.

The existing SENSOR endpoint handshake and encrypted protocol remain in the
Windows client. The service never sees chat, file, screen, keyboard, or mouse
plaintext. It forwards the already-encrypted SENSOR byte stream only.

Presence is deliberately in memory for this temporary free deployment. A
service restart removes online entries; running clients reconnect and register
again. Device identity and the long-lived public/private key remain in each
Windows user's DPAPI-protected profile.

## Deploy from the repository

1. Push this repository to a Git provider and sign in to Render.
2. Choose **New > Blueprint**, select the repository, and deploy `render.yaml`.
3. Keep the service on the **Free** plan for the temporary test.
4. Copy the service URL, for example `https://sensor-rendezvous.onrender.com`.
5. On every Windows endpoint set `SENSOR_MODE=RENDER_TEST` and
   `SENSOR_SERVER` to that HTTPS URL before launching `SENSOR-Remote.exe`.
   `SENSOR_WS=wss://<service>.onrender.com/ws` is an accepted fallback, but
   the application derives the WSS path from `SENSOR_SERVER` automatically.
6. Verify the URL returns JSON from `/health`, launch both apps, and use the
   displayed device IDs plus verified public keys for the first connection.

The Windows app also reads `sensor-network.json` from its profile, then beside
the executable, with the shape `{"server":"https://YOUR-SERVICE.onrender.com"}`.
An explicit `SENSOR_SERVER`/`SENSOR_WS` environment setting takes precedence.
Starting the Internet listener saves the editable URL in the profile. A saved
server selects the Internet route on later double-click launches unless
`SENSOR_MODE` explicitly selects another mode. Do not ship an unverified URL.

The Windows app keeps a Render listener registered when Internet mode is
configured. Incoming access
still opens the normal visible SENSOR consent prompt. Closing the app stops
the listener; it is not an unattended Windows service.

## Free-plan expectations

Render Free can spin a service down while idle. The first HTTP or WebSocket
connection can therefore take a cold-start interval. The client retries the
Render connection with bounded backoff and sends heartbeats while waiting for
pairing. The free service is a temporary proof-of-concept and is not an
availability, SLA, or unattended-access guarantee.

The service has no database resource in this Blueprint. Adding a free database
would not make the current presence design durable and would introduce the
free-tier expiry/backup limitations; persistent directory storage belongs to a
later production service design.

## Environment variables

| Variable | Used by | Meaning |
| --- | --- | --- |
| `PORT` | Render service | Supplied by Render; the server defaults to `10000` locally. |
| `SENSOR_MODE` | Windows app / Blueprint | `RENDER_TEST` enables the temporary cloud path. |
| `SENSOR_SERVER` | Windows app | HTTPS/WSS base URL; HTTP/WS is allowed only for loopback tests. |
| `SENSOR_WS` | Windows app | Optional WSS fallback when `SENSOR_SERVER` is absent. |
| `RELAY_MAX_BITRATE` | Render service | Aggregate relay cap in bits per second; default 2000000. |
| `RELAY_MAX_FPS` | Render service health metadata | Target capture policy for a future server-enforced media profile. |
| `RELAY_MAX_RESOLUTION` | Render service health metadata | Target resolution policy for a future server-enforced media profile. |

The service cannot inspect end-to-end encrypted screen frames, so FPS and
resolution are advertised policy values. The Windows Render route independently
limits capture to 1280x720, 15 FPS, and 1.5 Mbit/s H.264. The aggregate bitrate
cap is enforced by the relay. Server environment changes do not reconfigure
the client encoder dynamically.

Registration v2 signs a server-generated one-use challenge and the connection
role. Outgoing connections do not replace the same device's listener. Tokens
are bound to their socket; unauthenticated binary traffic and empty heartbeat
tokens are rejected. Control messages are bounded to 16 KiB, relay frames to
1 MiB, queued pairing offers to one, and concurrent accepted connections to 64.
These safeguards do not replace an independent security review or production
account/rate-limit infrastructure.

## Local smoke test

```powershell
$env:SENSOR_MODE = 'RENDER_TEST'
$env:PORT = '10000'
cargo run -p sensor-rendezvous
Invoke-RestMethod http://127.0.0.1:10000/health
```

The repository's `sensor-rendezvous` integration test starts this service on a
random local port and verifies two native clients exchange opaque bytes over a
WebSocket relay. The production Windows package remains a native app; no
browser is required on either endpoint.

## Scope boundary

This Render profile is the temporary Internet transport requested for the
current POC. It does not yet provide durable accounts, a production device
directory, multi-region failover, TURN/ICE direct path, an installed
unattended Windows service, signed installers/updates, audio, clipboard,
printing, or enterprise policy management.
