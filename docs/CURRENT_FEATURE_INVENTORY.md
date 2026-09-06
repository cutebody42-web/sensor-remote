# Current feature inventory — 0.3.0

The [parity matrix](PARITY_MATRIX.md) is the detailed live inventory.
TESTED means a scoped automated test exists, not product certification.

Implemented: native Windows UI with original logo, DPAPI identity, direct
encrypted pinned TCP, attended consent, operation permissions, file
send/receive/resume and integrity, turn-based chat, local contacts/alias,
incoming signed audit, and the attended remote desktop path: DXGI capture,
Media Foundation H.264, encrypted bounded frame fragments, native viewport,
monitor selection and permission-checked SendInput. The authenticated pair
relay remains a library-only component.

No web site was built. Unattended service/login/UAC, printing, H.265/AV1,
audio, clipboard, enterprise service, NAT traversal, installer or updater is
implemented. The temporary Render Internet path adds signed device
registration, ephemeral presence, heartbeat/retry, and an opaque WSS relay;
its native service is tested with a two-client integration test. The remote
path still needs unlocked two-device and long-run verification; code paths
without tests are not counted as certified behavior.
