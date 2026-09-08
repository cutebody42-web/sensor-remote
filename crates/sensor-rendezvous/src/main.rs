//! Render Free proof-of-concept rendezvous, presence, and opaque relay.
//!
//! This service deliberately does not terminate SENSOR endpoint encryption.
//! It authenticates device registration, pairs an initiator with an online
//! target, and forwards binary WebSocket frames without decoding them.

use rand_core::{OsRng, RngCore};
use sensor_render::{
    decode_public_key, encode_hex, token_hash, verify_registration, websocket_config,
    ControlMessage, MAX_CONTROL_BYTES, MAX_RELAY_BYTES,
};
use serde_json::json;
use std::{
    collections::HashMap,
    env,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tungstenite::{accept_with_config, Message, WebSocket};

const DEFAULT_PORT: u16 = 10_000;
const SOCKET_POLL: Duration = Duration::from_millis(5);
const TOKEN_TTL_SECONDS: u64 = 2_592_000;
const PRESENCE_TTL: Duration = Duration::from_secs(90);
const HTTP_HEADER_LIMIT: usize = 16 * 1024;

type ServerWebSocket = WebSocket<TcpStream>;

#[derive(Default)]
struct Metrics {
    connections: std::sync::atomic::AtomicUsize,
    relayed_bytes: std::sync::atomic::AtomicU64,
    active_sessions: std::sync::atomic::AtomicU64,
}

#[derive(Clone, Debug)]
struct Limits {
    max_bitrate: Option<u64>,
    max_fps: Option<u32>,
    max_resolution: Option<String>,
}

struct DeviceEntry {
    public_key: [u8; 32],
    commands: mpsc::SyncSender<PeerOffer>,
    busy: bool,
    connection_id: u64,
    last_seen: Instant,
}

struct PeerOffer {
    initiator_device_id: String,
    initiator_public_key: String,
    websocket: ServerWebSocket,
    guard: ConnectionGuard,
}

struct Registration {
    device_id: String,
    public_key: [u8; 32],
    token_hash: String,
    receiver: mpsc::Receiver<PeerOffer>,
    connection_id: u64,
    registered_at: Instant,
    listen: bool,
    _keep_sender: mpsc::SyncSender<PeerOffer>,
}

struct State {
    devices: Mutex<HashMap<String, DeviceEntry>>,
    metrics: Metrics,
    limits: Limits,
}

#[derive(Clone)]
struct Target {
    commands: mpsc::SyncSender<PeerOffer>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::var("PORT")
        .unwrap_or_else(|_| DEFAULT_PORT.to_string())
        .parse::<u16>()
        .map_err(|_| "PORT must be a valid TCP port")?;
    let bind = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&bind)?;
    // Report the socket actually reserved by the OS, including PORT=0.
    println!("SENSOR_LISTEN_ADDRESS={}", listener.local_addr()?);
    let limits = Limits {
        max_bitrate: optional_u64("RELAY_MAX_BITRATE")?.or(Some(2_000_000)),
        max_fps: optional_u64("RELAY_MAX_FPS")?
            .map(|value| value as u32)
            .or(Some(15)),
        max_resolution: env::var("RELAY_MAX_RESOLUTION")
            .ok()
            .or(Some("1280x720".into())),
    };
    let state = Arc::new(State {
        devices: Mutex::new(HashMap::new()),
        metrics: Metrics::default(),
        limits,
    });
    eprintln!("SENSOR rendezvous listening on {bind} (mode=RENDER_TEST)");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if state
                    .metrics
                    .connections
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    >= 64
                {
                    state
                        .metrics
                        .connections
                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    drop(stream);
                    continue;
                }
                let state = state.clone();
                let guard = ConnectionGuard(state.clone());
                if let Err(error) = thread::Builder::new()
                    .name("sensor-rendezvous-client".into())
                    .spawn(move || {
                        handle_connection(stream, state, guard);
                    })
                {
                    eprintln!("client worker unavailable: {error}");
                }
            }
            Err(error) => eprintln!("accept failed: {error}"),
        }
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, state: Arc<State>, guard: ConnectionGuard) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let path = match peek_path(&stream) {
        Ok(path) => path,
        Err(error) => {
            let _ = http_response(&mut stream, 400, json!({"error": error.to_string()}));
            return;
        }
    };
    if path == "/health" || path == "/" {
        if consume_headers(&mut stream).is_err() {
            let _ = http_response(&mut stream, 400, json!({"error": "invalid headers"}));
            return;
        }
        let _ = http_response(&mut stream, 200, health(&state));
        return;
    }
    if let Some(device_id) = path
        .strip_prefix("/api/v1/lookup/")
        .or_else(|| path.strip_prefix("/devices/"))
    {
        if consume_headers(&mut stream).is_err() {
            let _ = http_response(&mut stream, 400, json!({"error": "invalid headers"}));
            return;
        }
        let (status, body) = lookup(&state, device_id);
        let _ = http_response(&mut stream, status, body);
        return;
    }
    if path == "/ws" || path == "/relay" {
        match accept_with_config(stream, Some(websocket_config(MAX_CONTROL_BYTES))) {
            Ok(websocket) => handle_websocket(websocket, state, guard),
            Err(error) => eprintln!("WebSocket handshake failed: {error}"),
        }
        return;
    }
    let _ = consume_headers(&mut stream);
    let _ = http_response(&mut stream, 404, json!({"error": "not found"}));
}

