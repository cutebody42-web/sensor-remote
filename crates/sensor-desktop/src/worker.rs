use sensor_audit::AuditLog;
use sensor_client::{LocalInteraction, Message, Mode};
use sensor_files::{Receiver as FileReceiver, TransferId};
use sensor_identity::DeviceIdentity;
use sensor_session::ExpectedPeer;
use sensor_transport::connection::{ConnectionAbort, SecureConnection, DEFAULT_TIMEOUT};
use std::{
    io,
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub enum Event {
    Status(String),
    Consent(ExpectedPeer, Mode, SyncSender<bool>),
    Chat(String),
    Compose(SyncSender<Option<String>>),
    Progress {
        id: Option<TransferId>,
        bytes: u64,
        total: u64,
    },
    Finished(Result<String, String>),
}

pub enum Task {
    Host {
        address: SocketAddr,
        receive_dir: PathBuf,
    },
    Chat {
        address: SocketAddr,
    },
    Send {
        address: SocketAddr,
        file: PathBuf,
        resume: Option<TransferId>,
    },
}

#[derive(Clone, Default)]
pub struct Control {
    cancelled: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<ConnectionAbort>>>,
}
impl Control {
    pub fn stop(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(socket) = self.socket.lock() {
            if let Some(socket) = socket.as_ref() {
                socket.abort();
            }
        }
    }
    pub fn is_stopped(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    fn socket(&self, stream: &TcpStream) -> Result<(), String> {
        let abort = ConnectionAbort::from_stream(stream).map_err(|e| e.to_string())?;
        let mut socket = self
            .socket
            .lock()
            .map_err(|_| "Session control unavailable")?;
        if self.is_stopped() {
            abort.abort();
            return Err("Stopped locally".into());
        }
        *socket = Some(abort);
        Ok(())
    }
}

pub struct Job {
    pub events: Receiver<Event>,
    pub control: Control,
    thread: Option<thread::JoinHandle<()>>,
}
impl Job {
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.control.stop();
        // Never block the window message loop on network I/O.
        if self.is_finished() {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

pub fn start(task: Task, identity: DeviceIdentity, peer: ExpectedPeer, config: PathBuf) -> Job {
    let (events, receive) = mpsc::sync_channel(64);
    let control = Control::default();
    let worker_control = control.clone();
    let worker = thread::spawn(move || {
        let result = execute(task, identity, peer, config, &events, &worker_control);
        let _ = events.send(Event::Finished(result));
    });
    Job {
        events: receive,
        control,
        thread: Some(worker),
    }
}

fn wait_reply<T>(receiver: Receiver<T>, control: &Control) -> Option<T> {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !control.is_stopped() && Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(value) => return Some(value),
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
    }
    None
}

struct Interaction<'a> {
    events: &'a SyncSender<Event>,
    control: &'a Control,
    progress_at: Instant,
}
impl LocalInteraction for Interaction<'_> {
    fn accept(&mut self, peer: ExpectedPeer, mode: Mode) -> bool {
        let (sender, receiver) = mpsc::sync_channel(1);
        let accepted = self.events.send(Event::Consent(peer, mode, sender)).is_ok()
            && wait_reply(receiver, self.control).unwrap_or(false);
        let _ = self.events.send(Event::Status(if accepted {
            format!(
                "Active {mode:?} session • peer {} • Direct TCP • ChaCha20-Poly1305",
                peer.device_id
            )
        } else {
            "Session rejected locally.".into()
        }));
        accepted
    }
    fn chat_reply(&mut self, text: &str) -> Option<String> {
        self.events.send(Event::Chat(text.into())).ok()?;
        compose(self.events, self.control)
    }
    fn transfer_progress(&mut self, bytes: u64, total: u64) {
        if bytes == total || self.progress_at.elapsed() >= Duration::from_millis(100) {
            let _ = self.events.send(Event::Progress {
                id: None,
                bytes,
                total,
            });
            self.progress_at = Instant::now();
        }
    }
}
fn compose(events: &SyncSender<Event>, control: &Control) -> Option<String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    events.send(Event::Compose(sender)).ok()?;
    wait_reply(receiver, control).flatten()
}

