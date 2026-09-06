# Gap analysis — 0.3.0

## Present

Native Windows window with supplied logo; DPAPI-backed persistent identity;
explicitly pinned real TCP sessions and key confirmation; visible consent;
mode-scoped permissions; attended chat and root-confined, checksummed file
transfer; explicit reconnect/resume; local contacts/alias; signed incoming audit;
attended DXGI/H.264 remote desktop with native viewport and SendInput path.
A separately tested pair relay library exists but is not selected by the app.

## Remaining gaps to reach the requested product

1. Unlocked two-device, GPU/monitor/UIPI and multi-hour validation of the
   remote desktop path; H.265/AV1, audio, clipboard, recording and adaptation.
2. Rendezvous/ID lookup, global ID registration, NAT traversal, direct-first
   relay fallback, proxies and reconnect/failover.
3. Windows service, secure IPC, explicit unattended credentials/MFA, reboot,
   login, secure desktop/UAC and Ctrl+Alt+Del.
4. Audio, clipboard, physical remote printing/Auto Print, recording, privacy,
   discovery, VPN/tunnels and the enterprise platform.
5. Signed deployment and updates, release security review, VM/physical-device
   matrix, adverse networks and multi-hour stability evidence.

The attended remote-desktop slice is now implemented, but the complete product
is not complete. See the parity matrix for scoped evidence and remaining gaps.
