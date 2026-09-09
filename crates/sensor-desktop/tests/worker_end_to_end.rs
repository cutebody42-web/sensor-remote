use sensor_desktop::worker::{self, Event, Job, Route, Task};
use sensor_identity::DeviceIdentity;
use sensor_session::ExpectedPeer;
use std::{net::SocketAddr, path::Path, sync::mpsc::SyncSender, time::Duration};

fn pin(identity: &DeviceIdentity) -> ExpectedPeer {
    ExpectedPeer {
        device_id: identity.device_id(),
        public_key: identity.keypair().public_key(),
    }
}
fn next(job: &Job) -> Event {
    job.events
        .recv_timeout(Duration::from_secs(10))
        .expect("bounded worker event wait")
}
fn listen(identity: DeviceIdentity, peer: ExpectedPeer, root: &Path) -> (Job, SocketAddr) {
    let job = worker::start(
        Task::Host {
            route: Route::Direct("127.0.0.1:0".parse().unwrap()),
            receive_dir: root.into(),
            auto_accept: false,
            accept_any: false,
        },
        identity,
        peer,
        root.into(),
    );
    let Event::Status(text) = next(&job) else {
        panic!("expected listener status")
    };
    let address = text
        .strip_prefix("Listening on ")
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    (job, address)
}
fn listen_first_connection(
    identity: DeviceIdentity,
    root: &Path,
) -> (Job, SocketAddr, DeviceIdentity) {
    let identity_for_pin = identity.clone();
    let job = worker::start(
        Task::Host {
            route: Route::Direct("127.0.0.1:0".parse().unwrap()),
            receive_dir: root.into(),
            auto_accept: false,
            accept_any: true,
        },
        identity,
        ExpectedPeer {
            device_id: sensor_core::DeviceId::new(100_000_000).unwrap(),
            public_key: [0; 32],
        },
        root.into(),
    );
    let Event::Status(text) = next(&job) else {
        panic!("expected listener status")
    };
    let address = text
        .strip_prefix("Listening on ")
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    (job, address, identity_for_pin)
}
fn consent(job: &Job, accept: bool) {
    loop {
        match next(job) {
            Event::Consent(_, _, response) => {
                response.send(accept).unwrap();
                return;
            }
            Event::Status(_) => (),
            _ => panic!("expected consent before operation"),
        }
    }
}
fn finish(job: &Job) -> Result<String, String> {
    loop {
        match next(job) {
            Event::Finished(result) => return result,
            Event::Status(_) | Event::Progress { .. } => (),
            _ => panic!("unexpected user interaction"),
        }
    }
}
fn compose(job: &Job) -> SyncSender<Option<String>> {
    loop {
        match next(job) {
            Event::Compose(sender) => return sender,
            Event::Status(_) => (),
            _ => panic!("expected compose prompt"),
        }
    }
}

#[test]
fn native_worker_file_send_requires_consent_and_finishes_with_exact_bytes() {
    let host_dir = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();
    let host = DeviceIdentity::generate();
    let client = DeviceIdentity::generate();
    let expected = pin(&host);
    let (receiver, address) = listen(host, pin(&client), host_dir.path());
    let file = client_dir.path().join("native-transfer.bin");
    let bytes: Vec<_> = (0..524_395).map(|i| (i % 251) as u8).collect();
    std::fs::write(&file, &bytes).unwrap();
    let sender = worker::start(
        Task::Send {
            route: Route::Direct(address),
            file,
            resume: None,
        },
        client,
        expected,
        client_dir.path().into(),
    );
    assert!(!host_dir.path().join("native-transfer.bin").exists());
    consent(&receiver, true);
    let result = finish(&sender).unwrap();
    assert!(result.starts_with("Delivered 524395 bytes."));
    assert!(result.contains("verified SHA-256:"));
    assert!(finish(&receiver).is_ok());
    assert_eq!(
        std::fs::read(host_dir.path().join("native-transfer.bin")).unwrap(),
        bytes
    );
}

