use sensor_client::{LocalInteraction, Message, Mode};
use sensor_identity::DeviceIdentity;
use sensor_render::{
    accept_with_control, connect, encode_hex, registration_message, ControlMessage,
};
use sensor_session::ExpectedPeer;
use sensor_transport::connection::SecureConnection;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message as WsMessage, WebSocket};

const TIMEOUT: Duration = Duration::from_secs(90);
struct LocalServer {
    process: Child,
    url: String,
}
impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
fn server() -> LocalServer {
    let mut process = Command::new(env!("CARGO_BIN_EXE_sensor-rendezvous"))
        .env("PORT", "0")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let output = process.stdout.take().unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    thread::spawn(move || {
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if let Some(address) = line.strip_prefix("SENSOR_LISTEN_ADDRESS=") {
                let _ = send.try_send(address.to_owned());
            }
        }
    });
    let mut server = LocalServer {
        process,
        url: String::new(),
    };
    let address: std::net::SocketAddr = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("server did not report its actual bound address")
        .parse()
        .unwrap();
    let port = address.port();
    server.url = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return server;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("server did not start");
}
fn pin(identity: &DeviceIdentity) -> ExpectedPeer {
    ExpectedPeer {
        device_id: identity.device_id(),
        public_key: identity.keypair().public_key(),
    }
}

fn listen(server: &str, host: DeviceIdentity) -> thread::JoinHandle<TcpStream> {
    let server = server.to_owned();
    let (ready, waiting) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        accept_with_control(&server, &host, TIMEOUT, &|| false, &|| {
            ready.send(()).unwrap();
        })
        .unwrap()
    });
    waiting
        .recv_timeout(TIMEOUT)
        .expect("host did not register");
    handle
}

struct Decision(bool);
impl LocalInteraction for Decision {
    fn accept(&mut self, _: ExpectedPeer, _: Mode) -> bool {
        self.0
    }
    fn chat_reply(&mut self, text: &str) -> Option<String> {
        assert_eq!(text, "SENSOR public encrypted chat");
        Some("Authenticated reply".into())
    }
}

fn encrypted_roundtrip(server: &str, mode: Mode, consent: bool) {
    encrypted_roundtrip_with_trust(server, mode, consent, false);
}

fn encrypted_roundtrip_with_trust(server: &str, mode: Mode, consent: bool, id_only: bool) {
    let host = DeviceIdentity::generate();
    let client = DeviceIdentity::generate();
    let host_pin = pin(&host);
    let target = if id_only {
        ExpectedPeer {
            device_id: host_pin.device_id,
            public_key: [0; 32],
        }
    } else {
        host_pin
    };
    let client_pin = pin(&client);
    let root = tempfile::tempdir().unwrap();
    let received = root.path().join("received");
    std::fs::create_dir(&received).unwrap();
    let audit = root.path().join("audit.jsonl");
    let file = root.path().join("payload.bin");
    let payload: Vec<u8> = (0..1_048_737).map(|n| (n % 251) as u8).collect();
    std::fs::write(&file, &payload).unwrap();
    let host_handle = listen(server, host.clone());
    let stream = connect(server, &client, target, TIMEOUT).unwrap();
    let host_stream = host_handle.join().unwrap();
    let receive_path = received.clone();
    let audit_path = audit.clone();
    let worker = thread::spawn(move || {
        let receiver = sensor_files::Receiver::open(&receive_path).unwrap();
        let mut audit = sensor_audit::AuditLog::open(&audit_path, host.keypair()).unwrap();
        let connection = SecureConnection::accept(host_stream, &host, client_pin, TIMEOUT).unwrap();
        sensor_client::serve(
            connection,
            host.device_id(),
            &receiver,
            &mut audit,
            &mut Decision(consent),
        )
        .unwrap();
    });
    let mut connection = SecureConnection::initiate(stream, &client, target, TIMEOUT).unwrap();
    if !consent {
        assert!(sensor_client::request(&mut connection, mode).is_err());
    } else if matches!(mode, Mode::FileTransfer) {
        let completed =
            sensor_client::send_file(&mut connection, &file, None, |_, _, _| {}).unwrap();
        use sha2::{Digest, Sha256};
        assert_eq!(completed.sha256, <[u8; 32]>::from(Sha256::digest(&payload)));
    } else {
        sensor_client::request(&mut connection, mode).unwrap();
        connection
            .send(&Message::Chat("SENSOR public encrypted chat".into()))
            .unwrap();
        assert!(
            matches!(connection.receive::<Message>().unwrap(), Message::Chat(text) if text == "Authenticated reply")
        );
        connection.send(&Message::Close).unwrap();
    }
    worker.join().unwrap();
    if consent && matches!(mode, Mode::FileTransfer) {
        assert_eq!(
            std::fs::read(received.join("payload.bin")).unwrap(),
            payload
        );
    }
    if !consent {
        assert!(!received.join("payload.bin").exists());
    }
    assert!(
        sensor_audit::verify_file(&audit, &host_pin.public_key)
            .unwrap()
            .records
            > 0
    );
}