fn handle_websocket(mut websocket: ServerWebSocket, state: Arc<State>, guard: ConnectionGuard) {
    let _ = websocket
        .get_mut()
        .set_read_timeout(Some(Duration::from_secs(10)));
    let _ = websocket
        .get_mut()
        .set_write_timeout(Some(Duration::from_secs(30)));
    let registration = match register(&mut websocket, &state) {
        Ok(value) => value,
        Err(error) => {
            let _ = send_error(&mut websocket, "registration_failed", &error.to_string());
            return;
        }
    };
    let device_id = registration.device_id.clone();
    let _ = websocket.get_mut().set_read_timeout(Some(SOCKET_POLL));
    let connection_id = registration.connection_id;
    let result = endpoint_loop(websocket, state.clone(), registration, guard);
    remove_device(&state, &device_id, connection_id);
    if let Err(error) = result {
        eprintln!("device {device_id} disconnected: {error}");
    }
}

fn register(websocket: &mut ServerWebSocket, state: &Arc<State>) -> Result<Registration, String> {
    let mut challenge = [0u8; 32];
    OsRng.fill_bytes(&mut challenge);
    let challenge = encode_hex(&challenge);
    send_control(
        websocket,
        &ControlMessage::Challenge {
            nonce: challenge.clone(),
        },
    )?;
    let message = read_control(websocket).map_err(|error| error.to_string())?;
    let ControlMessage::Register {
        device_id,
        public_key,
        nonce,
        signature,
        listen,
    } = message
    else {
        return Err("the first message must register the device".into());
    };
    if nonce != challenge {
        return Err("registration challenge does not match this connection".into());
    }
    let parsed_id = verify_registration(&device_id, &public_key, &nonce, &signature, listen)
        .map_err(|error| error.to_string())?;
    let canonical_id = parsed_id.to_string();
    let public_key_bytes = decode_public_key(&public_key).map_err(|error| error.to_string())?;
    if public_key != encode_hex(&public_key_bytes) {
        return Err("public key must use lowercase hexadecimal encoding".into());
    }
    let mut raw_token = [0u8; 32];
    OsRng.fill_bytes(&mut raw_token);
    let token = encode_hex(&raw_token);
    let token_hash_value = token_hash(&token);
    let (sender, receiver) = mpsc::sync_channel(1);
    let connection_id = random_connection_id();
    if listen {
        let mut devices = state
            .devices
            .lock()
            .map_err(|_| "device directory unavailable".to_owned())?;
        if let Some(existing) = devices.get(&canonical_id) {
            if existing.public_key != public_key_bytes {
                return Err("device ID is already registered with a different key".into());
            }
        }
        devices.insert(
            canonical_id.clone(),
            DeviceEntry {
                public_key: public_key_bytes,
                commands: sender.clone(),
                busy: false,
                connection_id,
                last_seen: Instant::now(),
            },
        );
    }
    if let Err(error) = send_control(
        websocket,
        &ControlMessage::Registered {
            token,
            expires_seconds: TOKEN_TTL_SECONDS,
        },
    ) {
        remove_device(state, &canonical_id, connection_id);
        return Err(error);
    }
    Ok(Registration {
        device_id: canonical_id,
        public_key: public_key_bytes,
        token_hash: token_hash_value_as_string(token_hash_value),
        receiver,
        connection_id,
        registered_at: Instant::now(),
        listen,
        _keep_sender: sender,
    })
}