#[test]
fn native_worker_chat_is_bidirectional_and_local_stop_interrupts_wait() {
    let host_dir = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();
    let host = DeviceIdentity::generate();
    let client = DeviceIdentity::generate();
    let expected = pin(&host);
    let (receiver, address) = listen(host, pin(&client), host_dir.path());
    let sender = worker::start(
        Task::Chat {
            route: Route::Direct(address),
        },
        client,
        expected,
        client_dir.path().into(),
    );
    consent(&receiver, true);
    compose(&sender)
        .send(Some("test from viewer".into()))
        .unwrap();
    loop {
        match next(&receiver) {
            Event::Chat(text) => {
                assert_eq!(text, "test from viewer");
                break;
            }
            Event::Status(_) => (),
            _ => panic!("expected chat"),
        }
    }
    compose(&receiver)
        .send(Some("test from host".into()))
        .unwrap();
    let Event::Chat(text) = next(&sender) else {
        panic!("expected reply")
    };
    assert_eq!(text, "test from host");
    let _waiting = compose(&sender);
    let stopped_at = std::time::Instant::now();
    sender.control.stop();
    assert!(finish(&sender).is_ok());
    assert!(finish(&receiver).is_err()); // Transport abort is recorded, not a fake graceful close.
    assert!(stopped_at.elapsed() < Duration::from_secs(2));
}

#[test]
fn native_worker_explicit_chat_close_is_graceful_on_both_ends() {
    let host_dir = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();
    let host = DeviceIdentity::generate();
    let client = DeviceIdentity::generate();
    let expected = pin(&host);
    let (receiver, address) = listen(host, pin(&client), host_dir.path());
    let sender = worker::start(
        Task::Chat {
            route: Route::Direct(address),
        },
        client,
        expected,
        client_dir.path().into(),
    );
    consent(&receiver, true);
    compose(&sender).send(None).unwrap();
    assert!(finish(&sender).is_ok());
    assert!(finish(&receiver).is_ok());
}

#[test]
fn first_connection_flow_authenticates_unknown_key_before_consent() {
    let host_dir = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();
    let (receiver, address, host) =
        listen_first_connection(DeviceIdentity::generate(), host_dir.path());
    let client = DeviceIdentity::generate();
    let sender = worker::start(
        Task::Chat {
            route: Route::Direct(address),
        },
        client.clone(),
        ExpectedPeer {
            device_id: host.device_id(),
            public_key: [0; 32],
        },
        client_dir.path().into(),
    );
    consent(&receiver, true);
    compose(&sender)
        .send(Some("first connection works".into()))
        .unwrap();
    let text = loop {
        match next(&receiver) {
            Event::Chat(text) => break text,
            Event::Status(_) => (),
            _ => panic!("expected first-connection chat"),
        }
    };
    assert_eq!(text, "first connection works");
    compose(&receiver).send(Some("approved".into())).unwrap();
    let text = loop {
        match next(&sender) {
            Event::Chat(text) => break text,
            Event::Status(_) => (),
            _ => panic!("expected first-connection reply"),
        }
    };
    assert_eq!(text, "approved");
    sender.control.stop();
    receiver.control.stop();
}

#[test]
fn rejected_desktop_worker_session_cannot_create_files() {
    let host_dir = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();
    let host = DeviceIdentity::generate();
    let client = DeviceIdentity::generate();
    let expected = pin(&host);
    let (receiver, address) = listen(host, pin(&client), host_dir.path());
    let file = client_dir.path().join("rejected.txt");
    std::fs::write(&file, b"not allowed").unwrap();
    let sender = worker::start(
        Task::Send {
            route: Route::Direct(address),
            file,
            resume: None,
        },
        client,
        expected,
        client_dir.path().into(),
    );
    consent(&receiver, false);
    assert!(finish(&sender).is_err());
    assert!(finish(&receiver).is_ok());
    assert!(!host_dir.path().join("rejected.txt").exists());
    assert_eq!(std::fs::read_dir(host_dir.path()).unwrap().count(), 1); // audit only
}
