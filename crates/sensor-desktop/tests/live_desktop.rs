//! Explicitly opted-in tests: capture the current, unlocked Windows desktop.
//! No image or clipboard contents are written to disk or logged.
#![cfg(windows)]

use sensor_desktop::worker::{self, Event, Job, RenderRoute, Route, Task};
use sensor_identity::DeviceIdentity;
use sensor_session::ExpectedPeer;
use std::time::{Duration, Instant};

struct Fixture(std::process::Child);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn pin(identity: &DeviceIdentity) -> ExpectedPeer {
    ExpectedPeer {
        device_id: identity.device_id(),
        public_key: identity.keypair().public_key(),
    }
}

fn stopped(job: &Job) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !job.is_finished() && Instant::now() < deadline {
        while job.events.try_recv().is_ok() {}
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(job.is_finished(), "desktop shutdown exceeded five seconds");
}

#[test]
#[ignore = "requires an unlocked Windows desktop and SENSOR_TEST_SERVER; captures the actual screen"]
fn public_wss_real_windows_capture_encode_decode_and_stop() {
    real_desktop(false);
}

#[test]
#[ignore = "requires unlocked desktop, public SENSOR_TEST_SERVER and built SENSOR_QA_TARGET; operates only the visible QA fixture"]
fn public_wss_actual_mouse_unicode_keyboard_wheel_and_dynamic_video() {
    real_desktop(true);
}

