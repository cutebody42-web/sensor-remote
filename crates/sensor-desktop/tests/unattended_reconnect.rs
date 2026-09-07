//! Explicit real-desktop/Internet gate; ordinary signed-in user only.
#![cfg(windows)]
use sensor_desktop::{
    unattended::{self, Grant},
    worker::{self, Event, RenderRoute, Route, Task},
};
use sensor_identity::{DeviceIdentity, IdentityFileStore};
use sensor_session::ExpectedPeer;
use std::time::{Duration, Instant};
fn pin(identity: &DeviceIdentity) -> ExpectedPeer {
    ExpectedPeer {
        device_id: identity.device_id(),
        public_key: identity.keypair().public_key(),
    }
}
#[test]
#[ignore = "requires SENSOR_TEST_SERVER and unlocked Windows; captures desktop through public WSS"]
fn signed_in_unattended_grant_and_identity_survive_three_host_restarts() {
    let server = std::env::var("SENSOR_TEST_SERVER").expect("explicit public server required");
    assert!(server.starts_with("https://"));
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("received")).unwrap();
    let viewer_root = tempfile::tempdir().unwrap();
    let identity_file = root.path().join("identity.bin");
    let grant_file = root.path().join("grant.bin");
    let host = IdentityFileStore::new(&identity_file, sensor_windows::UserDpapi)
        .load_or_create()
        .unwrap();
    let viewer = DeviceIdentity::generate();
    let expected = pin(&host);
    let grant = Grant::new(pin(&viewer), sensor_desktop::updates::now().unwrap()).unwrap();
    unattended::save(
        &grant_file,
        Some(&grant),
        &sensor_windows::UserDpapi,
        &expected.public_key,
    )
    .unwrap();
    for cycle in 0..3 {
        let host = IdentityFileStore::new(&identity_file, sensor_windows::UserDpapi)
            .load()
            .unwrap();
        assert_eq!(pin(&host), expected);
        let restored = unattended::load(
            &grant_file,
            &sensor_windows::UserDpapi,
            &expected.public_key,
        )
        .unwrap()
        .unwrap();
        let route = Route::Render(RenderRoute {
            server: server.clone(),
        });
        let receiver = worker::start(
            Task::Host {
                route: route.clone(),
                receive_dir: root.path().join("received"),
                auto_accept: false,
                accept_any: false,
            },
            host,
            pin(&viewer),
            root.path().into(),
        );
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            assert!(Instant::now() < deadline, "listener restart timeout");
            match receiver.events.recv_timeout(Duration::from_secs(1)) {
                Ok(Event::Online(true)) => break,
                Ok(Event::Finished(r)) => panic!("host stopped: {r:?}"),
                _ => {}
            }
        }
        let sender = worker::start(
            Task::Remote {
                route,
                control: false,
                clipboard: false,
            },
            viewer.clone(),
            expected,
            viewer_root.path().into(),
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut accepted = false;
        let mut seen = false;
        while Instant::now() < deadline {
            while let Ok(e) = receiver.events.try_recv() {
                match e {
                    Event::Consent(peer, mode, answer) => {
                        let allow =
                            restored.permits(peer, mode, sensor_desktop::updates::now().unwrap());
                        answer.send(allow).unwrap();
                        assert!(allow);
                        accepted = true;
                    }
                    Event::Finished(r) => panic!("host ended: {r:?}"),
                    _ => {}
                }
            }
            while let Ok(e) = sender.events.try_recv() {
                if let Event::Finished(r) = e {
                    panic!("viewer ended: {r:?}");
                }
            }
            if sender.control.latest_frame().is_some() {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(seen && accepted, "no unattended real frame after restart");
        assert!(sender
            .control
            .send_remote(sensor_media::DesktopMessage::Close));
        for job in [&sender, &receiver] {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                assert!(Instant::now() < deadline, "close deadline");
                match job.events.recv_timeout(Duration::from_millis(20)) {
                    Ok(Event::Finished(r)) => {
                        assert!(r.is_ok(), "{r:?}");
                        break;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("no completion"),
                    _ => {}
                }
            }
        }
        println!("UNATTENDED_RESTART_PASS cycle={} same_identity=true persisted_dpapi_grant=true real_video=true graceful_close=true",cycle+1);
        std::thread::sleep(Duration::from_secs(2));
    }
    unattended::save(
        &grant_file,
        None,
        &sensor_windows::UserDpapi,
        &expected.public_key,
    )
    .unwrap();
    assert!(unattended::load(
        &grant_file,
        &sensor_windows::UserDpapi,
        &expected.public_key
    )
    .unwrap()
    .is_none());
    println!("UNATTENDED_REVOCATION_PASS ordinary_signed_in_desktop_only=true operating_system_reboot_tested=false");
}
