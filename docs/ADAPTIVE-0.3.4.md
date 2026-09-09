# 0.3.4 adaptive engineering build — acceptance still in progress

Install the **same adaptive build on both endpoints**. New encrypted telemetry
and frame-ack messages are not understood by the previous fixed-profile build;
the version remains 0.3.4 by owner instruction. Do not infer compatibility merely
from an identical displayed version.

## Implemented control and bounds

- Auto is default; Quality favors resolution, Performance favors 60fps at the
  intermediate tier, Balanced/Auto use intermediate resolution before 1080p.
- Starts at 1.8Mbps; host ceiling `SENSOR_VIDEO_MAX_BITRATE` defaults to 16Mbps
  and is clamped to 0.3–50Mbps. Relay `RELAY_MAX_BITRATE` defaults to 20Mbps
  **per direction**, clamped to 0.3–50Mbps. Deployed environment overrides win.
- Every authenticated frame is acknowledged after decoding. Host tracks exact
  current-generation submissions (including asynchronous MFT input), at most
  16 in flight, 250ms of bitrate-budgeted bytes (64KiB–2MiB), and a 500ms oldest
  submission threshold. A full window skips capture before encoding; it does
  not discard reference P-frames. Missing acknowledgements never raise rate.
- Once per second: measured decode/preparation cost limits FPS; delivery delay
  inflation, write stalls and stale queues reduce bitrate by 35%. Three healthy
  windows permit 20% growth. Rate changes use ICodecAPI or a fresh encoder
  generation when unsupported. MFT runtime failure falls back from hardware to
  software once, never disables authentication or native consent checks.
- Tier starting budgets are **tuning estimates**, not measured image-quality
  guarantees: 360p15/0.3M, 540p24/0.8M, 720p30/1.6M, 900p30 or720p60/3M,
  1080p30/5M, 1080p60/8M+. Runtime feedback can step down at any tier.
- Input injection remains on an independent reader. Relay rate pacing is
  nonblocking and independent by direction; video waits do not suspend input.
  Encrypted video fragments are 16KiB. Desktop writer records have a 250ms
  deadline; a packet loop aborts after 500ms plus at most one record. A missing
  video acknowledgement after 5 seconds closes the stream. These are safety
  bounds, not promises of a physical-network RTT.
- Fullscreen button / Ctrl+Alt+F / Escape; visible disconnect and actual-size
  view. UI shows source/encoded dimensions, target FPS, measured encoder/decode
  and distinct UI-submitted frames/sec, payload Mbps, RTT, pending bytes,
  pre-encode skipped opportunities, hardware/software encoder. UI-submitted
  frames are not a monitor-scanout measurement. TCP packet loss is unavailable.

## 1080p correctness and pipeline measurements

Real Media Foundation testing reproduced the old 1080 failure: decoded NV12
storage is 1920x1088, with a valid 1920x1080 display aperture. Decoder now checks
the declared aperture and uses storage height for the UV plane offset; only
padding is cropped. Unexpected dimensions still fail closed. Unit tests cover
the actual software 1080 codec round trip and bottom-row color correctness.

AVX2 NV12-to-RGBA conversion is runtime-selected on x64 with exact scalar
equivalence tests; other CPUs keep the scalar path. The initial serial local
synthetic codec benchmark before this optimization measured software 32.83fps
and Intel Quick Sync 38.22fps. After AVX2, one run measured Quick Sync 46.00fps
with 9.53ms decode/conversion time (previously15.48ms). These are **synthetic
codec-pipeline** results with host and decoder in one serial loop, not remotely
displayed FPS; concurrent machine load is not controlled.

`cargo run -p sensor-desktop --release --example media_bench -- --synthetic`
exercises actual codecs on synthetic frames. Omit `--synthetic` for real desktop
capture; add `--software` for forced software. It prints cumulative CPU and
per-stage timings. No desktop pixels are written to disk. GPU utilization and
physical presentation are explicitly not measured by this console benchmark.

## Acceptance status

On September 9, real DXGI capture became available on the owner's unlocked
Windows 11 laptop. The actual 1920x1080 serial capture/encode/decode benchmark
measured 9.20fps, 2.043Mbps and 9.25% process CPU across 15.006 seconds. Capture,
preparation, encode and decode averaged 19.729/21.341/16.280/44.434ms per frame.
This combines both endpoints serially under uncontrolled machine load, not
remote presentation or proof of a hardware ceiling; 1080p60 is NOT verified.

[Native two-computer run 34319516493](https://github.com/cutebody42-web/sensor-remote/actions/runs/34319516493)
passed with real mouse click, Unicode, End key, wheel and clean close. It received
291 changing frames from a 1024x768 cloud desktop, maximum sampled RTT274.918ms
and close223ms. This used the native worker viewer, not the installed GUI.
[Windows and Ubuntu CI at 0ef6e9e](https://github.com/cutebody42-web/sensor-remote/actions/runs/34320250004)
passed. FPS recovery now requires five healthy windows and quantized tiers to
avoid recreating the encoder for small CPU timing variations.

The installed GUI then connected to a separate cloud PC in run34347519018.
Actual video, mouse click, character-key entry and wheel were observed, with
roughly12-23 sampled UI fps and239-320ms sampled RTT. The full gate FAILED:
bulk text was not delivered and the host's bounded deadline expired before a
verified clean disconnect. No timeout or fixture acknowledgement was weakened.
The GUI follow-up handles explicit egui Paste as bounded Unicode keyboard
input, without reading/synchronizing clipboard contents or also forwarding
Ctrl+V. It keeps statistics in a fixed-height row so changing digits do not
move the remote click target. Disconnect now requests authenticated graceful
close with a two-second fallback abort; global Stop remains immediate.
These follow-up changes still require a fresh installed-GUI gate.
Windows 7 is **unsupported**, with a separate capability-probe architecture and
remaining blockers in [legacy audit](../legacy/README.md). GitHub Windows Server
runners do not prove Windows10 compatibility. No Win7/Win10 VM is available in
the currently discovered local tools.

Unattended access remains limited to an explicit verified-peer grant in the
ordinary unlocked, signed-in desktop. No pre-login/UAC/reboot-service support is
claimed. Signed release manifests are not Windows Authenticode certificates.
