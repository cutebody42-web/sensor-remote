use sensor_audit::{verify_file, AuditLog};
use sensor_client::{request, send_file, serve, LocalInteraction, Message, Mode};
use sensor_files::{Manifest, Receiver};
use sensor_identity::DeviceIdentity;
use sensor_session::ExpectedPeer;
use sensor_transport::connection::{SecureConnection, DEFAULT_TIMEOUT};
use sha2::{Digest, Sha256};
use std::net::TcpListener;

fn pin(identity: &DeviceIdentity) -> ExpectedPeer {
    ExpectedPeer {
        device_id: identity.device_id(),
        public_key: identity.keypair().public_key(),
    }
}
struct UserDecision(bool);
impl LocalInteraction for UserDecision {
    fn accept(&mut self, _: ExpectedPeer, _: Mode) -> bool {
        self.0
    }
    fn chat_reply(&mut self, text: &str) -> Option<String> {
        assert_eq!(text, "Support connected");
        Some("Customer reply".into())
    }
}

#[test]
fn attended_network_transfer_matches_original_and_creates_signed_history() {
    let root = tempfile::tempdir().unwrap();
    let receive_dir = root.path().join("received");
    std::fs::create_dir(&receive_dir).unwrap();
    let path = root.path().join("test.bin");
    let data: Vec<u8> = (0..1_048_737).map(|n| (n % 251) as u8).collect();
    std::fs::write(&path, &data).unwrap();
    let audit_path = root.path().join("audit.jsonl");
    let client = DeviceIdentity::generate();
    let host = DeviceIdentity::generate();
    let client_pin = pin(&client);
    let host_pin = pin(&host);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let log_path = audit_path.clone();
    let shared = receive_dir.clone();
    let worker = std::thread::spawn(move || {
        let receiver = Receiver::open(&shared).unwrap();
        let mut log = AuditLog::open(&log_path, host.keypair()).unwrap();
        let connection = SecureConnection::accept(
            listener.accept().unwrap().0,
            &host,
            client_pin,
            DEFAULT_TIMEOUT,
        )
        .unwrap();
        serve(
            connection,
            host.device_id(),
            &receiver,
            &mut log,
            &mut UserDecision(true),
        )
        .unwrap();
    });
    let mut connection =
        SecureConnection::connect(address, &client, host_pin, DEFAULT_TIMEOUT).unwrap();
    let completed = send_file(&mut connection, &path, None, |_, _, _| {}).unwrap();
    worker.join().unwrap();
    assert_eq!(std::fs::read(receive_dir.join("test.bin")).unwrap(), data);
    assert_eq!(completed.sha256, <[u8; 32]>::from(Sha256::digest(&data)));
    assert_eq!(
        verify_file(&audit_path, &host_pin.public_key)
            .unwrap()
            .records,
        5
    );
}

#[test]
fn rejected_session_cannot_transfer_any_file() {
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().to_path_buf();
    let client = DeviceIdentity::generate();
    let host = DeviceIdentity::generate();
    let client_pin = pin(&client);
    let host_pin = pin(&host);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let mut log = AuditLog::open(&shared.join("audit.jsonl"), host.keypair()).unwrap();
        let receiver = Receiver::open(&shared).unwrap();
        let connection = SecureConnection::accept(
            listener.accept().unwrap().0,
            &host,
            client_pin,
            DEFAULT_TIMEOUT,
        )
        .unwrap();
        serve(
            connection,
            host.device_id(),
            &receiver,
            &mut log,
            &mut UserDecision(false),
        )
        .unwrap();
    });
    let mut connection =
        SecureConnection::connect(address, &client, host_pin, DEFAULT_TIMEOUT).unwrap();
    assert!(request(&mut connection, Mode::FileTransfer).is_err());
    worker.join().unwrap();
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn accepted_chat_cannot_smuggle_file_operations() {
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().to_path_buf();
    let client = DeviceIdentity::generate();
    let host = DeviceIdentity::generate();
    let client_pin = pin(&client);
    let host_pin = pin(&host);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let mut log = AuditLog::open(&shared.join("audit.jsonl"), host.keypair()).unwrap();
        let receiver = Receiver::open(&shared).unwrap();
        let connection = SecureConnection::accept(
            listener.accept().unwrap().0,
            &host,
            client_pin,
            DEFAULT_TIMEOUT,
        )
        .unwrap();
        serve(
            connection,
            host.device_id(),
            &receiver,
            &mut log,
            &mut UserDecision(true),
        )
        .is_err()
    });
    let mut connection =
        SecureConnection::connect(address, &client, host_pin, DEFAULT_TIMEOUT).unwrap();
    request(&mut connection, Mode::Chat).unwrap();
    connection
        .send(&Message::Chat("Support connected".into()))
        .unwrap();
    assert!(
        matches!(connection.receive::<Message>().unwrap(), Message::Chat(text) if text == "Customer reply")
    );
    connection
        .send(&Message::FileOffer(Manifest {
            name: "unauthorized.txt".into(),
            size: 1,
            sha256: [0; 32],
        }))
        .unwrap();
    assert!(connection.receive::<Message>().is_err());
    assert!(worker.join().unwrap());
    assert!(!root.path().join("unauthorized.txt").exists());
}
