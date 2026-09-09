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

Real desktop capture on this laptop currently returns HRESULT 0x80070005 and
the Windows UI tool cannot activate SENSOR (black capture). No protection is
bypassed; owner-visible desktop availability is required for GUI acceptance.
Cross-computer and current-head CI results are recorded separately after runs.
Windows 7 is **unsupported**, with a separate capability-probe architecture and
remaining blockers in [legacy audit](../legacy/README.md). GitHub Windows Server
runners do not prove Windows10 compatibility. No Win7/Win10 VM is available in
the currently discovered local tools.

Unattended access remains limited to an explicit verified-peer grant in the
ordinary unlocked, signed-in desktop. No pre-login/UAC/reboot-service support is
claimed. Signed release manifests are not Windows Authenticode certificates.
