# ADR 0002: native Windows desktop client

Date: 2026-09-06. Status: accepted for development.

The product owner explicitly confirmed a Windows application, not a website,
and provided the official SENSOR JPEG logo. The app embeds that file unchanged.

Use Rust egui/eframe with wgpu Direct3D 12 for the visible Windows executable,
with native file dialogs and Windows executable resources. The console endpoint
is retained as SENSOR-CLI; the graphical binary is SENSOR-Remote. There is no
browser, WebView, HTML shell, or local HTTP server.

The [eframe native configuration](https://docs.rs/eframe/0.33.3/eframe/struct.NativeOptions.html)
supports a native viewport and wgpu renderer. An explicitly enabled native wgpu
DX12 backend is required when default features are disabled; a startup test
caught and corrected this integration issue. A regression test checks the backend.

Keep network and filesystem operations off the UI thread. Bounded message
channels carry actual status, one-shot consent/reply senders and throttled byte
progress. Socket shutdown and a cancellation flag terminate a job locally.
Do not create interactive controls for absent remote-control features.

This choice provides the native application shell; it does not implement remote
video capture/codec/GPU frame presentation. That subsystem and production OS/GPU
compatibility, accessibility and long-duration tests remain separate gates.
