# Known limitations — 0.2.0

This is a functioning native Windows development app, **not a complete remote
desktop product and not production ready**.

- No screen capture, remote video, mouse/keyboard injection, multi-monitor,
  UAC, secure desktop, login-screen support, or Ctrl+Alt+Del.
- No unattended access, Windows service, tray/background persistence,
  reconnect after reboot, audio, remote clipboard, recording or privacy mode.
- No virtual printer, physical printing pipeline, Auto Print, VPN, tunnels,
  Wake-on-LAN, discovery or whiteboard.
- GUI and CLI require an IP:port and an out-of-band verified public key.
  A nine-digit ID is random and persistent locally, not globally registered:
  there is no rendezvous/alias service or global collision resolution.
- The GUI and CLI use direct TCP only. The pair relay is library-only;
  no route selection, direct-first fallback, TLS metadata privacy, QUIC,
  ICE/STUN/TURN, NAT traversal, proxy or regional failover is integrated.
- A profile permits one active GUI job. Incoming mode is chat or file transfer;
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
- No publisher signature, MSI/installer, update mechanism or deployment server.
  No third-party policy or firewall rule was installed.
