# Deployment

The current deliverable is an **unsigned portable Windows package** containing
the native desktop, CLI, and provisioned relay executables, plus a temporary
Render Free Internet transport. It is not an installer, MSI, unattended
Windows service, or production availability release.

Open SENSOR-Remote.exe. Data defaults to
`%LOCALAPPDATA%/SENSOR Technology/Remote`: protected identity, public contacts,
incoming-session audit and a Received Files directory. Optional
`--config <directory>` isolates a profile. DPAPI still belongs to the current
Windows user; copying the profile does not create cross-account portability.

The app does not change firewall settings, install startup persistence,
request elevation or install a Windows service. A receiver may explicitly
enable auto-accept for one verified pinned peer while the app remains open;
closing it ends its sessions.
Do not bypass Windows protection or deploy this publicly as trusted signed software.

## Render Free Internet transport

The repository's `render.yaml` deploys `sensor-rendezvous` as a Docker Web
Service. Render supplies the public HTTPS endpoint and terminates TLS; the
service accepts WSS on the same public port and exposes `GET /health`.

From the Render service URL, configure every Windows client with:

```powershell
$env:SENSOR_MODE = 'RENDER_TEST'
$env:SENSOR_SERVER = 'https://sensor-rendezvous.onrender.com'
Start-Process .\SENSOR-Remote.exe
```

The service keeps only ephemeral online presence in memory. Clients retain
their device ID and key in the DPAPI profile and re-register after a service
restart or Render cold start. The application retries WSS setup with bounded
backoff and heartbeat while it is waiting for a peer. Use
[deployment/render/README.md](../deployment/render/README.md) for the complete
checklist and limitations.

This is a temporary cloud relay, not direct P2P/NAT traversal. The existing
SENSOR endpoint handshake and encrypted protocol remain end-to-end; the
service forwards opaque binary frames and cannot enforce media properties that
are inside the encrypted stream. No production signing key, durable account
directory, SLA, or unattended access was supplied.
