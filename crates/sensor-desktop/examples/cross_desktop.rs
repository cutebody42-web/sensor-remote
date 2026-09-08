//! Explicit, temporary two-machine Windows video fixture; never packaged.
#[cfg(windows)]
mod fixture {
    use sensor_desktop::worker::{self, Event, RenderRoute, Route, Task};
    use sensor_identity::{DeviceIdentity, IdentityFileStore};
    use sensor_session::ExpectedPeer;
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::{
        error::Error,
        fs,
        path::Path,
        time::{Duration, Instant},
    };

    const SERVER: &str = "https://sensor-rendezvous-production.up.railway.app";
    type Result<T> = std::result::Result<T, Box<dyn Error>>;
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Public {
        server: String,
        device_id: String,
        public_key: String,
    }
    fn identity(root: &Path) -> Result<DeviceIdentity> {
        fs::create_dir_all(root)?;
        Ok(
            IdentityFileStore::new(root.join("identity.bin"), sensor_windows::UserDpapi)
                .load_or_create()?,
        )
    }
    fn prepare(root: &Path, public: &Path) -> Result<()> {
        if public.exists() {
            return Err("public output already exists; choose a fresh fixture directory".into());
        }
        let identity = identity(root)?;
        let output = Public {
            server: SERVER.into(),
            device_id: identity.device_id().to_string(),
            public_key: sensor_desktop::hex(&identity.keypair().public_key()),
        };
        fs::write(public, serde_json::to_vec_pretty(&output)?)?;
        println!("DESKTOP_FIXTURE_PREPARED public_identity_only=true");
        Ok(())
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn prepare_control(root: &Path, public: &Path, target: &Path) -> Result<()> {
        prepare(root, public)?;
        sensor_windows::desktop::interactive_desktop()?;
        let _target = Child(
            std::process::Command::new(target)
                .env("SENSOR_QA_CLOUD_FOCUS", "1")
                .arg(root.join("qa-state.json"))
                .spawn()?,
        );
        let until = Instant::now() + Duration::from_secs(30);
        while Instant::now() < until {
            if let Ok(bytes) = fs::read(root.join("qa-state.json")) {
                let state: serde_json::Value = serde_json::from_slice(&bytes)?;
                if state["focused"] == true
                    && state["text_focused"] == true
                    && state["painted_frames"].as_u64().unwrap_or(0) > 5
                {
                    fs::write(root.join("target-public.json"), bytes)?;
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("Cloud fixture did not establish its own editable focus".into())
    }
    fn host(root: &Path, peer: ExpectedPeer, target: &Path, input: bool) -> Result<()> {
        // A headless/service/locked runner is an explicit failed prerequisite,
        // never a reason to bypass desktop security or synthesize fake capture.
        sensor_windows::desktop::interactive_desktop()?;
        let identity = identity(root)?;
        let mut command = std::process::Command::new(target);
        command.arg(root.join("qa-state.json"));
        if input {
            command.env("SENSOR_QA_CLOUD_FOCUS", "1");
        }
        let _target = Child(command.spawn()?);
        let receive = root.join("receive");
        fs::create_dir_all(&receive)?;
        let job = worker::start(
            Task::Host {
                route: Route::Render(RenderRoute {
                    server: SERVER.into(),
                }),
                receive_dir: receive,
                auto_accept: false,
                accept_any: false,
            },
            identity,
            peer,
            root.into(),
        );
        let until = Instant::now() + Duration::from_secs(480);
        let mut accepted = false;
        while Instant::now() < until {
            match job.events.recv_timeout(Duration::from_secs(1)) {
                Ok(Event::Consent(actual, mode, answer)) => {
                    let expected_mode = matches!(
                        (input, mode),
                        (true, sensor_client::Mode::RemoteControl)
                            | (false, sensor_client::Mode::ScreenView)
                    );
                    let fixture_ready = if input {
                        let state: serde_json::Value =
                            serde_json::from_slice(&fs::read(root.join("qa-state.json"))?)?;
                        let published: serde_json::Value =
                            serde_json::from_slice(&fs::read(root.join("target-public.json"))?)?;
                        state["focused"] == true
                            && state["text_focused"] == true
                            && state["text_center"] == published["text_center"]
                    } else {
                        true
                    };
                    let allow = actual == peer && expected_mode && fixture_ready;
                    answer.send(allow)?;
                    accepted = allow;
                }
                Ok(Event::Online(true)) => println!("DESKTOP_HOST_ONLINE pinned_viewer_only=true"),
                Ok(Event::Status(status)) => println!("DESKTOP_HOST_STATUS {status}"),
                Ok(Event::Finished(result)) => {
                    result?;
                    let (captured, encoded) = job.control.capture_statistics();
                    if !accepted || captured < 5 || encoded < 5 {
                        return Err("insufficient real captured/encoded frames".into());
                    }
                    if input {
                        let state: serde_json::Value =
                            serde_json::from_slice(&fs::read(root.join("qa-state.json"))?)?;
                        if state["typed_expected"] != true
                            || state["wheel"] != true
                            || state["clicks"].as_u64().unwrap_or(0) == 0
                        {
                            return Err("Cloud QA did not verify actual Unicode keyboard, mouse click and wheel".into());
                        }
                        println!("CROSS_COMPUTER_INPUT_PASS native_unicode=true mouse_click=true wheel=true");
                    }
                    println!("CROSS_DESKTOP_HOST_PASS platform=windows captured={captured} encoded={encoded} explicit_view_only={}", !input);
                    return Ok(());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("desktop worker disconnected".into())
                }
                _ => {}
            }
        }
        Err("bounded desktop host deadline exceeded".into())
    }
    fn view(root: &Path, public: &Path, qa: Option<&Path>) -> Result<()> {
        let bytes = fs::read(public)?;
        if bytes.len() > 4096 {
            return Err("public metadata too large".into());
        }
        let public: Public = serde_json::from_slice(&bytes)?;
        if public.server != SERVER {
            return Err("unexpected fixture server".into());
        }
        let peer = sensor_desktop::peer(&public.device_id, &public.public_key)?;
        let identity = identity(root)?;
        let job = worker::start(
            Task::Remote {
                route: Route::Render(RenderRoute {
                    server: SERVER.into(),
                }),
                control: qa.is_some(),
                clipboard: false,
            },
            identity,
            peer,
            root.into(),
        );
        let started = Instant::now();
        let until = started + Duration::from_secs(120);
        let mut first_pixels = None;
        let mut changed = false;
        let mut first_frame = None;
        let mut format_seen = false;
        let mut format_value = None;
        let mut input_sent = false;
        while Instant::now() < until {
            while let Ok(event) = job.events.try_recv() {
                match event {
                    Event::RemoteFormat(format) => {
                        format.validate()?;
                        format_value = Some(format.clone());
                        format_seen = true;
                        println!(
                            "CLOUD_NATIVE_FORMAT width={} height={} hardware={} encoder={}",
                            format.width, format.height, format.hardware, format.encoder
                        );
                    }
                    Event::Finished(result) => {
                        return Err(format!("desktop ended before verification: {result:?}").into())
                    }
                    _ => {}
                }
            }
            if let Some(frame) = job.control.latest_frame() {
                if !format_seen {
                    return Err("frame before validated format".into());
                }
                first_frame.get_or_insert(started.elapsed());
                let digest: [u8; 32] = Sha256::digest(&frame.rgba).into();
                changed |= first_pixels.is_some_and(|previous| previous != digest);
                first_pixels.get_or_insert(digest);
            }
            let (frames, bytes, rtt) = job.control.statistics();
            if let (Some(qa), Some(format)) = (qa, &format_value) {
                if !input_sent && frames > 4 {
                    let state_bytes = fs::read(qa)?;
                    if state_bytes.len() > 4096 {
                        return Err("Oversized QA fixture state".into());
                    }
                    let state: serde_json::Value = serde_json::from_slice(&state_bytes)?;
                    if state["focused"] != true || state["text_focused"] != true {
                        return Err("Cloud synthetic target has not confirmed text focus".into());
                    }
                    let x = state["text_center"][0].as_f64().ok_or("Missing QA x")? as i32
                        - format.display.left;
                    let y = state["text_center"][1].as_f64().ok_or("Missing QA y")? as i32
                        - format.display.top;
                    if x < 0 || y < 0 {
                        return Err("QA target outside captured monitor".into());
                    }
                    use sensor_media::{DesktopMessage, Input, MouseButton};
                    for event in [
                        Input::Move {
                            x: x as u32,
                            y: y as u32,
                        },
                        Input::Button {
                            button: MouseButton::Left,
                            down: true,
                        },
                        Input::Button {
                            button: MouseButton::Left,
                            down: false,
                        },
                        Input::Text("SENSOR QA مرحبا 123".into()),
                        Input::Wheel {
                            delta: -120,
                            horizontal: false,
                        },
                        Input::ReleaseAll,
                    ] {
                        event.validate(&format.display)?;
                        if !job.control.send_remote(DesktopMessage::Input {
                            generation: format.generation,
                            event,
                        }) {
                            return Err("Input queue unavailable".into());
                        }
                    }
                    input_sent = true;
                    println!("CROSS_COMPUTER_INPUT_SENT only_synthetic_fixture=true");
                }
            }
            if frames >= 5 && changed && rtt > 0 && started.elapsed() > Duration::from_secs(30) {
                if qa.is_some() && !input_sent {
                    return Err("Input not sent".into());
                }
                if !job.control.send_remote(sensor_media::DesktopMessage::Close) {
                    return Err("cannot send graceful desktop close".into());
                }
                let end = Instant::now() + Duration::from_secs(5);
                while !job.is_finished() && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(20));
                }
                if !job.is_finished() {
                    return Err("desktop shutdown exceeded five seconds".into());
                }
                println!("CROSS_DESKTOP_VIEWER_PASS platform=windows frames={frames} encoded_bytes={bytes} first_frame_ms={} rtt_us={rtt} changed_pixels=true", first_frame.unwrap_or_default().as_millis());
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err("real two-machine video verification timed out".into())
    }
    pub fn run() -> Result<()> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        match args.as_slice() {
            [mode, root, public] if mode == "prepare" => prepare(Path::new(root), Path::new(public)),
            [mode, root, public, target] if mode == "prepare-control" => prepare_control(Path::new(root), Path::new(public), Path::new(target)),
            [mode, root, public] if mode == "view" => view(Path::new(root), Path::new(public), None),
            [mode, root, public, qa] if mode == "view-control" => view(Path::new(root), Path::new(public), Some(Path::new(qa))),
            [mode, root, id, key, target] if mode == "host" || mode == "host-control" => host(Path::new(root), sensor_desktop::peer(id, key)?, Path::new(target), mode == "host-control"),
            _ => Err("Usage: cross_desktop prepare <profile> <public-json> | view <profile> <host-json> | host <profile> <viewer-id> <viewer-key> <qa-target-exe>".into()),
        }
    }
}
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    fixture::run()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This native cross-desktop fixture requires Windows.");
}