fn endpoint_loop(
    mut websocket: ServerWebSocket,
    state: Arc<State>,
    registration: Registration,
    guard: ConnectionGuard,
) -> Result<(), String> {
    let Registration {
        device_id,
        public_key,
        token_hash,
        receiver,
        connection_id,
        registered_at,
        listen,
        _keep_sender,
    } = registration;
    let mut last_seen = Instant::now();
    let mut control_window = Instant::now();
    let mut control_count = 0u32;
    loop {
        if registered_at.elapsed() >= Duration::from_secs(TOKEN_TTL_SECONDS) {
            return Err("registration token expired".into());
        }
        if last_seen.elapsed() > PRESENCE_TTL
            || (listen && presence_expired(&state, &device_id, connection_id))
        {
            return Err("device presence expired".into());
        }
        match receiver.try_recv() {
            Ok(offer) => {
                state
                    .metrics
                    .active_sessions
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let result = relay_pair(
                    websocket,
                    offer,
                    &state.metrics,
                    &state.limits,
                    &device_id,
                    &encode_hex(&public_key),
                );
                state
                    .metrics
                    .active_sessions
                    .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                return result;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("device registration was replaced".into())
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        match websocket.read() {
            Ok(Message::Text(text)) if text.len() <= MAX_CONTROL_BYTES => {
                if control_window.elapsed() >= Duration::from_secs(1) {
                    control_window = Instant::now();
                    control_count = 0;
                }
                control_count += 1;
                if control_count > 10 {
                    return Err("control rate limit exceeded".into());
                }
                let message: ControlMessage =
                    serde_json::from_str(text.as_ref()).map_err(|_| "invalid control JSON")?;
                match message {
                    ControlMessage::Connect {
                        token: supplied,
                        target_device_id,
                        target_public_key,
                    } => {
                        if listen {
                            send_error(
                                &mut websocket,
                                "invalid_state",
                                "listener cannot initiate a session",
                            )?;
                            continue;
                        }
                        if !valid_token(&supplied, &token_hash) {
                            send_error(
                                &mut websocket,
                                "unauthorized",
                                "registration token rejected",
                            )?;
                            continue;
                        }
                        touch_device(&state, &device_id, connection_id);
                        let target_id = canonical_id(&target_device_id)?;
                        if target_id == device_id {
                            send_error(&mut websocket, "self_connection", "choose another device")?;
                            continue;
                        }
                        let target = match find_target(&state, &target_id, &target_public_key) {
                            Ok(target) => target,
                            Err(error) => {
                                send_error(&mut websocket, "target_unavailable", &error)?;
                                continue;
                            }
                        };
                        websocket.set_config(|c| {
                            c.max_message_size = Some(MAX_RELAY_BYTES);
                            c.max_frame_size = Some(MAX_RELAY_BYTES);
                        });
                        let offer = PeerOffer {
                            initiator_device_id: device_id.clone(),
                            initiator_public_key: encode_hex(&public_key),
                            websocket,
                            guard,
                        };
                        if target.commands.try_send(offer).is_err() {
                            return Err("target disconnected before pairing".into());
                        }
                        return Ok(());
                    }
                    ControlMessage::Heartbeat { token: supplied } => {
                        if !valid_token(&supplied, &token_hash) {
                            send_error(
                                &mut websocket,
                                "unauthorized",
                                "registration token rejected",
                            )?;
                            continue;
                        }
                        touch_device(&state, &device_id, connection_id);
                        last_seen = Instant::now();
                        send_control(&mut websocket, &ControlMessage::HeartbeatAck)?;
                    }
                    ControlMessage::Register { .. } => {
                        send_error(
                            &mut websocket,
                            "invalid_state",
                            "device is already registered",
                        )?;
                    }
                    _ => send_error(
                        &mut websocket,
                        "invalid_state",
                        "unexpected control message",
                    )?,
                }
            }
            Ok(Message::Text(_)) => return Err("control message exceeds size limit".into()),
            Ok(Message::Ping(payload)) => websocket
                .send(Message::Pong(payload))
                .map_err(|error| error.to_string())?,
            Ok(Message::Pong(_)) => {} // Only authenticated heartbeat tokens renew presence.
            Ok(Message::Close(_)) => return Ok(()),
            Ok(Message::Binary(_)) | Ok(Message::Frame(_)) => {
                return Err("binary data arrived before pairing".into())
            }
            Err(tungstenite::Error::Io(error))
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn relay_pair(
    mut target_websocket: ServerWebSocket,
    offer: PeerOffer,
    metrics: &Metrics,
    limits: &Limits,
    target_device_id: &str,
    target_public_key: &str,
) -> Result<(), String> {
    let _initiator_guard = offer.guard;
    let mut initiator_websocket = offer.websocket;
    send_control(
        &mut target_websocket,
        &ControlMessage::PeerReady {
            peer_device_id: offer.initiator_device_id.clone(),
            peer_public_key: offer.initiator_public_key.clone(),
        },
    )?;
    send_control(
        &mut initiator_websocket,
        &ControlMessage::PeerReady {
            peer_device_id: target_device_id.to_owned(),
            peer_public_key: target_public_key.to_owned(),
        },
    )?;
    for socket in [&mut target_websocket, &mut initiator_websocket] {
        socket.set_config(|c| {
            c.max_message_size = Some(MAX_RELAY_BYTES);
            c.max_frame_size = Some(MAX_RELAY_BYTES);
        });
        socket
            .get_mut()
            .set_read_timeout(Some(SOCKET_POLL))
            .map_err(|e| e.to_string())?;
        socket
            .get_mut()
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
    }
    let mut limiter = RateLimiter::new(limits.max_bitrate);
    loop {
        let mut progressed = false;
        match target_websocket.read() {
            Ok(Message::Binary(bytes)) if bytes.len() <= MAX_RELAY_BYTES => {
                metrics
                    .relayed_bytes
                    .fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed);
                limiter.wait(bytes.len());
                initiator_websocket
                    .send(Message::binary(bytes))
                    .map_err(|error| error.to_string())?;
                progressed = true;
            }
            Ok(Message::Ping(payload)) => {
                target_websocket
                    .send(Message::Pong(payload))
                    .map_err(|error| error.to_string())?;
                progressed = true;
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Pong(_)) => progressed = true,
            Ok(Message::Text(_)) | Ok(Message::Frame(_)) | Ok(Message::Binary(_)) => break,
            Err(tungstenite::Error::Io(error))
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut => {}
            Err(error) => return Err(error.to_string()),
        }
        match initiator_websocket.read() {
            Ok(Message::Binary(bytes)) if bytes.len() <= MAX_RELAY_BYTES => {
                metrics
                    .relayed_bytes
                    .fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed);
                limiter.wait(bytes.len());
                target_websocket
                    .send(Message::binary(bytes))
                    .map_err(|error| error.to_string())?;
                progressed = true;
            }
            Ok(Message::Ping(payload)) => {
                initiator_websocket
                    .send(Message::Pong(payload))
                    .map_err(|error| error.to_string())?;
                progressed = true;
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Pong(_)) => progressed = true,
            Ok(Message::Text(_)) | Ok(Message::Frame(_)) | Ok(Message::Binary(_)) => break,
            Err(tungstenite::Error::Io(error))
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut => {}
            Err(error) => return Err(error.to_string()),
        }
        if !progressed {
            thread::sleep(SOCKET_POLL);
        }
    }
    let _ = target_websocket.close(None);
    let _ = initiator_websocket.close(None);
    Ok(())
}

fn find_target(state: &Arc<State>, device_id: &str, expected_key: &str) -> Result<Target, String> {
    expire_devices(state);
    let mut devices = state
        .devices
        .lock()
        .map_err(|_| "device directory unavailable".to_owned())?;
    let entry = devices
        .get_mut(device_id)
        .ok_or_else(|| "target device is offline".to_owned())?;
    if entry.busy {
        return Err("target is busy".into());
    }
    if expected_key != encode_hex(&entry.public_key) && expected_key != encode_hex(&[0u8; 32]) {
        return Err("target public key does not match".into());
    }
    entry.busy = true;
    Ok(Target {
        commands: entry.commands.clone(),
    })
}

fn valid_token(supplied: &str, expected_hash: &str) -> bool {
    if supplied.len() != 64 || !supplied.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let supplied = token_hash(supplied);
    let expected = decode_token_hash(expected_hash);
    supplied
        .iter()
        .zip(expected)
        .fold(0u8, |diff, (a, b)| diff | (*a ^ b))
        == 0
}

fn decode_token_hash(value: &str) -> [u8; 32] {
    let bytes = value.as_bytes();
    let mut output = [0_u8; 32];
    for (index, output_byte) in output.iter_mut().enumerate() {
        let start = index * 2;
        let Some(pair) = bytes.get(start..start.saturating_add(2)) else {
            break;
        };
        *output_byte =
            u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0);
    }
    output
}

fn token_hash_value_as_string(value: [u8; 32]) -> String {
    encode_hex(&value)
}

fn random_connection_id() -> u64 {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    u64::from_be_bytes(bytes)
}

fn touch_device(state: &Arc<State>, device_id: &str, connection_id: u64) {
    if let Ok(mut devices) = state.devices.lock() {
        if let Some(entry) = devices.get_mut(device_id) {
            if entry.connection_id == connection_id {
                entry.last_seen = Instant::now();
            }
        }
    }
}

fn presence_expired(state: &Arc<State>, device_id: &str, connection_id: u64) -> bool {
    state
        .devices
        .lock()
        .map(|devices| {
            devices.get(device_id).is_none_or(|entry| {
                entry.connection_id != connection_id || entry.last_seen.elapsed() >= PRESENCE_TTL
            })
        })
        .unwrap_or(true)
}

fn expire_devices(state: &Arc<State>) {
    if let Ok(mut devices) = state.devices.lock() {
        devices.retain(|_, entry| entry.last_seen.elapsed() < PRESENCE_TTL);
    }
}

fn remove_device(state: &Arc<State>, device_id: &str, connection_id: u64) {
    if let Ok(mut devices) = state.devices.lock() {
        if devices
            .get(device_id)
            .is_some_and(|entry| entry.connection_id == connection_id)
        {
            devices.remove(device_id);
        }
    }
}

fn health(state: &Arc<State>) -> serde_json::Value {
    expire_devices(state);
    let connected_devices = state.devices.lock().map(|d| d.len()).unwrap_or(0);
    json!({
        "status": "ok",
        "server": "sensor",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "RENDER_TEST",
        "database": "in_memory_presence",
        "connected_devices": connected_devices,
        "active_sessions": state.metrics.active_sessions.load(std::sync::atomic::Ordering::Relaxed),
        "relayed_bytes": state.metrics.relayed_bytes.load(std::sync::atomic::Ordering::Relaxed),
        "relay_max_bitrate": state.limits.max_bitrate,
        "relay_max_fps": state.limits.max_fps,
        "relay_max_resolution": state.limits.max_resolution,
    })
}

fn optional_u64(name: &str) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value.parse::<u64>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must be a non-negative integer"),
            )
        })?)),
        _ => Ok(None),
    }
}

