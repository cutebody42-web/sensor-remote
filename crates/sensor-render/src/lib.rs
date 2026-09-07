//! Temporary HTTPS/WSS transport for the Render proof of concept.
//!
//! The native SENSOR handshake and encrypted records remain unchanged. This
//! crate only turns a Render WebSocket into a local byte stream so the existing
//! endpoint code can run over it. The Render service sees opaque binary frames,
//! not chat, files, screen pixels, or private keys.

use sensor_core::DeviceId;
use sensor_crypto::IdentityKeypair;
use sensor_identity::DeviceIdentity;
use sensor_session::ExpectedPeer;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

pub const MAX_CONTROL_BYTES: usize = 16 * 1024;
pub const MAX_RELAY_BYTES: usize = 1024 * 1024;
/// Render Free may need a cold-start interval before accepting WSS.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
const SOCKET_POLL: Duration = Duration::from_millis(5);
const HEARTBEAT: Duration = Duration::from_secs(25);
static DNS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

struct DnsSlot;
impl Drop for DnsSlot {
    fn drop(&mut self) {
        DNS_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The system resolver can block beyond socket timeouts. Bound the caller's
/// wait and cancellation independently, and cap outstanding resolver threads.
fn resolve(
    host: &str,
    port: u16,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<SocketAddr>, RenderError> {
    check_cancelled(cancelled)?;
    if Instant::now() >= deadline {
        return Err(RenderError::Timeout);
    }
    DNS_IN_FLIGHT
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            (count < 4).then_some(count + 1)
        })
        .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "DNS resolver capacity reached"))?;
    let slot = DnsSlot;
    let host = host.trim_matches(['[', ']']).to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("sensor-dns".into())
        .spawn(move || {
            let _slot = slot;
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map(|addresses| addresses.take(32).collect());
            let _ = sender.send(result);
        })?;
    loop {
        check_cancelled(cancelled)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(RenderError::Timeout);
        }
        match receiver.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(addresses) => return addresses.map_err(RenderError::Io),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("DNS resolver stopped").into())
            }
        }
    }
}

type RenderWebSocket = WebSocket<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("Render URL is invalid: {0}")]
    Url(String),
    #[error("Render WebSocket failed: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("Render transport I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Render control message failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Render service rejected the connection: {0}")]
    Rejected(String),
    #[error("Render service returned an unexpected message")]
    UnexpectedMessage,
    #[error("Render operation timed out")]
    Timeout,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    Challenge {
        nonce: String,
    },
    Register {
        device_id: String,
        public_key: String,
        nonce: String,
        signature: String,
        listen: bool,
    },
    Registered {
        token: String,
        expires_seconds: u64,
    },
    Connect {
        token: String,
        target_device_id: String,
        target_public_key: String,
    },
    PeerReady {
        peer_device_id: String,
        peer_public_key: String,
    },
    Heartbeat {
        token: String,
    },
    HeartbeatAck,
    Error {
        code: String,
        message: String,
    },
}

pub fn connect(
    server: &str,
    identity: &DeviceIdentity,
    target: ExpectedPeer,
    timeout: Duration,
) -> Result<TcpStream, RenderError> {
    connect_with_control(server, identity, target, timeout, &|| false)
}

