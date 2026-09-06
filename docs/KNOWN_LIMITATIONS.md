# Known Limitations

- This repository does not yet contain a client UI or a remote desktop data
  path.
- No network sockets, NAT traversal, relay, proxy, or reconnect implementation
  exists.
- The identity persistence API requires a platform `KeyProtector`; the Windows
  DPAPI implementation is pending.
- The ordered replay guard is suitable for reliable ordered records only. A
  future datagram path needs a bounded replay window.
- No production security review has occurred.
- The checked-in GitHub Actions workflow has not yet run on a hosted runner;
  local format, test, and strict lint verification is green.
- The official SENSOR transparent logo asset was not present in the supplied
  workspace.
