//! Ordered TCP transport with pinned endpoint authentication and mutual key confirmation.
//! This module makes no claim of TLS, NAT traversal, relay selection, or authorization.
use sensor_crypto::EncryptedRecord;
use sensor_identity::DeviceIdentity;
use sensor_protocol::{
    ClientHello, Frame, MessageType, ProtocolError, MAX_FRAME_SIZE, PROTOCOL_VERSION,
};
use sensor_session::{
    accept_hello, accept_hello_unpinned, finish_initiator, finish_responder, start_initiator,
    ExpectedPeer, SecureSession, SessionError,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use thiserror::Error;

const HANDSHAKE_LIMIT: usize = 4096;
const AAD: &[u8] = b"SENSOR/ordered-record/v2";
const CLIENT_FINISH: &[u8] = b"SENSOR/client-finished/v2";
const SERVER_FINISH: &[u8] = b"SENSOR/server-finished/v2";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("network I/O: {0}")]
    Io(#[from] io::Error),
    #[error("protocol: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("session: {0}")]
    Session(#[from] SessionError),
    #[error("unexpected handshake or record message")]
    UnexpectedMessage,
    #[error("connection is closed after an earlier failure")]
    Closed,
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "SENSOR record deadline exceeded"))
}

fn read_exact(stream: &mut TcpStream, mut buffer: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !buffer.is_empty() {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        match stream.read(buffer) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "peer closed connection",
                ))
            }
            Ok(count) => {
                buffer = &mut buffer[count..];
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn write_all(stream: &mut TcpStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining(deadline)?))?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "peer cannot receive",
                ))
            }
            Ok(count) => bytes = &bytes[count..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn read_frame(
    stream: &mut TcpStream,
    limit: usize,
    deadline: Instant,
) -> Result<Frame, ConnectionError> {
    let mut prefix = [0; 4];
    read_exact(stream, &mut prefix, deadline)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length > limit {
        return Err(ProtocolError::FrameTooLarge.into());
    }
    // Length is checked before allocation, even for the unauthenticated handshake.
    let mut bytes = vec![0; length];
    read_exact(stream, &mut bytes, deadline)?;
    Ok(Frame::decode(&bytes)?)
}

fn write_frame(
    stream: &mut TcpStream,
    frame: &Frame,
    deadline: Instant,
) -> Result<(), ConnectionError> {
    let bytes = frame.encode()?;
    write_all(stream, &(bytes.len() as u32).to_be_bytes(), deadline)?;
    write_all(stream, &bytes, deadline)?;
    Ok(())
}

/// Owns one stream and its record keys. Application messages are unavailable
/// until both peers have checked the encrypted handshake confirmation.
pub struct SecureConnection {
    stream: TcpStream,
    session: Option<SecureSession>,
    peer: ExpectedPeer,
    transcript: [u8; 32],
    timeout: Duration,
    closed: bool,
}

/// A local UI can terminate blocking I/O without accessing session keys.
pub struct ConnectionAbort(TcpStream);
impl ConnectionAbort {
    pub fn from_stream(stream: &TcpStream) -> io::Result<Self> {
        Ok(Self(stream.try_clone()?))
    }
    pub fn abort(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

impl SecureConnection {
    pub fn abort_handle(&self) -> io::Result<ConnectionAbort> {
        ConnectionAbort::from_stream(&self.stream)
    }
    pub fn connect(
        address: SocketAddr,
        local: &DeviceIdentity,
        expected: ExpectedPeer,
        timeout: Duration,
    ) -> Result<Self, ConnectionError> {
        let deadline = Instant::now() + timeout;
        let stream = TcpStream::connect_timeout(&address, remaining(deadline)?)?;
        Self::initiate(stream, local, expected, remaining(deadline)?)
    }

    pub fn initiate(
        mut stream: TcpStream,
        local: &DeviceIdentity,
        expected: ExpectedPeer,
        timeout: Duration,
    ) -> Result<Self, ConnectionError> {
        stream.set_nodelay(true)?;
        let deadline = Instant::now() + timeout;
        let (pending, hello) = start_initiator(local.keypair(), local.device_id(), expected);
        write_frame(
            &mut stream,
            &Frame::new(MessageType::ClientHello, &hello)?,
            deadline,
        )?;
        let response = read_frame(&mut stream, HANDSHAKE_LIMIT, deadline)?;
        if response.message_type != MessageType::ServerHello {
            return Err(ConnectionError::UnexpectedMessage);
        }
        let session = finish_initiator(pending, response.decode_payload()?)?;
        let mut connection = Self {
            stream,
            peer: ExpectedPeer {
                device_id: session.peer_device_id(),
                public_key: session.peer_identity_key(),
            },
            transcript: session.transcript_hash(),
            session: Some(session),
            timeout,
            closed: false,
        };
        connection.send_bytes(CLIENT_FINISH, deadline)?;
        if connection.receive_bytes(deadline)? != SERVER_FINISH {
            return Err(ConnectionError::UnexpectedMessage);
        }
        Ok(connection)
    }

    pub fn accept(
        mut stream: TcpStream,
        local: &DeviceIdentity,
        expected: ExpectedPeer,
        timeout: Duration,
    ) -> Result<Self, ConnectionError> {
        stream.set_nodelay(true)?;
        let deadline = Instant::now() + timeout;
        let request = read_frame(&mut stream, HANDSHAKE_LIMIT, deadline)?;
        if request.message_type != MessageType::ClientHello {
            return Err(ConnectionError::UnexpectedMessage);
        }
        let hello: ClientHello = request.decode_payload()?;
        let (pending, response) =
            accept_hello(local.keypair(), local.device_id(), hello, expected)?;
        write_frame(
            &mut stream,
            &Frame::new(MessageType::ServerHello, &response)?,
            deadline,
        )?;
        let session = finish_responder(pending)?;
        let mut connection = Self {
            stream,
            peer: ExpectedPeer {
                device_id: session.peer_device_id(),
                public_key: session.peer_identity_key(),
            },
            transcript: session.transcript_hash(),
            session: Some(session),
            timeout,
            closed: false,
        };
        if connection.receive_bytes(deadline)? != CLIENT_FINISH {
            return Err(ConnectionError::UnexpectedMessage);
        }
        connection.send_bytes(SERVER_FINISH, deadline)?;
        Ok(connection)
    }

    /// Accept an explicitly attended first connection without a pre-pinned
    /// caller key. The caller's signed identity is still authenticated and is
    /// exposed via `peer()` before application consent is requested.
    pub fn accept_unpinned(
        mut stream: TcpStream,
        local: &DeviceIdentity,
        timeout: Duration,
    ) -> Result<Self, ConnectionError> {
        stream.set_nodelay(true)?;
        let deadline = Instant::now() + timeout;
        let request = read_frame(&mut stream, HANDSHAKE_LIMIT, deadline)?;
        if request.message_type != MessageType::ClientHello {
            return Err(ConnectionError::UnexpectedMessage);
        }
        let hello: ClientHello = request.decode_payload()?;
        let (pending, response) = accept_hello_unpinned(local.keypair(), local.device_id(), hello)?;
        write_frame(
            &mut stream,
            &Frame::new(MessageType::ServerHello, &response)?,
            deadline,
        )?;
        let session = finish_responder(pending)?;
        let mut connection = Self {
            stream,
            peer: ExpectedPeer {
                device_id: session.peer_device_id(),
                public_key: session.peer_identity_key(),
            },
            transcript: session.transcript_hash(),
            session: Some(session),
            timeout,
            closed: false,
        };
        if connection.receive_bytes(deadline)? != CLIENT_FINISH {
            return Err(ConnectionError::UnexpectedMessage);
        }
        connection.send_bytes(SERVER_FINISH, deadline)?;
        Ok(connection)
    }

    pub fn peer(&self) -> ExpectedPeer {
        self.peer
    }

    pub fn transcript_hash(&self) -> [u8; 32] {
        self.transcript
    }

    /// Deadline for each subsequent record, including local attended interaction.
    pub fn set_timeout(&mut self, timeout: Duration) -> Result<(), ConnectionError> {
        if timeout.is_zero() || timeout > Duration::from_secs(600) {
            return Err(ConnectionError::UnexpectedMessage);
        }
        self.timeout = timeout;
        Ok(())
    }

    pub fn send<T: Serialize>(&mut self, message: &T) -> Result<(), ConnectionError> {
        let frame = Frame::new(MessageType::Control, message)?;
        self.send_bytes(&frame.encode()?, Instant::now() + self.timeout)
    }

    pub fn receive<T: DeserializeOwned>(&mut self) -> Result<T, ConnectionError> {
        let result = self
            .receive_bytes(Instant::now() + self.timeout)
            .and_then(|bytes| {
                let frame = Frame::decode(&bytes)?;
                if frame.message_type != MessageType::Control {
                    return Err(ConnectionError::UnexpectedMessage);
                }
                Ok(frame.decode_payload()?)
            });
        if result.is_err() {
            self.close();
        }
        result
    }

    fn send_bytes(&mut self, plaintext: &[u8], deadline: Instant) -> Result<(), ConnectionError> {
        if self.closed {
            return Err(ConnectionError::Closed);
        }
        if plaintext.len() > MAX_FRAME_SIZE - 64 {
            return Err(ProtocolError::FrameTooLarge.into());
        }
        let result = (|| {
            let record = self
                .session
                .as_mut()
                .ok_or(ConnectionError::Closed)?
                .seal(AAD, plaintext)?;
            let mut payload = record.sequence.to_be_bytes().to_vec();
            payload.extend_from_slice(&record.ciphertext);
            write_frame(
                &mut self.stream,
                &Frame {
                    version: PROTOCOL_VERSION,
                    message_type: MessageType::Control,
                    payload,
                },
                deadline,
            )
        })();
        if result.is_err() {
            self.close();
        }
        result
    }

    fn receive_bytes(&mut self, deadline: Instant) -> Result<Vec<u8>, ConnectionError> {
        if self.closed {
            return Err(ConnectionError::Closed);
        }
        let result = (|| {
            let frame = read_frame(&mut self.stream, MAX_FRAME_SIZE, deadline)?;
            if frame.message_type != MessageType::Control || frame.payload.len() < 24 {
                return Err(ConnectionError::UnexpectedMessage);
            }
            let sequence = u64::from_be_bytes(
                frame.payload[..8]
                    .try_into()
                    .map_err(|_| ConnectionError::UnexpectedMessage)?,
            );
            Ok(self.session.as_mut().ok_or(ConnectionError::Closed)?.open(
                &EncryptedRecord {
                    sequence,
                    ciphertext: frame.payload[8..].to_vec(),
                },
                AAD,
            )?)
        })();
        if result.is_err() {
            self.close();
        }
        result
    }

    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _ = self.stream.shutdown(Shutdown::Both);
    }

    /// Transfers the authenticated record state to exactly one reader and writer.
    /// The crypto lock is never held during socket I/O. Dropping either half aborts both.
    pub fn split(&mut self) -> Result<(SecureReader, SecureWriter), ConnectionError> {
        if self.closed {
            return Err(ConnectionError::Closed);
        }
        let read = self.stream.try_clone()?;
        let write = self.stream.try_clone()?;
        let state = Arc::new(DuplexState {
            session: Mutex::new(self.session.take().ok_or(ConnectionError::Closed)?),
            failed: AtomicBool::new(false),
        });
        self.closed = true;
        Ok((
            SecureReader {
                stream: read,
                state: state.clone(),
                timeout: self.timeout,
            },
            SecureWriter {
                stream: write,
                state,
                timeout: self.timeout,
            },
        ))
    }
}

struct DuplexState {
    session: Mutex<SecureSession>,
    failed: AtomicBool,
}
pub struct SecureReader {
    stream: TcpStream,
    state: Arc<DuplexState>,
    timeout: Duration,
}
pub struct SecureWriter {
    stream: TcpStream,
    state: Arc<DuplexState>,
    timeout: Duration,
}
fn fail_duplex(state: &DuplexState, stream: &TcpStream) {
    state.failed.store(true, Ordering::SeqCst);
    let _ = stream.shutdown(Shutdown::Both);
}
impl SecureReader {
    pub fn receive<T: DeserializeOwned>(&mut self) -> Result<T, ConnectionError> {
        let result = (|| {
            if self.state.failed.load(Ordering::SeqCst) {
                return Err(ConnectionError::Closed);
            }
            let frame = read_frame(
                &mut self.stream,
                MAX_FRAME_SIZE,
                Instant::now() + self.timeout,
            )?;
            if frame.message_type != MessageType::Control || frame.payload.len() < 24 {
                return Err(ConnectionError::UnexpectedMessage);
            }
            let record = EncryptedRecord {
                sequence: u64::from_be_bytes(
                    frame.payload[..8]
                        .try_into()
                        .map_err(|_| ConnectionError::UnexpectedMessage)?,
                ),
                ciphertext: frame.payload[8..].to_vec(),
            };
            let plaintext = self
                .state
                .session
                .lock()
                .map_err(|_| ConnectionError::Closed)?
                .open(&record, AAD)?;
            let frame = Frame::decode(&plaintext)?;
            if frame.message_type != MessageType::Control {
                return Err(ConnectionError::UnexpectedMessage);
            }
            Ok(frame.decode_payload()?)
        })();
        if result.is_err() {
            fail_duplex(&self.state, &self.stream);
        }
        result
    }
}
impl SecureWriter {
    /// Independent record-write deadline; long idle read deadlines must not
    /// allow interactive video to block a writer for minutes.
    pub fn set_timeout(&mut self, timeout: Duration) -> Result<(), ConnectionError> {
        if timeout.is_zero() || timeout > Duration::from_secs(600) {
            return Err(ConnectionError::UnexpectedMessage);
        }
        self.timeout = timeout;
        Ok(())
    }
    pub fn send<T: Serialize>(&mut self, message: &T) -> Result<(), ConnectionError> {
        let result = (|| {
            if self.state.failed.load(Ordering::SeqCst) {
                return Err(ConnectionError::Closed);
            }
            let plaintext = Frame::new(MessageType::Control, message)?.encode()?;
            if plaintext.len() > MAX_FRAME_SIZE - 64 {
                return Err(ProtocolError::FrameTooLarge.into());
            }
            let record = self
                .state
                .session
                .lock()
                .map_err(|_| ConnectionError::Closed)?
                .seal(AAD, &plaintext)?;
            let mut payload = record.sequence.to_be_bytes().to_vec();
            payload.extend_from_slice(&record.ciphertext);
            write_frame(
                &mut self.stream,
                &Frame {
                    version: PROTOCOL_VERSION,
                    message_type: MessageType::Control,
                    payload,
                },
                Instant::now() + self.timeout,
            )
        })();
        if result.is_err() {
            fail_duplex(&self.state, &self.stream);
        }
        result
    }
}
impl Drop for SecureReader {
    fn drop(&mut self) {
        fail_duplex(&self.state, &self.stream);
    }
}
impl Drop for SecureWriter {
    fn drop(&mut self) {
        fail_duplex(&self.state, &self.stream);
    }
}

impl Drop for SecureConnection {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn expected(identity: &DeviceIdentity) -> ExpectedPeer {
        ExpectedPeer {
            device_id: identity.device_id(),
            public_key: identity.keypair().public_key(),
        }
    }

    #[test]
    fn real_tcp_peers_confirm_keys_and_exchange_encrypted_messages() {
        let local = DeviceIdentity::generate();
        let remote = DeviceIdentity::generate();
        let local_pin = expected(&local);
        let remote_pin = expected(&remote);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            let mut connection = SecureConnection::accept(
                listener.accept().unwrap().0,
                &remote,
                local_pin,
                DEFAULT_TIMEOUT,
            )
            .unwrap();
            let message: Vec<u8> = connection.receive().unwrap();
            assert_eq!(message, vec![17; 131_072]);
            connection.send(&"received").unwrap();
            connection.transcript_hash()
        });
        let mut connection =
            SecureConnection::connect(address, &local, remote_pin, DEFAULT_TIMEOUT).unwrap();
        connection.send(&vec![17u8; 131_072]).unwrap();
        assert_eq!(connection.receive::<String>().unwrap(), "received");
        assert_eq!(connection.transcript_hash(), host.join().unwrap());
    }

    #[test]
    fn self_signed_impostor_is_rejected_before_application_data() {
        let local = DeviceIdentity::generate();
        let remote = DeviceIdentity::generate();
        let impostor = DeviceIdentity::generate();
        let local_pin = expected(&local);
        let remote_pin = expected(&remote);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            SecureConnection::accept(
                listener.accept().unwrap().0,
                &impostor,
                local_pin,
                DEFAULT_TIMEOUT,
            )
            .is_err()
        });
        assert!(SecureConnection::connect(address, &local, remote_pin, DEFAULT_TIMEOUT).is_err());
        assert!(host.join().unwrap());
    }

    #[test]
    fn unauthenticated_oversized_length_is_rejected_immediately() {
        let local = DeviceIdentity::generate();
        let remote = DeviceIdentity::generate();
        let local_pin = expected(&local);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            matches!(
                SecureConnection::accept(
                    listener.accept().unwrap().0,
                    &remote,
                    local_pin,
                    DEFAULT_TIMEOUT
                ),
                Err(ConnectionError::Protocol(ProtocolError::FrameTooLarge))
            )
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(&u32::MAX.to_be_bytes()).unwrap();
        assert!(host.join().unwrap());
    }

    #[test]
    fn split_reader_and_writer_keep_the_authenticated_stream_full_duplex() {
        let local = DeviceIdentity::generate();
        let remote = DeviceIdentity::generate();
        let local_pin = expected(&local);
        let remote_pin = expected(&remote);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            let stream = listener.accept().unwrap().0;
            let mut connection =
                SecureConnection::accept(stream, &remote, local_pin, DEFAULT_TIMEOUT).unwrap();
            let (mut reader, mut writer) = connection.split().unwrap();
            let received: String = reader.receive().unwrap();
            assert_eq!(received, "client-to-host");
            writer.send(&"host-to-client").unwrap();
        });
        let mut connection =
            SecureConnection::connect(address, &local, remote_pin, DEFAULT_TIMEOUT).unwrap();
        let (mut reader, mut writer) = connection.split().unwrap();
        writer.send(&"client-to-host").unwrap();
        let received: String = reader.receive().unwrap();
        assert_eq!(received, "host-to-client");
        host.join().unwrap();
    }
}
