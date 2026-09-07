# Known limitations — 0.3.2

This is a functioning native Windows attended remote-desktop development app,
**not a complete AnyDesk-equivalent product and not production ready**.

- Screen capture, H.264 remote video, attended mouse/keyboard/text injection,
  monitor enumeration and selection are implemented for the ordinary unlocked
  interactive desktop. The real capture/encode/decode/input path passed over
  public WSS with both endpoints on this laptop. Cross-computer interactive
  desktop and long-running certification remain. H.265/AV1, UAC, secure desktop, login-screen
  support and Ctrl+Alt+Del are not implemented.
- Unattended view/control now requires an explicit saved 30-day verified-key
  grant, an open SENSOR GUI, and an unlocked signed-in desktop. It is not a
  Windows service. Optional sign-in startup reopens the app after login;
  operating-system reboot, pre-login access and UAC are not verified or supported.
  No tray/background persistence, password/MFA login, audio, image/file clipboard,
  recording or privacy mode. Text clipboard is explicitly permission-gated
  and bounded; initial clipboard contents are not sent. A visible direct-TCP receiver can explicitly auto-accept one
  already pinned peer while the GUI remains open.
- No virtual printer, physical printing pipeline, Auto Print, VPN, tunnels,
  Wake-on-LAN, discovery or whiteboard.
- Railway `RENDER_TEST` provides temporary Internet registration, online lookup,
  heartbeat and opaque WSS relay. Presence is in memory and disappears on a
  service restart; the Railway trial has a finite allowance. This is not durable account
  authentication, global collision resolution, an SLA, or unattended access.
- The GUI supports direct TCP, the Internet WSS relay, or a self-hosted,
  provisioned single-pair relay. The CLI remains direct-TCP oriented. There is
  no automatic direct-first fallback, TLS metadata privacy, QUIC, ICE/STUN/
  TURN, NAT traversal, proxy or regional failover. The rendezvous service does not terminate
  SENSOR endpoint encryption and currently cannot enforce FPS/resolution
  inside encrypted media frames.
- A profile permits one active GUI job. Incoming mode is chat, file transfer,
  view-only desktop or desktop control;
  no mixed-operation, multi-session or dual-pane file manager.
- Chat is turn-based with a 120-second reply/idle deadline and 200-message
  in-memory display cap. Chat history is not persisted or uploaded.
- Maximum received file offer is 1 GiB. Filename is flat, not a remote path.
  Existing files are never overwritten. Partial files are retained on an
  interrupted transfer; reconnect, re-accept and supply its transfer ID.
  No disk quota/retention UI, folder sync, directory transfer or parallel queue.
  Atomic no-overwrite publication requires a filesystem supporting hard links
  (tested on the local Windows volume); unsupported filesystems fail explicitly.
- DPAPI is tied to the current Windows user. Cross-user migration, backup,
  service-account recovery and reboot testing have not been implemented.
- Signed audit records are local and currently cover incoming session/file
  operations. An external trusted chain-head checkpoint is needed to detect
  deletion of the final records or whole log. No central retention service.
- Local contacts and public metadata rely on the current user's filesystem
  security. A compromised endpoint/current-user account is outside protection.
- No production security review, extended fuzz campaign, network impairment
  matrix, Windows 10/11/Server certification or 8+ hour stability evidence.
- Publisher-signed offline update manifests and byte-verified staging are
  implemented. No Windows Authenticode certificate, MSI, automatic update feed,
  atomic installer rollback, publisher-key rotation/recovery or update server.
  The NSIS per-user installer remains Authenticode-unsigned; it is not a Windows service.
  No third-party policy or firewall rule was installed.
- Current executable needs Direct3D 12 and Media Foundation. The Windows C
  runtime is statically linked. Packaging records a CycloneDX SBOM,
  dependencies and available license files;
  some registry crates omit license text, which the inventory flags with zero
  collected files. Distribution/license review is not complete.