struct RateLimiter {
    max_bitrate: Option<u64>,
    window: Instant,
    bytes: u64,
}

impl RateLimiter {
    fn new(max_bitrate: Option<u64>) -> Self {
        Self {
            max_bitrate,
            window: Instant::now(),
            bytes: 0,
        }
    }

    fn wait(&mut self, bytes: usize) {
        let Some(limit) = self.max_bitrate.filter(|limit| *limit > 0) else {
            return;
        };
        if self.window.elapsed() >= Duration::from_secs(1) {
            self.window = Instant::now();
            self.bytes = 0;
        }
        self.bytes = self.bytes.saturating_add(bytes as u64);
        let required = Duration::from_secs_f64(self.bytes as f64 * 8.0 / limit as f64);
        thread::sleep(required.saturating_sub(self.window.elapsed()));
    }
}

fn lookup(state: &Arc<State>, device_id: &str) -> (u16, serde_json::Value) {
    expire_devices(state);
    let device_id = match canonical_id(device_id) {
        Ok(id) => id,
        Err(_) => return (400, json!({"error": "invalid device ID"})),
    };
    let online = state
        .devices
        .lock()
        .map(|devices| devices.get(&device_id).is_some_and(|entry| !entry.busy))
        .unwrap_or(false);
    (
        if online { 200 } else { 404 },
        json!({
            "device_id": device_id,
            "online": online,
            "mode": "RENDER_TEST",
        }),
    )
}

