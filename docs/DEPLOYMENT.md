# Deployment

The current deliverable is an **unsigned portable Windows development package**
containing the native desktop, CLI, and provisioned relay executables. It is
not an installer, MSI, Windows service or production release.

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

Internet ID/rendezvous, NAT traversal, server containers, regional deployments, organization authentication,
installer/uninstaller, MSI, ADMX, signed updates/rollback and publication
infrastructure remain unimplemented. No production signing key was supplied.