fn real_desktop(input_test: bool) {
    let test_seconds = std::env::var("SENSOR_TEST_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>().expect("invalid test duration"))
        .unwrap_or(12);
    assert!(
        (12..=480).contains(&test_seconds),
        "opt-in duration must be 12 to 480 seconds"
    );
    let server = std::env::var("SENSOR_TEST_SERVER").expect("explicit test endpoint required");
    assert!(server.starts_with("https://"));
    let route = Route::Render(RenderRoute { server });
    let host_dir = tempfile::tempdir().unwrap();
    let viewer_dir = tempfile::tempdir().unwrap();
    let state_path = viewer_dir.path().join("qa-state.json");
    let _fixture = input_test.then(|| {
        let exe =
            std::env::var_os("SENSOR_QA_TARGET").expect("explicit QA fixture executable required");
        Fixture(
            std::process::Command::new(exe)
                .arg(&state_path)
                .spawn()
                .unwrap(),
        )
    });
    let host = DeviceIdentity::generate();
    let viewer = DeviceIdentity::generate();
    let host_peer = pin(&host);
    let viewer_peer = pin(&viewer);
    let receiver = worker::start(
        Task::Host {
            route: route.clone(),
            receive_dir: host_dir.path().into(),
            auto_accept: false,
            accept_any: false,
        },
        host,
        viewer_peer,
        host_dir.path().into(),
    );
    let deadline = Instant::now() + Duration::from_secs(100);
    loop {
        assert!(Instant::now() < deadline, "host registration timed out");
        match receiver.events.recv_timeout(Duration::from_secs(1)) {
            Ok(Event::Online(true)) => break,
            Ok(Event::Finished(result)) => panic!("host failed: {result:?}"),
            _ => {}
        }
    }
    let started = Instant::now();
    let sender = worker::start(
        Task::Remote {
            route,
            control: input_test,
            clipboard: false,
        },
        viewer,
        host_peer,
        viewer_dir.path().into(),
    );
    let deadline = Instant::now() + Duration::from_secs(test_seconds + 90);
    let mut approved = false;
    let mut format_seen = false;
    let mut first_frame = None;
    let mut reported_at = Instant::now();
    let mut video_format = None;
    let mut input_stage = 0;
    let mut input_at = Instant::now();
    let mut first_pixels = None;
    let mut pixels_changed = false;
    loop {
        assert!(Instant::now() < deadline, "real video test timed out");
        while let Ok(event) = receiver.events.try_recv() {
            match event {
                Event::Consent(peer, mode, response) => {
                    assert_eq!(peer.public_key, viewer_peer.public_key);
                    assert_eq!(mode.controls_input(), input_test);
                    assert!(sender.control.latest_frame().is_none());
                    response.send(true).unwrap();
                    approved = true;
                }
                Event::Finished(result) => panic!("host stopped: {result:?}"),
                _ => {}
            }
        }
        while let Ok(event) = sender.events.try_recv() {
            match event {
                Event::RemoteFormat(format) => {
                    assert!(approved);
                    assert!(format.width <= 1280 && format.height <= 720);
                    println!(
                        "actual encoder={}, hardware={}, dimensions={}x{}, fps_cap={}",
                        format.encoder,
                        format.hardware,
                        format.width,
                        format.height,
                        format.fps_limit
                    );
                    format_seen = true;
                    video_format = Some(format);
                }
                Event::Finished(result) => panic!("viewer stopped: {result:?}"),
                _ => {}
            }
        }
        if let Some(frame) = sender.control.latest_frame() {
            assert!(approved && format_seen, "frame before consent/format");
            assert_eq!(
                frame.rgba.len(),
                frame.width as usize * frame.height as usize * 4
            );
            assert!(frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[..3] != [0, 0, 0]));
            first_frame.get_or_insert(started.elapsed());
            use sha2::{Digest, Sha256};
            let digest: [u8; 32] = Sha256::digest(&frame.rgba).into();
            if let Some(first) = first_pixels {
                pixels_changed |= digest != first;
            } else {
                first_pixels = Some(digest);
            }
        }
        let (frames, bytes, rtt) = sender.control.statistics();
        if input_test && first_frame.is_some() {
            if let (Some(format), Ok(state)) = (&video_format, std::fs::read(&state_path)) {
                let state: serde_json::Value = serde_json::from_slice(&state).unwrap();
                let focused = state["focused"].as_bool() == Some(true);
                let send = |event| {
                    assert!(sender
                        .control
                        .send_remote(sensor_media::DesktopMessage::Input {
                            generation: format.generation,
                            event
                        }))
                };
                use sensor_media::{Input, MouseButton};
                if input_stage == 0 && focused {
                    let x = state["text_center"][0].as_f64().unwrap() as i32 - format.display.left;
                    let y = state["text_center"][1].as_f64().unwrap() as i32 - format.display.top;
                    assert!(
                        x >= 0
                            && y >= 0
                            && x < format.display.width as i32
                            && y < format.display.height as i32
                    );
                    send(Input::Move {
                        x: x as u32,
                        y: y as u32,
                    });
                    send(Input::Button {
                        button: MouseButton::Left,
                        down: true,
                    });
                    send(Input::Button {
                        button: MouseButton::Left,
                        down: false,
                    });
                    input_stage = 1;
                    input_at = Instant::now();
                } else if input_stage == 1
                    && focused
                    && state["text_focused"].as_bool() == Some(true)
                    && input_at.elapsed() > Duration::from_millis(500)
                {
                    // Keyboard is sent only after the fixture itself confirms
                    // foreground and text-field focus. No other app is targeted.
                    send(Input::Text("SENSOR QA مرحبا 123".into()));
                    send(Input::Wheel {
                        delta: -120,
                        horizontal: false,
                    });
                    send(Input::ReleaseAll);
                    input_stage = 2;
                } else if input_stage == 2
                    && state["typed_expected"].as_bool() == Some(true)
                    && state["wheel"].as_bool() == Some(true)
                    && state["clicks"].as_u64().unwrap_or(0) > 0
                {
                    input_stage = 3;
                    println!(
                        "REAL INPUT: native QA target verified mouse click, Unicode text and wheel"
                    );
                } else if input_stage == 3 && focused {
                    assert!(sender.control.send_remote(
                        sensor_media::DesktopMessage::SelectDisplay(format.display.index)
                    ));
                    input_stage = 4;
                } else if input_stage == 4
                    && format.generation > 1
                    && focused
                    && state["text_focused"].as_bool() == Some(true)
                {
                    // This stale event must be discarded before Windows input.
                    assert!(sender
                        .control
                        .send_remote(sensor_media::DesktopMessage::Input {
                            generation: format.generation - 1,
                            event: Input::Text("STALE EVENT MUST NOT ARRIVE".into()),
                        }));
                    input_at = Instant::now();
                    input_stage = 5;
                } else if input_stage == 5 && input_at.elapsed() > Duration::from_secs(1) {
                    assert_eq!(
                        state["typed_expected"].as_bool(),
                        Some(true),
                        "stale input altered the native fixture after monitor reconfiguration"
                    );
                    input_stage = 6;
                    println!("REAL MONITOR RECONFIGURATION: new format received and stale-generation text rejected");
                }
            }
        }
        if reported_at.elapsed() > Duration::from_secs(5) {
            println!(
                "capture/encode={:?}, viewer_frames={frames}, video_bytes={bytes}, rtt_us={rtt}",
                receiver.control.capture_statistics()
            );
            reported_at = Instant::now();
        }
        if let Some(first_frame) = first_frame {
            if rtt > 0
                && started.elapsed() > Duration::from_secs(test_seconds)
                && (!input_test || (input_stage == 6 && pixels_changed && frames > 4))
            {
                println!(
                    "PUBLIC WSS REAL DESKTOP: first_frame_ms={}, frames={frames}, encoded_bytes={bytes}, rtt_ms={:.1}, duration_ms={}",
                    first_frame.as_millis(), rtt as f64 / 1000.0, started.elapsed().as_millis()
                );
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!sender.control.clipboard_allowed());
    sender.control.set_clipboard_enabled(true);
    assert!(!sender.control.clipboard_enabled());
    let stop_at = Instant::now();
    sender.control.stop();
    stopped(&sender);
    stopped(&receiver);
    assert!(
        !receiver.control.was_stopped_locally(),
        "remote disconnect must allow automatic listener restart"
    );
    println!("both_workers_stopped_ms={}", stop_at.elapsed().as_millis());
}