pub fn connect_with_control(
    server: &str,
    identity: &DeviceIdentity,
    target: ExpectedPeer,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<TcpStream, RenderError> {
    let mut websocket = open(server, timeout, cancelled)?;
    let token = register(&mut websocket, identity, timeout, false, cancelled)?;
    send_control(
        &mut websocket,
        &ControlMessage::Connect {
            token: token.clone(),
            target_device_id: target.device_id.to_string(),
            target_public_key: encode_hex(&target.public_key),
        },
    )?;
    wait_for_peer(&mut websocket, &token, Some(timeout), cancelled)?;
    local_stream(websocket)
}

pub fn accept(
    server: &str,
    identity: &DeviceIdentity,
    timeout: Duration,
) -> Result<TcpStream, RenderError> {
    accept_with_control(server, identity, timeout, &|| false, &|| {})
}

pub fn accept_with_control(
    server: &str,
    identity: &DeviceIdentity,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
    ready: &dyn Fn(),
) -> Result<TcpStream, RenderError> {
    let mut websocket = open(server, timeout, cancelled)?;
    let token = register(&mut websocket, identity, timeout, true, cancelled)?;
    ready();
    wait_for_peer(&mut websocket, &token, None, cancelled)?;
    local_stream(websocket)
}

pub fn websocket_url(server: &str) -> Result<String, RenderError> {
    let server = server.trim().trim_end_matches('/');
    let websocket = if let Some(rest) = server.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = server.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if server.starts_with("wss://") || server.starts_with("ws://") {
        server.to_owned()
    } else {
        return Err(RenderError::Url(
            "use https://, http://, wss://, or ws://".into(),
        ));
    };
    let uri: tungstenite::http::Uri = websocket
        .parse()
        .map_err(|_| RenderError::Url("invalid server URL".into()))?;
    let host = uri
        .host()
        .ok_or_else(|| RenderError::Url("server URL has no host".into()))?;
    let loopback = host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if (uri.scheme_str() == Some("ws") && !loopback)
        || uri.authority().is_some_and(|a| a.as_str().contains('@'))
        || uri.query().is_some()
        || !matches!(uri.path(), "" | "/" | "/ws")
    {
        return Err(RenderError::Url(
            "use a public HTTPS/WSS base URL; plain WS is restricted to loopback tests".into(),
        ));
    }
    if websocket.ends_with("/ws") {
        Ok(websocket)
    } else {
        Ok(format!("{websocket}/ws"))
    }
}

fn open(
    server: &str,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<RenderWebSocket, RenderError> {
    // Tungstenite deliberately leaves Rustls' process-level crypto provider
    // choice to the application. SENSOR ships with ring enabled so the
    // native Windows client and the deployed-WSS integration test behave the
    // same way regardless of which other Rustls users are in the process.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let url = websocket_url(server)?;
    let uri: tungstenite::http::Uri = url
        .parse()
        .map_err(|error| RenderError::Url(format!("invalid WebSocket URL: {error}")))?;
    let host = uri
        .host()
        .ok_or_else(|| RenderError::Url("WebSocket URL has no host".into()))?;
    let port = uri
        .port_u16()
        .unwrap_or(if uri.scheme_str() == Some("wss") {
            443
        } else {
            80
        });
    let deadline = Instant::now() + timeout;
    let addresses = resolve(host, port, deadline, cancelled)?;
    if addresses.is_empty() {
        return Err(RenderError::Url("WebSocket host has no addresses".into()));
    }
    let mut delay = Duration::from_millis(250);
    let mut last_error = None;
    loop {
        check_cancelled(cancelled)?;
        for address in &addresses {
            check_cancelled(cancelled)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(last_error
                    .map(RenderError::WebSocket)
                    .unwrap_or(RenderError::Timeout));
            }
            match TcpStream::connect_timeout(address, remaining.min(Duration::from_secs(3))) {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    let _ = stream.set_read_timeout(Some(remaining.min(Duration::from_secs(5))));
                    let _ = stream.set_write_timeout(Some(remaining.min(Duration::from_secs(5))));
                    match tungstenite::client_tls_with_config(
                        url.as_str(),
                        stream,
                        Some(websocket_config(MAX_CONTROL_BYTES)),
                        None,
                    ) {
                        Ok((mut websocket, _)) => {
                            set_socket_timeouts(
                                &mut websocket,
                                SOCKET_POLL,
                                Duration::from_secs(30),
                            )?;
                            return Ok(websocket);
                        }
                        Err(tungstenite::HandshakeError::Failure(error)) => {
                            last_error = Some(error);
                        }
                        Err(tungstenite::HandshakeError::Interrupted(_)) => {
                            last_error = Some(tungstenite::Error::Io(io::Error::other(
                                "WebSocket handshake was interrupted",
                            )));
                        }
                    }
                }
                Err(error) => last_error = Some(tungstenite::Error::Io(error)),
            }
        }
        if Instant::now() >= deadline {
            return Err(last_error
                .map(RenderError::WebSocket)
                .unwrap_or(RenderError::Timeout));
        }
        let retry_at =
            Instant::now() + delay.min(deadline.saturating_duration_since(Instant::now()));
        while Instant::now() < retry_at {
            check_cancelled(cancelled)?;
            thread::sleep(Duration::from_millis(25));
        }
        delay = (delay * 2).min(Duration::from_secs(4));
    }
}