fn send_control(websocket: &mut ServerWebSocket, message: &ControlMessage) -> Result<(), String> {
    let bytes = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return Err("control message too large".into());
    }
    websocket
        .send(Message::text(String::from_utf8_lossy(&bytes).into_owned()))
        .map_err(|error| error.to_string())
}

fn send_error(websocket: &mut ServerWebSocket, code: &str, message: &str) -> Result<(), String> {
    send_control(
        websocket,
        &ControlMessage::Error {
            code: code.into(),
            message: message.into(),
        },
    )
}

fn read_control(websocket: &mut ServerWebSocket) -> Result<ControlMessage, tungstenite::Error> {
    match websocket.read()? {
        Message::Text(text) if text.len() <= MAX_CONTROL_BYTES => {
            serde_json::from_str(text.as_ref()).map_err(|error| {
                tungstenite::Error::Io(io::Error::new(io::ErrorKind::InvalidData, error))
            })
        }
        Message::Ping(payload) => {
            websocket.send(Message::Pong(payload))?;
            Err(tungstenite::Error::Io(io::Error::new(
                io::ErrorKind::WouldBlock,
                "ping before registration",
            )))
        }
        _ => Err(tungstenite::Error::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected registration text",
        ))),
    }
}

fn peek_path(stream: &TcpStream) -> io::Result<String> {
    let mut buffer = vec![0u8; HTTP_HEADER_LIMIT];
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "header deadline exceeded",
            ));
        }
        let count = stream.peek(&mut buffer)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "empty request",
            ));
        }
        if let Some(end) = buffer[..count]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        {
            let request = std::str::from_utf8(&buffer[..end + 4])
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP request"))?;
            let line = request.lines().next().unwrap_or_default();
            let fields: Vec<_> = line.split_whitespace().collect();
            return match fields.as_slice() {
                ["GET", path, "HTTP/1.1" | "HTTP/1.0"] if path.starts_with('/') => {
                    Ok((*path).to_owned())
                }
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "expected HTTP GET request",
                )),
            };
        }
        if count == buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP headers exceed limit",
            ));
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn canonical_id(value: &str) -> Result<String, String> {
    let id = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .parse::<u32>()
        .map_err(|_| "invalid device ID")?;
    sensor_core::DeviceId::try_from(id)
        .map(|id| id.to_string())
        .map_err(|_| "invalid device ID".into())
}

struct ConnectionGuard(Arc<State>);
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0
            .metrics
            .connections
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

fn consume_headers(stream: &mut TcpStream) -> io::Result<()> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete HTTP headers",
            ));
        }
        buffer.extend_from_slice(&chunk[..count]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
        if buffer.len() >= HTTP_HEADER_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP headers exceed limit",
            ));
        }
    }
}

fn http_response(stream: &mut TcpStream, status: u16, body: serde_json::Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(&body).map_err(io::Error::other)?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    )?;
    stream.write_all(&bytes)
}
