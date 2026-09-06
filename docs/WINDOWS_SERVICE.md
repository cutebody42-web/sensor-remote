# Windows Service

The Windows service is not implemented yet.

The planned `SENSOR Remote Service` will be installed explicitly, start before
interactive login when configured, and expose a narrow authenticated IPC
surface to the desktop client. It will own only the privileged operations that
Windows requires, including approved login-screen/UAC workflows, service
startup, and the virtual printer. It will not provide hidden or covert access.