fn execute(
    task: Task,
    identity: DeviceIdentity,
    peer: ExpectedPeer,
    config: PathBuf,
    events: &SyncSender<Event>,
    control: &Control,
) -> Result<String, String> {
    let status = |text: String| {
        events
            .send(Event::Status(text))
            .map_err(|_| "Window closed".to_owned())
    };
    if let Task::Host {
        address,
        ref receive_dir,
    } = task
    {
        let receiver = FileReceiver::open(receive_dir).map_err(|e| e.to_string())?;
        let mut log = AuditLog::open(&config.join("audit.jsonl"), identity.keypair())
            .map_err(|e| e.to_string())?;
        let listener = TcpListener::bind(address).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        status(format!(
            "Listening on {} • permitted peer {}",
            listener.local_addr().map_err(|e| e.to_string())?,
            peer.device_id
        ))?;
        while !control.is_stopped() {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
                    control.socket(&stream)?;
                    let mut connection =
                        match SecureConnection::accept(stream, &identity, peer, DEFAULT_TIMEOUT) {
                            Ok(connection) => connection,
                            Err(_) if !control.is_stopped() => {
                                status("Unverified connection refused. Still listening.".into())?;
                                continue;
                            }
                            Err(e) => return Err(e.to_string()),
                        };
                    connection
                        .set_timeout(Duration::from_secs(120))
                        .map_err(|e| e.to_string())?;
                    status(format!(
                        "Authenticated peer {} • Direct TCP • Awaiting local consent",
                        peer.device_id
                    ))?;
                    let mut interaction = Interaction {
                        events,
                        control,
                        progress_at: Instant::now(),
                    };
                    sensor_client::serve(
                        connection,
                        identity.device_id(),
                        &receiver,
                        &mut log,
                        &mut interaction,
                    )
                    .map_err(|e| e.to_string())?;
                    return Ok(format!(
                        "Session ended. {} signed audit records.",
                        log.head().records
                    ));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(30))
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        return Ok("Listener stopped. No incoming connections accepted.".into());
    }
    let address = match task {
        Task::Chat { address } | Task::Send { address, .. } => address,
        _ => unreachable!(),
    };
    status(format!("Connecting to {address}..."))?;
    let stream =
        TcpStream::connect_timeout(&address, DEFAULT_TIMEOUT).map_err(|e| e.to_string())?;
    control.socket(&stream)?;
    let mut connection = SecureConnection::initiate(stream, &identity, peer, DEFAULT_TIMEOUT)
        .map_err(|e| e.to_string())?;
    connection
        .set_timeout(Duration::from_secs(120))
        .map_err(|e| e.to_string())?;
    status(format!(
        "Authenticated {} • Direct TCP • Waiting for acceptance",
        peer.device_id
    ))?;
    if let Task::Send { file, resume, .. } = task {
        let mut updated = Instant::now() - Duration::from_secs(1);
        let manifest =
            sensor_client::send_file(&mut connection, &file, resume, |id, bytes, total| {
                if bytes == total || updated.elapsed() >= Duration::from_millis(100) {
                    let _ = events.send(Event::Progress {
                        id: Some(id),
                        bytes,
                        total,
                    });
                    updated = Instant::now();
                }
            })
            .map_err(|e| e.to_string())?;
        return Ok(format!(
            "Delivered {} bytes. Receiver verified SHA-256: {}",
            manifest.size,
            crate::hex(&manifest.sha256)
        ));
    }
    sensor_client::request(&mut connection, Mode::Chat).map_err(|e| e.to_string())?;
    status(format!(
        "Chat accepted • peer {} • Direct TCP • ChaCha20-Poly1305",
        peer.device_id
    ))?;
    while let Some(text) = compose(events, control) {
        connection
            .send(&Message::Chat(text))
            .map_err(|e| e.to_string())?;
        match connection.receive::<Message>().map_err(|e| e.to_string())? {
            Message::Chat(text) if text.len() <= sensor_client::MAX_CHAT_BYTES => {
                events
                    .send(Event::Chat(text))
                    .map_err(|_| "Window closed")?;
            }
            Message::Close => return Ok("Peer ended the chat.".into()),
            _ => return Err("Invalid chat response.".into()),
        }
    }
    let _ = connection.send(&Message::Close);
    Ok("Chat closed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelling_a_listener_releases_the_socket_and_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let identity = DeviceIdentity::generate();
        let peer = DeviceIdentity::generate();
        let job = start(
            Task::Host {
                address: "127.0.0.1:0".parse().unwrap(),
                receive_dir: dir.path().into(),
            },
            identity,
            ExpectedPeer {
                device_id: peer.device_id(),
                public_key: peer.keypair().public_key(),
            },
            dir.path().into(),
        );
        let Event::Status(status) = job.events.recv_timeout(Duration::from_secs(5)).unwrap() else {
            panic!("listener did not start")
        };
        assert!(status.starts_with("Listening on"));
        job.control.stop();
        assert!(matches!(
            job.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Finished(Ok(_))
        ));
    }
    #[test]
    fn closing_consent_channel_fails_closed() {
        let (send, receive) = mpsc::sync_channel::<bool>(1);
        drop(send);
        assert!(!wait_reply(receive, &Control::default()).unwrap_or(false));
    }
}