fn register(
    websocket: &mut RenderWebSocket,
    identity: &DeviceIdentity,
    timeout: Duration,
    listen: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<String, RenderError> {
    let deadline = Instant::now() + timeout;
    let ControlMessage::Challenge { nonce: nonce_hex } =
        read_until(websocket, deadline, cancelled)?
    else {
        return Err(RenderError::UnexpectedMessage);
    };
    decode_hex::<32>(&nonce_hex)?;
    let device_id = identity.device_id().to_string();
    let public_key = encode_hex(&identity.keypair().public_key());
    let signature = identity.keypair().sign(&registration_bytes(
        &device_id,
        &public_key,
        &nonce_hex,
        listen,
    ));
    send_control(
        websocket,
        &ControlMessage::Register {
            device_id,
            public_key,
            nonce: nonce_hex,
            signature: encode_hex(&signature),
            listen,
        },
    )?;
    loop {
        match read_until(websocket, deadline, cancelled)? {
            ControlMessage::Registered { token, .. } => return Ok(token),
            ControlMessage::Error { message, .. } => return Err(RenderError::Rejected(message)),
            ControlMessage::HeartbeatAck => continue,
            _ => return Err(RenderError::UnexpectedMessage),
        }
    }
}

fn wait_for_peer(
    websocket: &mut RenderWebSocket,
    token: &str,
    timeout: Option<Duration>,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), RenderError> {
    let deadline = timeout.map(|t| Instant::now() + t);
    let mut last_heartbeat = Instant::now();
    let mut last_ack = Instant::now();
    loop {
        check_cancelled(cancelled)?;
        if deadline.is_some_and(|d| Instant::now() >= d) || last_ack.elapsed() > CONNECT_TIMEOUT {
            return Err(RenderError::Timeout);
        }
        match receive_control(
            websocket,
            deadline.unwrap_or_else(|| Instant::now() + CONNECT_TIMEOUT),
        ) {
            Ok(ControlMessage::PeerReady { .. }) => return Ok(()),
            Ok(ControlMessage::HeartbeatAck) => {
                last_ack = Instant::now();
                continue;
            }
            Ok(ControlMessage::Error { message, .. }) => {
                return Err(RenderError::Rejected(message))
            }
            Ok(_) => return Err(RenderError::UnexpectedMessage),
            Err(RenderError::WebSocket(tungstenite::Error::Io(error)))
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut =>
            {
                if last_heartbeat.elapsed() >= HEARTBEAT {
                    send_control(
                        websocket,
                        &ControlMessage::Heartbeat {
                            token: token.to_owned(),
                        },
                    )?;
                    last_heartbeat = Instant::now();
                }
            }
            Err(error) => return Err(error),
        }
    }
}

fn local_stream(mut websocket: RenderWebSocket) -> Result<TcpStream, RenderError> {
    websocket.set_config(|config| {
        config.max_message_size = Some(MAX_RELAY_BYTES);
        config.max_frame_size = Some(MAX_RELAY_BYTES);
    });
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let client = TcpStream::connect(address)?;
    let (stream, _) = listener.accept()?;
    client.set_nodelay(true)?;
    stream.set_nodelay(true)?;
    thread::Builder::new()
        .name("sensor-render-bridge".into())
        .spawn(move || {
            bridge(stream, websocket);
        })?;
    Ok(client)
}

fn bridge(mut tcp: TcpStream, mut websocket: RenderWebSocket) {
    if tcp.set_read_timeout(Some(SOCKET_POLL)).is_err()
        || tcp
            .set_write_timeout(Some(Duration::from_secs(30)))
            .is_err()
        || set_socket_timeouts(&mut websocket, SOCKET_POLL, Duration::from_secs(30)).is_err()
    {
        let _ = tcp.shutdown(Shutdown::Both);
        return;
    }
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        match tcp.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => {
                if websocket
                    .send(Message::binary(buffer[..size].to_vec()))
                    .is_err()
                {
                    break;
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
        match websocket.read() {
            Ok(Message::Binary(bytes)) if bytes.len() <= MAX_RELAY_BYTES => {
                if tcp.write_all(&bytes).is_err() {
                    break;
                }
            }
            Ok(Message::Ping(payload)) => {
                if websocket.send(Message::Pong(payload)).is_err() {
                    break;
                }
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => break,
            Ok(Message::Text(_)) | Ok(Message::Frame(_)) => break,
            Ok(Message::Binary(_)) => break,
            Err(tungstenite::Error::Io(error))
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
    let _ = websocket.close(None);
    let _ = tcp.shutdown(Shutdown::Both);
}

fn send_control(
    websocket: &mut RenderWebSocket,
    message: &ControlMessage,
) -> Result<(), RenderError> {
    let bytes = serde_json::to_vec(message)?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return Err(RenderError::UnexpectedMessage);
    }
    websocket.send(Message::text(String::from_utf8_lossy(&bytes).into_owned()))?;
    Ok(())
}

fn receive_control(
    websocket: &mut RenderWebSocket,
    deadline: Instant,
) -> Result<ControlMessage, RenderError> {
    if Instant::now() >= deadline {
        return Err(RenderError::Timeout);
    }
    match websocket.read()? {
        Message::Text(text) if text.len() <= MAX_CONTROL_BYTES => {
            Ok(serde_json::from_str(text.as_ref())?)
        }
        Message::Ping(payload) => {
            websocket.send(Message::Pong(payload))?;
            Err(RenderError::WebSocket(tungstenite::Error::Io(
                io::Error::new(io::ErrorKind::WouldBlock, "heartbeat frame"),
            )))
        }
        Message::Pong(_) => Err(RenderError::WebSocket(tungstenite::Error::Io(
            io::Error::new(io::ErrorKind::WouldBlock, "pong frame"),
        ))),
        Message::Close(_) => Err(RenderError::Rejected(
            "service closed the connection".into(),
        )),
        _ => Err(RenderError::UnexpectedMessage),
    }
}

fn registration_bytes(device_id: &str, public_key: &str, nonce: &str, listen: bool) -> Vec<u8> {
    let mut bytes = b"SENSOR/Render/register/v2".to_vec();
    for value in [
        device_id,
        public_key,
        nonce,
        if listen { "listen" } else { "connect" },
    ] {
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    bytes
}

pub fn verify_registration(
    device_id: &str,
    public_key: &str,
    nonce: &str,
    signature: &str,
    listen: bool,
) -> Result<DeviceId, RenderError> {
    let device_id_value = device_id
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .parse::<u32>()
        .map_err(|_| RenderError::Rejected("invalid device ID".into()))?;
    let device_id = DeviceId::try_from(device_id_value)
        .map_err(|_| RenderError::Rejected("invalid device ID".into()))?;
    let public_key = decode_hex::<32>(public_key)?;
    let canonical_public_key = encode_hex(&public_key);
    let nonce = decode_hex_variable(nonce)?;
    if nonce.len() != 32 {
        return Err(RenderError::Rejected("invalid registration nonce".into()));
    }
    let signature = decode_hex_variable(signature)?;
    let canonical_nonce = encode_hex(&nonce);
    IdentityKeypair::verify(
        &public_key,
        &registration_bytes(
            &device_id.to_string(),
            &canonical_public_key,
            &canonical_nonce,
            listen,
        ),
        &signature,
    )
    .map_err(|_| RenderError::Rejected("device signature verification failed".into()))?;
    Ok(device_id)
}

pub fn decode_public_key(value: &str) -> Result<[u8; 32], RenderError> {
    decode_hex(value)
}

pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

pub fn registration_message(
    device_id: &str,
    public_key: &str,
    nonce: &str,
    listen: bool,
) -> Vec<u8> {
    registration_bytes(device_id, public_key, nonce, listen)
}

fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N], RenderError> {
    let bytes = decode_hex_variable(value)?;
    bytes
        .try_into()
        .map_err(|_| RenderError::Rejected("invalid hexadecimal length".into()))
}

fn decode_hex_variable(value: &str) -> Result<Vec<u8>, RenderError> {
    let value = value.trim();
    if !value.len().is_multiple_of(2)
        || value.len() > 128
        || !value.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(RenderError::Rejected("invalid hexadecimal value".into()));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| RenderError::Rejected("invalid hexadecimal value".into()))
        })
        .collect()
}

fn set_socket_timeouts(
    websocket: &mut RenderWebSocket,
    read: Duration,
    write: Duration,
) -> io::Result<()> {
    match websocket.get_mut() {
        MaybeTlsStream::Plain(stream) => {
            stream.set_read_timeout(Some(read))?;
            stream.set_write_timeout(Some(write))?;
        }
        MaybeTlsStream::Rustls(stream) => {
            stream.sock.set_read_timeout(Some(read))?;
            stream.sock.set_write_timeout(Some(write))?;
        }
        _ => return Err(io::Error::other("unsupported Render TLS stream")),
    }
    Ok(())
}

pub fn websocket_config(limit: usize) -> tungstenite::protocol::WebSocketConfig {
    tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(limit))
        .max_frame_size(Some(limit))
}

