//! Explicit, bounded two-endpoint relay. Session payload stays endpoint-encrypted.
//! Each listener instance is provisioned for one pair and one connection. Regional
//! routing, concurrent rooms, TLS metadata protection and quotas belong to later services.
use rand_core::{OsRng, RngCore};
use sensor_crypto::{CryptoError, IdentityKeypair};
use std::{
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

const CHALLENGE_DOMAIN: &[u8] = b"SENSOR/relay/challenge/v1";
const RESPONSE_DOMAIN: &[u8] = b"SENSOR/relay/join/v1";
const MAGIC: [u8; 4] = *b"SRL1";

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("relay I/O: {0}")]
    Io(#[from] io::Error),
    #[error("relay authentication: {0}")]
    Crypto(#[from] CryptoError),
    #[error("relay identity or endpoint is not authorized")]
    Unauthorized,
    #[error("relay pairing deadline exceeded")]
    Timeout,
    #[error("relay forwarding thread failed")]
    Worker,
}

fn deadline_left(deadline: Instant) -> Result<Duration, RelayError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or(RelayError::Timeout)
}

fn receive(
    stream: &mut TcpStream,
    mut buffer: &mut [u8],
    deadline: Instant,
) -> Result<(), RelayError> {
    while !buffer.is_empty() {
        stream.set_read_timeout(Some(deadline_left(deadline)?))?;
        match stream.read(buffer) {
            Ok(0) => {
                return Err(
                    io::Error::new(io::ErrorKind::UnexpectedEof, "relay peer closed").into(),
                )
            }
            Ok(count) => buffer = &mut buffer[count..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn challenge_bytes(nonce: &[u8; 32]) -> Vec<u8> {
    let mut bytes = CHALLENGE_DOMAIN.to_vec();
    bytes.extend_from_slice(nonce);
    bytes
}
fn response_bytes(challenge: &[u8], key: &[u8; 32]) -> Vec<u8> {
    let mut bytes = RESPONSE_DOMAIN.to_vec();
    bytes.extend_from_slice(challenge);
    bytes.extend_from_slice(key);
    bytes
}

/// Connect to a relay whose public key was checked out of band. Returns an
/// ordinary socket for the existing endpoint-to-endpoint handshake.
pub fn join(
    address: SocketAddr,
    local: &IdentityKeypair,
    relay_key: &[u8; 32],
    timeout: Duration,
) -> Result<TcpStream, RelayError> {
    let deadline = Instant::now() + timeout;
    let mut stream = TcpStream::connect_timeout(&address, deadline_left(deadline)?)?;
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(deadline_left(deadline)?))?;
    let mut challenge = [0; 132];
    receive(&mut stream, &mut challenge, deadline)?;
    if challenge[..4] != MAGIC || &challenge[36..68] != relay_key {
        return Err(RelayError::Unauthorized);
    }
    let nonce: [u8; 32] = challenge[4..36]
        .try_into()
        .map_err(|_| RelayError::Unauthorized)?;
    IdentityKeypair::verify(relay_key, &challenge_bytes(&nonce), &challenge[68..])?;
    let public_key = local.public_key();
    let signature = local.sign(&response_bytes(&challenge, &public_key));
    let mut response = public_key.to_vec();
    response.extend_from_slice(&signature);
    stream.write_all(&response)?;
    let mut ready = [0; 1];
    receive(&mut stream, &mut ready, deadline)?;
    if ready != [1] {
        return Err(RelayError::Unauthorized);
    }
    Ok(stream)
}

fn authenticate(
    mut stream: TcpStream,
    relay: &IdentityKeypair,
    allowed: &[[u8; 32]; 2],
    deadline: Instant,
) -> Result<(usize, TcpStream), RelayError> {
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(deadline_left(deadline)?))?;
    let mut nonce = [0; 32];
    OsRng.fill_bytes(&mut nonce);
    let mut challenge = MAGIC.to_vec();
    challenge.extend_from_slice(&nonce);
    challenge.extend_from_slice(&relay.public_key());
    challenge.extend_from_slice(&relay.sign(&challenge_bytes(&nonce)));
    stream.write_all(&challenge)?;
    let mut response = [0; 96];
    receive(&mut stream, &mut response, deadline)?;
    let public_key: [u8; 32] = response[..32]
        .try_into()
        .map_err(|_| RelayError::Unauthorized)?;
    let role = allowed
        .iter()
        .position(|key| key == &public_key)
        .ok_or(RelayError::Unauthorized)?;
    IdentityKeypair::verify(
        &public_key,
        &response_bytes(&challenge, &public_key),
        &response[32..],
    )?;
    Ok((role, stream))
}

#[derive(Clone, Copy, Debug)]
pub struct RelayStats {
    pub left_to_right: u64,
    pub right_to_left: u64,
}

/// Serve exactly one preauthorized pair. Fixed memory, bounded pairing time and
/// a configured idle timeout; no remote-controlled destination socket is opened.
pub fn serve_pair(
    listener: TcpListener,
    relay: &IdentityKeypair,
    allowed: [[u8; 32]; 2],
    pairing_timeout: Duration,
    idle_timeout: Duration,
) -> Result<RelayStats, RelayError> {
    if allowed[0] == allowed[1] || pairing_timeout.is_zero() || idle_timeout.is_zero() {
        return Err(RelayError::Unauthorized);
    }
    let deadline = Instant::now() + pairing_timeout;
    listener.set_nonblocking(true)?;
    let mut streams: [Option<TcpStream>; 2] = [None, None];
    let mut rejected = 0;
    while streams.iter().any(Option::is_none) {
        deadline_left(deadline)?;
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                let authentication_deadline =
                    (Instant::now() + Duration::from_secs(5)).min(deadline);
                match authenticate(stream, relay, &allowed, authentication_deadline) {
                    Ok((role, stream)) if streams[role].is_none() => streams[role] = Some(stream),
                    _ => {
                        rejected += 1;
                        if rejected >= 16 {
                            return Err(RelayError::Unauthorized);
                        }
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(e.into()),
        }
    }
    let mut left = streams[0].take().ok_or(RelayError::Unauthorized)?;
    let mut right = streams[1].take().ok_or(RelayError::Unauthorized)?;
    for stream in [&mut left, &mut right] {
        stream.set_read_timeout(Some(idle_timeout))?;
        stream.set_write_timeout(Some(idle_timeout))?;
        stream.write_all(&[1])?;
    }
    let mut left_reader = left.try_clone()?;
    let mut right_writer = right.try_clone()?;
    let worker = thread::spawn(move || {
        let result = io::copy(&mut left_reader, &mut right_writer);
        if result.is_ok() {
            // Preserve pending traffic in the reverse direction after a clean EOF.
            let _ = right_writer.shutdown(Shutdown::Write);
        } else {
            let _ = left_reader.shutdown(Shutdown::Both);
            let _ = right_writer.shutdown(Shutdown::Both);
        }
        result
    });
    let backward = io::copy(&mut right, &mut left);
    if backward.is_ok() {
        let _ = left.shutdown(Shutdown::Write);
    } else {
        let _ = left.shutdown(Shutdown::Both);
        let _ = right.shutdown(Shutdown::Both);
    }
    let forward = worker.join().map_err(|_| RelayError::Worker)?;
    // Closing either endpoint terminates the pair. A shutdown race may surface
    // as an OS error; report it rather than fabricating forwarded-byte counts.
    Ok(RelayStats {
        left_to_right: forward?,
        right_to_left: backward?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sensor_identity::DeviceIdentity;
    use sensor_session::ExpectedPeer;
    use sensor_transport::connection::{SecureConnection, DEFAULT_TIMEOUT};
    #[test]
    fn relay_forwards_a_real_pinned_end_to_end_session() {
        let left = DeviceIdentity::generate();
        let right = DeviceIdentity::generate();
        let relay = IdentityKeypair::generate();
        let left_pin = ExpectedPeer {
            device_id: left.device_id(),
            public_key: left.keypair().public_key(),
        };
        let right_pin = ExpectedPeer {
            device_id: right.device_id(),
            public_key: right.keypair().public_key(),
        };
        let relay_key = relay.public_key();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            serve_pair(
                listener,
                &relay,
                [left_pin.public_key, right_pin.public_key],
                DEFAULT_TIMEOUT,
                DEFAULT_TIMEOUT,
            )
        });
        let host = thread::spawn(move || {
            let stream = join(address, right.keypair(), &relay_key, DEFAULT_TIMEOUT).unwrap();
            let mut connection =
                SecureConnection::accept(stream, &right, left_pin, DEFAULT_TIMEOUT).unwrap();
            assert_eq!(connection.receive::<String>().unwrap(), "through relay");
            connection.send(&"confirmed").unwrap();
            assert_eq!(connection.receive::<String>().unwrap(), "closing");
        });
        let stream = join(address, left.keypair(), &relay_key, DEFAULT_TIMEOUT).unwrap();
        let mut connection =
            SecureConnection::initiate(stream, &left, right_pin, DEFAULT_TIMEOUT).unwrap();
        connection.send(&"through relay").unwrap();
        assert_eq!(connection.receive::<String>().unwrap(), "confirmed");
        connection.send(&"closing").unwrap();
        host.join().unwrap();
        connection.close();
        let stats = server.join().unwrap().unwrap();
        assert!(stats.left_to_right > 0 && stats.right_to_left > 0);
    }
    #[test]
    fn wrong_relay_key_is_rejected() {
        let relay = IdentityKeypair::generate();
        let user = IdentityKeypair::generate();
        let allowed = [user.public_key(), IdentityKeypair::generate().public_key()];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let stream = listener.accept().unwrap().0;
            authenticate(stream, &relay, &allowed, Instant::now() + DEFAULT_TIMEOUT).is_err()
        });
        assert!(matches!(
            join(address, &user, &[0; 32], DEFAULT_TIMEOUT),
            Err(RelayError::Unauthorized)
        ));
        assert!(worker.join().unwrap());
    }
}
