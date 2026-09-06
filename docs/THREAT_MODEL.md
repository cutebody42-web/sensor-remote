# Threat Model

## Assets

- Device identity and private keys
- Consent and permission decisions
- Remote screen/input/session data
- Files, clipboard, audio, print jobs, and recordings
- Organization policy and audit records

## Adversaries

- Internet attacker observing or modifying traffic
- Malicious or compromised relay/control-plane operator
- Attacker guessing device IDs or unattended credentials
- Malicious local user attempting to hide or escalate a session
- Compromised endpoint or operator device
- Supply-chain attacker targeting installers or updates

## Required controls

| Threat | Control |
| --- | --- |
| MITM during rendezvous | Mutual identity authentication, transcript binding, fingerprint display/policy |
| Replay | Fresh nonces, ephemeral keys, authenticated sequence numbers |
| Relay compromise | Relay sees routing metadata only; payload is endpoint-encrypted |
| Credential guessing | Rate limits, lockout/backoff, 2FA, audit events |
| Unauthorized attended access | Visible request, explicit accept/reject, endpoint permission profile |
| Unauthorized unattended access | Explicit enablement, OS-protected secrets, ACL, 2FA, revocation |
| Privilege escalation | Separate installed service, authenticated IPC, narrow privileged operations |
| Malicious update | Signed manifest/package verification, rollback and staged rollout |
| Data leakage | Redacted diagnostics, local recording controls, retention policy |

The model must be revisited before M2 networking, M4 service work, M6
printing, M9 management, and M10 update/deployment work.

