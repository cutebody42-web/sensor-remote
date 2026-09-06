# Test plan

## Current automated coverage

[VERIFICATION.md](VERIFICATION.md) records the executed final 48-test release suite:
validated IDs/aliases; actual DPAPI and atomic identity; pinned authenticated
TCP and mutual key confirmation; malformed framing and ordered AEAD records;
endpoint permission enforcement; accepted/rejected chat/files; checksummed,
root-confined resume with a single-writer lock; signed audit; relay bytes;
desktop worker cancellation, contacts, native backend and original logo;
compiled CLI subprocesses.

The bounded deterministic parser mutation corpus is a regression suite,
not a sustained libFuzzer campaign. Real loopback tests are not a two-PC test.

## Native Windows acceptance still required

With the desktop unlocked: inspect the original logo at normal/high DPI,
resize/minimize/reopen, keyboard/screen-reader navigation, file picker,
peer verification, Accept/Reject expiration, local disconnect and window close.
Use two independently profiled app instances and then two physical PCs.
Confirm the GUI never reports online/active after worker/socket failure.
The current visual inspection was paused at the lock screen.

## Full release gates still required

Windows 10, Windows 11 and supported Server VMs; GPU/driver coverage; LAN/NAT/
Internet/relay; 0.1/1/5/10 percent packet loss; 20/50/100/200/500 ms RTT;
128 kbps through 100+ Mbps; codec and multi-monitor tests; UAC/login/lock/reboot;
unattended MFA; printing/Auto Print; service/installer/uninstaller; signed
update/rollback; independent security review; parser fuzzing; 1/8/24 hour
stability and resource measurements.

Those subsystems must first exist. No release claim is valid without evidence
on every applicable product-brief gate and parity row.
