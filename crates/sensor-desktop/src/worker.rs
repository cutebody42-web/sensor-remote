use sensor_audit::AuditLog;
use sensor_client::{LocalInteraction, Message, Mode};
use sensor_files::{Receiver as FileReceiver, TransferId};
use sensor_identity::DeviceIdentity;
use sensor_media::{DecodedFrame, DesktopMessage, Display, VideoFormat};
use sensor_relay::join as join_relay;
use sensor_session::ExpectedPeer;
use sensor_transport::connection::{ConnectionAbort, SecureConnection, DEFAULT_TIMEOUT};
use std::{
    io,
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use sensor_session::permissions::{Consent, Permissions};
#[cfg(windows)]
use sensor_windows::{codec, desktop, input};

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
    RemoteDisplays(Vec<Display>),
    RemoteFormat(VideoFormat),
    RemoteCursor {
        x: i32,
        y: i32,
        visible: bool,
    },
    Finished(Result<String, String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayRoute {
    pub address: SocketAddr,
    pub relay_key: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Route {
    Direct(SocketAddr),
    Relay(RelayRoute),
}

impl Route {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Direct(_) => "Direct TCP",
            Self::Relay(_) => "Secure relay",
        }
    }
}

pub enum Task {
    Host {
        route: Route,
        receive_dir: PathBuf,
        auto_accept: bool,
        accept_any: bool,
    },
    Chat {
        route: Route,
    },
    Send {
        route: Route,
        file: PathBuf,
        resume: Option<TransferId>,
    },
    Remote {
        route: Route,
        control: bool,
    },
}

#[derive(Clone, Default)]
pub struct Control {
    cancelled: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<ConnectionAbort>>>,
    remote_sender: Arc<Mutex<Option<SyncSender<DesktopMessage>>>>,
    remote_frame: Arc<Mutex<Option<Arc<DecodedFrame>>>>,
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
    pub fn send_remote(&self, message: DesktopMessage) -> bool {
        let sender = self
            .remote_sender
            .lock()
            .ok()
            .and_then(|sender| sender.clone());
        let Some(sender) = sender else {
            return false;
        };
        match sender.try_send(message) {
            Ok(()) => true,
            Err(mpsc::TrySendError::Disconnected(_)) => false,
            Err(mpsc::TrySendError::Full(_)) => {
                // Never leave a key or button logically held when the bounded
                // input path cannot keep up. The worker is stopped so the host
                // drops its Injector and releases any tracked state.
                self.cancelled.store(true, Ordering::SeqCst);
                if let Ok(socket) = self.socket.lock() {
                    if let Some(socket) = socket.as_ref() {
                        socket.abort();
                    }
                }
                false
            }
        }
    }
    pub fn latest_frame(&self) -> Option<Arc<DecodedFrame>> {
        self.remote_frame
            .lock()
            .ok()
            .and_then(|frame| frame.clone())
    }
    fn set_remote_sender(&self, sender: SyncSender<DesktopMessage>) {
        if let Ok(mut current) = self.remote_sender.lock() {
            *current = Some(sender);
        }
    }
    fn clear_remote_sender(&self) {
        if let Ok(mut current) = self.remote_sender.lock() {
            *current = None;
        }
    }
    fn publish_frame(&self, frame: DecodedFrame) {
        if let Ok(mut current) = self.remote_frame.lock() {
            *current = Some(Arc::new(frame));
        }
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
    auto_accept: bool,
}
impl LocalInteraction for Interaction<'_> {
    fn accept(&mut self, peer: ExpectedPeer, mode: Mode) -> bool {
        if self.auto_accept {
            let _ = self.events.send(Event::Status(format!(
                "Auto-accepted {mode:?} session • pinned peer {} • explicit local profile",
                peer.device_id
            )));
            return true;
        }
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

    #[cfg(windows)]
    fn desktop(
        &mut self,
        connection: &mut SecureConnection,
        consent: &sensor_session::permissions::Consent,
    ) -> Result<(), sensor_client::EndpointError> {
        host_desktop(connection, consent, self.control)
    }
}
fn compose(events: &SyncSender<Event>, control: &Control) -> Option<String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    events.send(Event::Compose(sender)).ok()?;
    wait_reply(receiver, control).flatten()
}

fn connect_route(route: &Route, identity: &DeviceIdentity) -> Result<TcpStream, String> {
    match route {
        Route::Direct(address) => {
            TcpStream::connect_timeout(address, DEFAULT_TIMEOUT).map_err(|error| error.to_string())
        }
        Route::Relay(relay) => join_relay(
            relay.address,
            identity.keypair(),
            &relay.relay_key,
            DEFAULT_TIMEOUT,
        )
        .map_err(|error| error.to_string()),
    }
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
        route,
        ref receive_dir,
        auto_accept,
        accept_any,
    } = task
    {
        let receiver = FileReceiver::open(receive_dir).map_err(|e| e.to_string())?;
        let mut log = AuditLog::open(&config.join("audit.jsonl"), identity.keypair())
            .map_err(|e| e.to_string())?;
        let listener = match &route {
            Route::Direct(address) => {
                let listener = TcpListener::bind(address).map_err(|e| e.to_string())?;
                listener.set_nonblocking(true).map_err(|e| e.to_string())?;
                status(format!(
                    "Listening on {} • permitted peer {} • {}",
                    listener.local_addr().map_err(|e| e.to_string())?,
                    peer.device_id,
                    route.label()
                ))?;
                Some(listener)
            }
            Route::Relay(relay) => {
                status(format!(
                    "Waiting at secure relay {} • permitted peer {}",
                    relay.address, peer.device_id
                ))?;
                None
            }
        };
        let mut connection = loop {
            let stream = match &listener {
                Some(listener) => loop {
                    if control.is_stopped() {
                        return Ok("Listener stopped. No incoming connections accepted.".into());
                    }
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream.set_nonblocking(false).map_err(|e| e.to_string())?;
                            break stream;
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(30));
                        }
                        Err(error) => return Err(error.to_string()),
                    }
                },
                None => match &route {
                    Route::Relay(relay) => join_relay(
                        relay.address,
                        identity.keypair(),
                        &relay.relay_key,
                        DEFAULT_TIMEOUT,
                    )
                    .map_err(|error| error.to_string())?,
                    Route::Direct(_) => unreachable!("direct routes always have a listener"),
                },
            };
            control.socket(&stream)?;
            let accepted = if accept_any {
                SecureConnection::accept_unpinned(stream, &identity, DEFAULT_TIMEOUT)
            } else {
                SecureConnection::accept(stream, &identity, peer, DEFAULT_TIMEOUT)
            };
            match accepted {
                Ok(connection) => break connection,
                Err(error) if listener.is_some() && !control.is_stopped() => {
                    status(format!(
                        "Unverified connection refused ({error}). Still listening."
                    ))?;
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        connection
            .set_timeout(Duration::from_secs(120))
            .map_err(|e| e.to_string())?;
        status(format!(
            "Authenticated peer {} • {} • Awaiting local consent",
            peer.device_id,
            route.label()
        ))?;
        let mut interaction = Interaction {
            events,
            control,
            progress_at: Instant::now(),
            auto_accept,
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
    let route = match &task {
        Task::Chat { route } | Task::Send { route, .. } => route.clone(),
        Task::Remote {
            route,
            control: remote_control,
        } => {
            return execute_remote(
                route.clone(),
                *remote_control,
                identity,
                peer,
                events,
                control,
            );
        }
        _ => unreachable!(),
    };
    status(format!("Connecting via {}...", route.label()))?;
    let stream = connect_route(&route, &identity)?;
    control.socket(&stream)?;
    let mut connection = SecureConnection::initiate(stream, &identity, peer, DEFAULT_TIMEOUT)
        .map_err(|e| e.to_string())?;
    connection
        .set_timeout(Duration::from_secs(120))
        .map_err(|e| e.to_string())?;
    status(format!(
        "Authenticated {} • {} • Waiting for acceptance",
        peer.device_id,
        route.label()
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
        "Chat accepted • peer {} • {} • ChaCha20-Poly1305",
        peer.device_id,
        route.label()
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

#[cfg(windows)]
enum HostCommand {
    SelectDisplay(u32),
    Pong(u64),
    Close,
    Failed(String),
}

#[cfg(windows)]
fn desktop_message(
    writer: &mut sensor_transport::connection::SecureWriter,
    message: DesktopMessage,
) -> Result<(), sensor_client::EndpointError> {
    writer.send(&Message::Desktop(message))?;
    Ok(())
}

#[cfg(windows)]
fn spawn_host_reader(
    mut reader: sensor_transport::connection::SecureReader,
    commands: SyncSender<HostCommand>,
    control: Control,
    displays: Arc<Vec<Display>>,
    active: Arc<Mutex<Display>>,
    generation: Arc<AtomicU64>,
    permissions: Permissions,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut injector = input::Injector::default();
        let mut local_consent = Consent::pending(permissions);
        if local_consent.accept(permissions).is_err() {
            let _ = commands.send(HostCommand::Failed(
                "Input permission state was invalid.".into(),
            ));
            return;
        }
        loop {
            if control.is_stopped() {
                break;
            }
            let message = match reader.receive::<Message>() {
                Ok(message) => message,
                Err(error) => {
                    if !control.is_stopped() {
                        let _ = commands.send(HostCommand::Failed(error.to_string()));
                    }
                    break;
                }
            };
            match message {
                Message::Desktop(DesktopMessage::Input {
                    generation: input_generation,
                    event,
                }) => {
                    if input_generation != generation.load(Ordering::SeqCst) {
                        continue;
                    }
                    let display = match active.lock() {
                        Ok(display) => display.clone(),
                        Err(_) => {
                            let _ = commands.send(HostCommand::Failed(
                                "Remote display state became unavailable.".into(),
                            ));
                            break;
                        }
                    };
                    if let Err(error) = injector.apply(&event, &display, &local_consent) {
                        let _ = commands.send(HostCommand::Failed(error.to_string()));
                        break;
                    }
                }
                Message::Desktop(DesktopMessage::SelectDisplay(index)) => {
                    let Some(display) = displays.iter().find(|display| display.index == index)
                    else {
                        let _ =
                            commands.send(HostCommand::Failed("Unknown monitor selected.".into()));
                        break;
                    };
                    if let Err(error) = injector.release() {
                        let _ = commands.send(HostCommand::Failed(error.to_string()));
                        break;
                    }
                    if let Ok(mut active_display) = active.lock() {
                        *active_display = display.clone();
                    } else {
                        let _ = commands.send(HostCommand::Failed(
                            "Remote display state became unavailable.".into(),
                        ));
                        break;
                    }
                    if commands.send(HostCommand::SelectDisplay(index)).is_err() {
                        break;
                    }
                }
                Message::Desktop(DesktopMessage::Ping(value)) => {
                    if commands.send(HostCommand::Pong(value)).is_err() {
                        break;
                    }
                }
                Message::Desktop(DesktopMessage::Close) | Message::Close => {
                    let _ = injector.release();
                    let _ = commands.send(HostCommand::Close);
                    break;
                }
                _ => {
                    let _ = commands.send(HostCommand::Failed(
                        "Unexpected remote desktop command.".into(),
                    ));
                    break;
                }
            }
        }
        let _ = injector.release();
    })
}

#[cfg(windows)]
fn host_desktop(
    connection: &mut SecureConnection,
    consent: &Consent,
    control: &Control,
) -> Result<(), sensor_client::EndpointError> {
    let displays = desktop::displays(consent).map_err(|error| {
        sensor_client::EndpointError::Desktop(format!("Cannot enumerate displays: {error}"))
    })?;
    if displays.is_empty() {
        return Err(sensor_client::EndpointError::Desktop(
            "No attached desktop display is available.".into(),
        ));
    }
    let permissions = consent.granted();
    let selected = 0_u32;
    let mut generation = 1_u64;
    let (reader, mut writer) = connection.split()?;
    desktop_message(&mut writer, DesktopMessage::Displays(displays.clone()))?;

    let active = Arc::new(Mutex::new(displays[0].clone()));
    let generation_shared = Arc::new(AtomicU64::new(generation));
    let (commands, command_receiver) = mpsc::sync_channel(64);
    let reader_thread = spawn_host_reader(
        reader,
        commands,
        control.clone(),
        Arc::new(displays.clone()),
        active.clone(),
        generation_shared.clone(),
        permissions,
    );

    let setup = |index: u32, generation: u64| {
        let capture = desktop::Capture::new(index, consent)
            .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
        let raw_width = if capture.rotation == 90 || capture.rotation == 270 {
            capture.display.height
        } else {
            capture.display.width
        };
        let raw_height = if capture.rotation == 90 || capture.rotation == 270 {
            capture.display.width
        } else {
            capture.display.height
        };
        let output_width = if capture.rotation == 90 || capture.rotation == 270 {
            raw_height
        } else {
            raw_width
        };
        let output_height = if capture.rotation == 90 || capture.rotation == 270 {
            raw_width
        } else {
            raw_height
        };
        let (width, height) = sensor_media::stream_size(output_width, output_height)
            .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
        let encoder = codec::H264Encoder::new(width, height, 30, 4_000_000, true)
            .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
        let format = VideoFormat {
            generation,
            display: capture.display.clone(),
            width,
            height,
            fps_limit: 30,
            bitrate: 4_000_000,
            encoder: encoder.name.clone(),
            hardware: encoder.hardware,
        };
        format
            .validate()
            .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
        Ok::<_, sensor_client::EndpointError>((capture, encoder, format))
    };

    let (mut capture, mut encoder, mut format) = setup(selected, generation)?;
    desktop_message(&mut writer, DesktopMessage::Format(format.clone()))?;
    let mut last_frame = Instant::now() - Duration::from_millis(34);
    let mut last_cursor_at = Instant::now() - Duration::from_millis(101);
    let mut last_cursor = None;
    let capture_started = Instant::now();
    let result = (|| loop {
        if control.is_stopped() {
            return Ok(());
        }
        while let Ok(command) = command_receiver.try_recv() {
            match command {
                HostCommand::SelectDisplay(index) => {
                    if index >= displays.len() as u32 {
                        return Err(sensor_client::EndpointError::Desktop(
                            "Unknown monitor selected.".into(),
                        ));
                    }
                    let next_generation = generation.saturating_add(1);
                    let (next_capture, next_encoder, next_format) = setup(index, next_generation)?;
                    generation = next_generation;
                    generation_shared.store(generation, Ordering::SeqCst);
                    if let Ok(mut current) = active.lock() {
                        *current = next_capture.display.clone();
                    }
                    capture = next_capture;
                    encoder = next_encoder;
                    format = next_format;
                    desktop_message(&mut writer, DesktopMessage::Format(format.clone()))?;
                }
                HostCommand::Pong(value) => {
                    desktop_message(&mut writer, DesktopMessage::Pong(value))?;
                }
                HostCommand::Close => {
                    let _ = desktop_message(&mut writer, DesktopMessage::Close);
                    return Ok(());
                }
                HostCommand::Failed(error) => {
                    return Err(sensor_client::EndpointError::Desktop(error));
                }
            }
        }
        if last_frame.elapsed() >= Duration::from_millis(33) {
            match capture
                .next(consent)
                .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?
            {
                Some(frame) => {
                    let frame = sensor_media::rotate_scale_bgra(
                        frame,
                        capture.rotation,
                        format.width,
                        format.height,
                    )
                    .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
                    let nv12 = sensor_media::bgra_to_nv12(&frame).map_err(|error| {
                        sensor_client::EndpointError::Desktop(error.to_string())
                    })?;
                    let timestamp = capture_started
                        .elapsed()
                        .as_micros()
                        .saturating_mul(10)
                        .min(i64::MAX as u128) as i64;
                    for packet in encoder
                        .encode(&nv12, timestamp)
                        .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?
                    {
                        let total = u32::try_from(packet.bytes.len()).map_err(|_| {
                            sensor_client::EndpointError::Desktop(
                                "Encoded frame is too large.".into(),
                            )
                        })?;
                        for (offset, bytes) in packet
                            .bytes
                            .chunks(sensor_media::VIDEO_FRAGMENT)
                            .enumerate()
                        {
                            let offset = offset
                                .checked_mul(sensor_media::VIDEO_FRAGMENT)
                                .and_then(|value| u32::try_from(value).ok())
                                .ok_or_else(|| {
                                    sensor_client::EndpointError::Desktop(
                                        "Encoded frame offset overflow.".into(),
                                    )
                                })?;
                            desktop_message(
                                &mut writer,
                                DesktopMessage::Fragment {
                                    generation,
                                    sequence: packet.timestamp_100ns as u64,
                                    timestamp_100ns: packet.timestamp_100ns,
                                    keyframe: packet.keyframe,
                                    offset,
                                    total,
                                    bytes: bytes.to_vec(),
                                },
                            )?;
                        }
                    }
                    last_frame = Instant::now();
                }
                None => thread::sleep(Duration::from_millis(1)),
            }
        } else {
            thread::sleep(Duration::from_millis(1));
        }
        if last_cursor_at.elapsed() >= Duration::from_millis(100) {
            let cursor = capture
                .cursor()
                .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
            if last_cursor != Some(cursor) {
                desktop_message(
                    &mut writer,
                    DesktopMessage::Cursor {
                        generation,
                        x: cursor.0,
                        y: cursor.1,
                        visible: cursor.2,
                    },
                )?;
                last_cursor = Some(cursor);
            }
            last_cursor_at = Instant::now();
        }
    })();
    drop(writer);
    let _ = reader_thread.join();
    result
}

#[cfg(windows)]
fn execute_remote(
    route: Route,
    control_mode: bool,
    identity: DeviceIdentity,
    peer: ExpectedPeer,
    events: &SyncSender<Event>,
    control: &Control,
) -> Result<String, String> {
    events
        .send(Event::Status(format!(
            "Connecting via {}...",
            route.label()
        )))
        .map_err(|_| "Window closed".to_owned())?;
    let stream = connect_route(&route, &identity)?;
    control.socket(&stream)?;
    let mut connection = SecureConnection::initiate(stream, &identity, peer, DEFAULT_TIMEOUT)
        .map_err(|error| error.to_string())?;
    connection
        .set_timeout(Duration::from_secs(120))
        .map_err(|error| error.to_string())?;
    let mode = if control_mode {
        Mode::RemoteControl
    } else {
        Mode::ScreenView
    };
    sensor_client::request(&mut connection, mode).map_err(|error| error.to_string())?;
    events
        .send(Event::Status(format!(
            "Remote desktop active • peer {} • H.264 over {}",
            peer.device_id,
            route.label()
        )))
        .map_err(|_| "Window closed".to_owned())?;
    let (mut reader, mut writer) = connection.split().map_err(|error| error.to_string())?;
    let (commands, command_receiver) = mpsc::sync_channel(256);
    control.set_remote_sender(commands.clone());
    let writer_control = control.clone();
    let writer_thread = thread::spawn(move || {
        let mut last_ping = Instant::now();
        loop {
            if writer_control.is_stopped() {
                break;
            }
            match command_receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(message) => {
                    if writer.send(&Message::Desktop(message)).is_err() {
                        writer_control.stop();
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if last_ping.elapsed() >= Duration::from_secs(5) {
                        if writer
                            .send(&Message::Desktop(DesktopMessage::Ping(
                                Instant::now().elapsed().as_nanos() as u64,
                            )))
                            .is_err()
                        {
                            writer_control.stop();
                            break;
                        }
                        last_ping = Instant::now();
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    let result = (|| {
        let mut assembler = sensor_media::Assembler::default();
        let mut decoder: Option<codec::H264Decoder> = None;
        loop {
            let message: Message = match reader.receive() {
                Ok(message) => message,
                Err(_error) if control.is_stopped() => break Ok(()),
                Err(error) => return Err(error.to_string()),
            };
            match message {
                Message::Desktop(DesktopMessage::Displays(displays)) => {
                    for display in &displays {
                        display.validate().map_err(|error| error.to_string())?;
                    }
                    events
                        .send(Event::RemoteDisplays(displays))
                        .map_err(|_| "Window closed".to_owned())?;
                }
                Message::Desktop(DesktopMessage::Format(format)) => {
                    format.validate().map_err(|error| error.to_string())?;
                    decoder = Some(
                        codec::H264Decoder::new(format.width, format.height, format.fps_limit)
                            .map_err(|error| error.to_string())?,
                    );
                    assembler.clear();
                    events
                        .send(Event::RemoteFormat(format))
                        .map_err(|_| "Window closed".to_owned())?;
                }
                Message::Desktop(message @ DesktopMessage::Fragment { .. }) => {
                    let Some(frame) = assembler.push(message).map_err(|error| error.to_string())?
                    else {
                        continue;
                    };
                    let decoder = decoder
                        .as_mut()
                        .ok_or_else(|| "Received video before its format.".to_owned())?;
                    for frame in decoder.decode(&frame).map_err(|error| error.to_string())? {
                        control.publish_frame(frame);
                    }
                }
                Message::Desktop(DesktopMessage::Cursor { x, y, visible, .. }) => {
                    events
                        .send(Event::RemoteCursor { x, y, visible })
                        .map_err(|_| "Window closed".to_owned())?;
                }
                Message::Desktop(DesktopMessage::Ping(value)) => {
                    let _ = commands.try_send(DesktopMessage::Pong(value));
                }
                Message::Desktop(DesktopMessage::Close) | Message::Close => break Ok(()),
                Message::Desktop(DesktopMessage::Pong(_)) => {}
                Message::Desktop(DesktopMessage::Error(error)) => return Err(error),
                Message::Desktop(DesktopMessage::Input { .. })
                | Message::Desktop(DesktopMessage::SelectDisplay(_)) => {
                    return Err("Peer sent an invalid desktop command.".into())
                }
                _ => return Err("Unexpected message in remote desktop session.".into()),
            }
        }
    })();
    control.clear_remote_sender();
    control.stop();
    drop(commands);
    let _ = writer_thread.join();
    result.map(|()| "Remote desktop session closed.".into())
}

#[cfg(not(windows))]
fn execute_remote(
    _route: Route,
    _control_mode: bool,
    _identity: DeviceIdentity,
    _peer: ExpectedPeer,
    _events: &SyncSender<Event>,
    _control: &Control,
) -> Result<String, String> {
    Err("Remote desktop sessions require Windows.".into())
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
                route: Route::Direct("127.0.0.1:0".parse().unwrap()),
                receive_dir: dir.path().into(),
                auto_accept: false,
                accept_any: false,
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