#[test]
fn encrypted_chat_file_and_denial_over_local_websocket() {
    let server = server();
    encrypted_roundtrip(&server.url, Mode::Chat, true);
    encrypted_roundtrip(&server.url, Mode::FileTransfer, true);
    encrypted_roundtrip(&server.url, Mode::FileTransfer, false);
}

#[test]
fn id_only_connection_to_pinned_host_works_through_rendezvous() {
    let server = server();
    encrypted_roundtrip_with_trust(&server.url, Mode::Chat, true, true);
    encrypted_roundtrip_with_trust(&server.url, Mode::FileTransfer, false, true);
}

#[test]
#[ignore = "requires explicitly configured deployed HTTPS service"]
fn deployed_wss_encrypted_chat_file_and_denial() {
    let url = std::env::var("SENSOR_TEST_SERVER")
        .expect("set SENSOR_TEST_SERVER to the deployed HTTPS base URL");
    assert!(url.starts_with("https://"));
    encrypted_roundtrip(&url, Mode::Chat, true);
    encrypted_roundtrip(&url, Mode::FileTransfer, true);
    encrypted_roundtrip(&url, Mode::FileTransfer, false);
}

fn control(socket: &mut WebSocket<TcpStream>) -> ControlMessage {
    let WsMessage::Text(text) = socket.read().unwrap() else {
        panic!("expected control text")
    };
    serde_json::from_str(&text).unwrap()
}
fn raw(server: &str) -> WebSocket<TcpStream> {
    let uri: tungstenite::http::Uri = server.parse().unwrap();
    let stream = TcpStream::connect((uri.host().unwrap(), uri.port_u16().unwrap())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    tungstenite::client(sensor_render::websocket_url(server).unwrap(), stream)
        .unwrap()
        .0
}
fn send(socket: &mut WebSocket<TcpStream>, msg: &ControlMessage) {
    socket
        .send(WsMessage::text(serde_json::to_string(msg).unwrap()))
        .unwrap();
}

#[test]
fn delayed_registration_replay_invalid_token_and_role_tampering() {
    let server = server();
    let identity = DeviceIdentity::generate();
    let mut socket = raw(&server.url);
    let ControlMessage::Challenge { nonce } = control(&mut socket) else {
        panic!()
    };
    let id = identity.device_id().to_string();
    let key = encode_hex(&identity.keypair().public_key());
    let signature = encode_hex(
        &identity
            .keypair()
            .sign(&registration_message(&id, &key, &nonce, true)),
    );
    let register = ControlMessage::Register {
        device_id: id,
        public_key: key,
        nonce,
        signature,
        listen: true,
    };
    thread::sleep(Duration::from_millis(200));
    send(&mut socket, &register);
    assert!(matches!(
        control(&mut socket),
        ControlMessage::Registered { .. }
    ));
    send(
        &mut socket,
        &ControlMessage::Heartbeat {
            token: String::new(),
        },
    );
    assert!(
        matches!(control(&mut socket), ControlMessage::Error {code,..} if code == "unauthorized")
    );
    let mut replay = raw(&server.url);
    let _ = control(&mut replay);
    send(&mut replay, &register);
    assert!(matches!(control(&mut replay), ControlMessage::Error { .. }));
    if let ControlMessage::Register {
        device_id,
        public_key,
        nonce,
        signature,
        ..
    } = register
    {
        assert!(sensor_render::verify_registration(
            &device_id,
            &public_key,
            &nonce,
            &signature,
            false
        )
        .is_err());
    }
}

#[test]
fn outgoing_connection_does_not_replace_own_listener_and_stop_cancels_wait() {
    let server = server();
    let client = DeviceIdentity::generate();
    let host = DeviceIdentity::generate();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_copy = stop.clone();
    let (ready, wait) = mpsc::sync_channel(1);
    let url = server.url.clone();
    let client_copy = client.clone();
    let own_listener = thread::spawn(move || {
        accept_with_control(
            &url,
            &client_copy,
            TIMEOUT,
            &|| stop_copy.load(Ordering::SeqCst),
            &|| {
                ready.send(()).unwrap();
            },
        )
    });
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    let expected = pin(&host);
    let host_listener = listen(&server.url, host);
    let mut outgoing = connect(&server.url, &client, expected, TIMEOUT).unwrap();
    let mut incoming = host_listener.join().unwrap();
    outgoing.write_all(b"paired").unwrap();
    let mut received = [0; 6];
    incoming.read_exact(&mut received).unwrap();
    assert_eq!(&received, b"paired");
    assert!(!own_listener.is_finished());
    stop.store(true, Ordering::SeqCst);
    assert!(own_listener.join().unwrap().is_err());
}
