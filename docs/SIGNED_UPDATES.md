# Publisher-signed updates

In Device & diagnostics, select **Verify and stage signed update**, then the
release's signed JSON manifest and matching installer EXE. Verification runs
off the UI thread. The app checks a compiled Ed25519 publisher public key,
product/platform/schema, newer three-part version, 90-day maximum validity,
256 MiB maximum size and SHA-256 of the staged copy. No unsigned fallback,
manifest-supplied command, automatic download or automatic execution exists.

After success, close SENSOR and run the displayed staged installer. The
original source and installed application are not modified during verification.
The current Windows user's filesystem is the staging trust boundary; compromise
of that account is outside this model. No transactional install rollback exists.

Publisher key v1 (public):

`f3ffe88aeb043e5a18899339ca64fe685d5a277cf6d7da240bc4587085958032`

The release-operator tool is `cargo run -p sensor-desktop --release --example
sign_release -- ...`. Its `create-key`, `sign` and `verify-stage` subcommands
are intentionally excluded from the application package. The private signing
identity is user-DPAPI encrypted, outside the repository, at the publisher's
`%LOCALAPPDATA%\SENSOR Technology\Release Signing\publisher-v1.bin`.
It is never a GitHub secret/artifact or part of an installer. Losing that Windows
profile loses signing capability; offline backup/recovery and rotation remain
release-engineering requirements. Do not replace the compiled key silently.

This signature authenticates SENSOR's release bytes. It is NOT Windows
Authenticode or proof of publisher identity issued by a certificate authority.
The current installer will still show an unknown/unverified publisher in Windows.
No certificate is installed, and SmartScreen/UAC must not be disabled or bypassed.
