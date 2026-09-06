# Public Benchmark Research Notes

Research date: 2026-09-06

This document records publicly documented behavioral capabilities used to
expand the internal parity matrix. It is not an implementation dependency and
does not copy any private protocol or code.

## Findings

- Unattended access is explicitly enabled by a password and supports sessions
  when nobody is present, including locked or logged-out systems. The public
  documentation also describes saved login behavior and 2FA prompts.
- File-only sessions are distinct from graphical remote-control sessions.
- Session recording can be configured for incoming, outgoing, or both
  directions and is stored locally.
- Wake-on-LAN may use an online endpoint on the target LAN to send the magic
  packet, then reconnect automatically after wake-up.
- TCP tunneling includes forward and reverse directions.
- Permission profiles cover clipboard, printing, Privacy Mode, and tunnels.
- The public capability overview includes a virtual-printer path for remote
  printing and automatic reconnect/restart workflows.

## Sources

- [Remote access and control overview](https://support.anydesk.com/access-and-control)
- [Unattended Access](https://support.anydesk.com/docs/unattended-access)
- [File Manager and File Transfer](https://support.anydesk.com/docs/file-manager-and-file-transfer)
- [Session recording](https://support.anydesk.com/docs/session-recording)
- [Wake-on-LAN](https://support.anydesk.com/docs/wake-on-lan)
- [TCP tunneling](https://support.anydesk.com/docs/tcp-tunneling)
- [Permission profiles](https://support.anydesk.com/docs/setting-up-permission-profiles)
- [Windows command-line interface](https://support.anydesk.com/command-line-interface-for-windows)

## Engineering rule

Research changes the list of capabilities to evaluate, not the implementation
status. Each corresponding row in `PARITY_MATRIX.md` remains `NOT STARTED`,
`IN PROGRESS`, or a tested status until SENSOR code and evidence exist.