fn check_cancelled(cancelled: &dyn Fn() -> bool) -> Result<(), RenderError> {
    if cancelled() {
        Err(RenderError::Io(io::Error::new(
            io::ErrorKind::Interrupted,
            "operation cancelled",
        )))
    } else {
        Ok(())
    }
}

fn read_until(
    websocket: &mut RenderWebSocket,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<ControlMessage, RenderError> {
    loop {
        check_cancelled(cancelled)?;
        match receive_control(websocket, deadline) {
            Err(RenderError::WebSocket(tungstenite::Error::Io(e)))
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dns_resolution_obeys_cancellation_deadline_and_local_address() {
        let started = Instant::now();
        assert!(resolve(
            "example.invalid",
            443,
            started + Duration::from_secs(10),
            &|| true
        )
        .is_err());
        assert!(matches!(
            resolve("example.invalid", 443, started, &|| false),
            Err(RenderError::Timeout)
        ));
        let addresses = resolve(
            "127.0.0.1",
            443,
            Instant::now() + Duration::from_secs(2),
            &|| false,
        )
        .unwrap();
        assert_eq!(
            addresses,
            vec!["127.0.0.1:443".parse::<SocketAddr>().unwrap()]
        );
    }
    #[test]
    fn registration_message_is_stable_and_hex_is_strict() {
        let bytes = registration_message("123456789", &"ab".repeat(32), &"cd".repeat(32), true);
        assert!(bytes.starts_with(b"SENSOR/Render/register/v2"));
        assert_eq!(decode_hex::<2>("aabb").unwrap(), [0xaa, 0xbb]);
        assert!(decode_hex::<2>("aabbcc").is_err());
        assert!(decode_hex::<2>("a€").is_err());
        assert!(websocket_url("http://example.com").is_err());
        assert!(websocket_url("https://user:password@example.com").is_err());
        assert!(websocket_url("http://127.0.0.1:1234").is_ok());
    }
}
