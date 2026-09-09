use sensor_audit::AuditLog;
use sensor_client::{LocalInteraction, Message, Mode};
use sensor_files::{Receiver as FileReceiver, TransferId};
use sensor_identity::DeviceIdentity;
use sensor_media::{DecodedFrame, DesktopMessage, Display, VideoFormat};
use sensor_relay::join as join_relay;
use sensor_render::{
    accept_with_control as accept_render, connect_with_control as connect_render,
    CONNECT_TIMEOUT as RENDER_TIMEOUT,
};
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
    Online(bool),
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
pub struct RenderRoute {
    pub server: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Route {
    Direct(SocketAddr),
    Relay(RelayRoute),
    Render(RenderRoute),
}

impl Route {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Direct(_) => "Direct TCP",
            Self::Relay(_) => "Secure relay",
            Self::Render(_) => "Render HTTPS/WSS",
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
        clipboard: bool,
    },
}

#[derive(Clone, Default)]
pub struct Control {
    cancelled: Arc<AtomicBool>,
    local_stop: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<ConnectionAbort>>>,
    remote_sender: Arc<Mutex<Option<SyncSender<DesktopMessage>>>>,
    remote_frame: Arc<Mutex<Option<Arc<DecodedFrame>>>>,
    clipboard_enabled: Arc<AtomicBool>,
    clipboard_allowed: Arc<AtomicBool>,
    decoded_frames: Arc<AtomicU64>,
    video_bytes: Arc<AtomicU64>,
    rtt_micros: Arc<AtomicU64>,
    captured_frames: Arc<AtomicU64>,
    encoded_frames: Arc<AtomicU64>,
    telemetry: Arc<Mutex<Option<sensor_media::adaptive::Telemetry>>>,
    presented_frames: Arc<AtomicU64>,
}
impl Control {
    pub fn telemetry(&self) -> Option<sensor_media::adaptive::Telemetry> {
        self.telemetry.lock().ok().and_then(|value| value.clone())
    }
    pub fn mark_presented(&self) {
        self.presented_frames.fetch_add(1, Ordering::Relaxed);
    }
    pub fn presented_frames(&self) -> u64 {
        self.presented_frames.load(Ordering::Relaxed)
    }
    pub fn set_clipboard_enabled(&self, enabled: bool) {
        self.clipboard_enabled
            .store(enabled && self.clipboard_allowed(), Ordering::SeqCst);
    }
    pub fn clipboard_allowed(&self) -> bool {
        self.clipboard_allowed.load(Ordering::SeqCst)
    }
    fn grant_clipboard(&self, allowed: bool) {
        self.clipboard_allowed.store(allowed, Ordering::SeqCst);
        self.set_clipboard_enabled(allowed);
    }
    pub fn clipboard_enabled(&self) -> bool {
        self.clipboard_enabled.load(Ordering::SeqCst)
    }
    pub fn statistics(&self) -> (u64, u64, u64) {
        (
            self.decoded_frames.load(Ordering::Relaxed),
            self.video_bytes.load(Ordering::Relaxed),
            self.rtt_micros.load(Ordering::Relaxed),
        )
    }
    pub fn capture_statistics(&self) -> (u64, u64) {
        (
            self.captured_frames.load(Ordering::Relaxed),
            self.encoded_frames.load(Ordering::Relaxed),
        )
    }
    pub fn stop(&self) {
        if self.local_stop.swap(true, Ordering::SeqCst) {
            self.abort();
            return;
        }
        self.set_clipboard_enabled(false);
        let sender = self
            .remote_sender
            .lock()
            .ok()
            .and_then(|mut sender| sender.take());
        if sender.is_some_and(|sender| sender.try_send(DesktopMessage::Close).is_ok()) {
            // Block further input immediately; allow a bounded close acknowledgement.
            let control = self.clone();
            thread::spawn(move || {
                let until = Instant::now() + Duration::from_millis(750);
                while !control.is_stopped() && Instant::now() < until {
                    thread::sleep(Duration::from_millis(5));
                }
                control.abort();
            });
            return;
        }
        self.abort();
    }
    pub fn was_stopped_locally(&self) -> bool {
        self.local_stop.load(Ordering::SeqCst)
    }
    fn abort(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.close_socket();
    }
    fn close_socket(&self) {
        if let Ok(mut socket) = self.socket.lock() {
            if let Some(socket) = socket.take() {
                socket.abort();
            }
        }
    }
    pub fn is_stopped(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    pub fn send_remote(&self, message: DesktopMessage) -> bool {
        if self.was_stopped_locally() || self.is_stopped() {
            return false;
        }
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
    #[cfg(windows)]
    fn set_remote_sender(&self, sender: SyncSender<DesktopMessage>) {
        if let Ok(mut current) = self.remote_sender.lock() {
            *current = Some(sender);
        }
    }
    #[cfg(windows)]
    fn clear_remote_sender(&self) {
        if let Ok(mut current) = self.remote_sender.lock() {
            *current = None;
        }
    }
    #[cfg(windows)]
    fn publish_frame(&self, frame: DecodedFrame) {
        self.decoded_frames.fetch_add(1, Ordering::Relaxed);
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
    route_label: &'static str,
}
impl LocalInteraction for Interaction<'_> {
    fn accept(&mut self, peer: ExpectedPeer, mode: Mode) -> bool {
        if self.auto_accept {
            self.control.grant_clipboard(
                mode.permissions()
                    .allows(sensor_session::permissions::Permission::ClipboardText),
            );
            let _ = self.events.send(Event::Status(format!(
                "Auto-accepted {mode:?} session • pinned peer {} • explicit local profile",
                peer.device_id
            )));
            return true;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let accepted = self.events.send(Event::Consent(peer, mode, sender)).is_ok()
            && wait_reply(receiver, self.control).unwrap_or(false);
        if accepted {
            self.control.grant_clipboard(
                mode.permissions()
                    .allows(sensor_session::permissions::Permission::ClipboardText),
            );
        }
        let _ = self.events.send(Event::Status(if accepted {
            format!(
                "Active {mode:?} session • peer {} • {} • ChaCha20-Poly1305",
                peer.device_id, self.route_label
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
        host_desktop(
            connection,
            consent,
            self.control,
            self.route_label == "Render HTTPS/WSS",
        )
    }
}
fn compose(events: &SyncSender<Event>, control: &Control) -> Option<String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    events.send(Event::Compose(sender)).ok()?;
    wait_reply(receiver, control).flatten()
}

fn connect_route(
    route: &Route,
    identity: &DeviceIdentity,
    peer: ExpectedPeer,
    control: &Control,
) -> Result<TcpStream, String> {
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
        Route::Render(render) => {
            connect_render(&render.server, identity, peer, RENDER_TIMEOUT, &|| {
                control.is_stopped()
            })
            .map_err(|error| error.to_string())
        }
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
        if auto_accept && (accept_any || peer.public_key == [0; 32]) {
            return Err("Automatic acceptance requires one explicitly pinned peer; unknown peers always require local consent.".into());
        }
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
            Route::Render(render) => {
                status(format!(
                    "Registering Device ID {} through Render HTTPS/WSS • {}",
                    identity.device_id(),
                    render.server
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
                    Route::Render(render) => {
                        let mut delay = Duration::from_secs(1);
                        loop {
                            let _ = events.send(Event::Online(false));
                            match accept_render(
                                &render.server,
                                &identity,
                                RENDER_TIMEOUT,
                                &|| control.is_stopped(),
                                &|| {
                                    let _ = events.send(Event::Online(true));
                                },
                            ) {
                                Ok(stream) => {
                                    let _ = events.send(Event::Online(false));
                                    break stream;
                                }
                                Err(_) if control.is_stopped() => {
                                    return Ok("Internet listener stopped.".into())
                                }
                                Err(error) => {
                                    status(format!("Internet connection interrupted ({error}). Retrying in {}s...", delay.as_secs()))?;
                                    let retry_at = Instant::now() + delay;
                                    while Instant::now() < retry_at {
                                        if control.is_stopped() {
                                            return Ok("Internet listener stopped.".into());
                                        }
                                        thread::sleep(Duration::from_millis(25));
                                    }
                                    delay = (delay * 2).min(Duration::from_secs(30));
                                }
                            }
                        }
                    }
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
                Err(error)
                    if (listener.is_some() || matches!(route, Route::Render(_)))
                        && !control.is_stopped() =>
                {
                    // The cancellation handle owns a cloned socket. Dropping
                    // only the failed handshake's stream leaves the peer
                    // waiting until Windows reports 10060.
                    control.close_socket();
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
            route_label: route.label(),
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
            clipboard,
        } => {
            return execute_remote(
                route.clone(),
                *remote_control,
                *clipboard,
                identity,
                peer,
                events,
                control,
            );
        }
        _ => unreachable!(),
    };
    status(format!("Connecting via {}...", route.label()))?;
    let stream = connect_route(&route, &identity, peer, control)?;
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
    // Cancellation is a transport abort, not a graceful peer close. In
    // particular, wait_reply can observe cancelled before abort has acquired
    // the socket mutex. Do not race that shutdown by writing a success Close.
    if !control.was_stopped_locally() && !control.is_stopped() {
        connection
            .send(&Message::Close)
            .map_err(|e| e.to_string())?;
    }
    Ok("Chat closed.".into())
}

#[cfg(windows)]
enum HostCommand {
    SelectDisplay(u32),
    SelectVideoProfile(sensor_media::VideoProfile),
    Pong(u64),
    Close,
    Failed(String),
    Clipboard(String),
    Ack {
        generation: u64,
        sequence: u64,
        decode_us: u64,
        rtt_us: u64,
    },
}

#[cfg(windows)]
struct InputTarget {
    generation: u64,
    display: Display,
}

#[cfg(windows)]
struct HostCommands(SyncSender<HostCommand>, Control);
#[cfg(windows)]
impl HostCommands {
    fn send(&self, command: HostCommand) -> Result<(), mpsc::TrySendError<HostCommand>> {
        let result = self.0.try_send(command);
        if result.is_err() {
            // A stalled or flooding peer must not block shutdown or leave input held.
            self.1.abort();
        }
        result
    }
}

#[cfg(windows)]
struct DesktopStop(Control);
#[cfg(windows)]
impl Drop for DesktopStop {
    fn drop(&mut self) {
        self.0.abort();
    }
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
fn send_video_packets(
    writer: &mut sensor_transport::connection::SecureWriter,
    packets: Vec<sensor_media::EncodedFrame>,
    generation: u64,
    control: &Control,
    flight: &mut sensor_media::adaptive::FlightWindow,
    telemetry: &mut sensor_media::adaptive::Telemetry,
) -> Result<(), sensor_client::EndpointError> {
    for packet in packets {
        flight.encoded(packet.timestamp_100ns as u64, packet.bytes.len());
        telemetry.bytes += packet.bytes.len() as u64;
        let send_started = Instant::now();
        let total = u32::try_from(packet.bytes.len()).map_err(|_| {
            sensor_client::EndpointError::Desktop("Encoded frame is too large.".into())
        })?;
        for (index, bytes) in packet
            .bytes
            .chunks(sensor_media::VIDEO_FRAGMENT)
            .enumerate()
        {
            if send_started.elapsed() > Duration::from_millis(500) {
                return Err(sensor_client::EndpointError::Desktop("Video writer exceeded its interactive deadline; session closed to prevent stale input.".into()));
            }
            let offset = index
                .checked_mul(sensor_media::VIDEO_FRAGMENT)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    sensor_client::EndpointError::Desktop("Encoded frame offset overflow.".into())
                })?;
            desktop_message(
                writer,
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
        control.encoded_frames.fetch_add(1, Ordering::Relaxed);
        telemetry.encoded += 1;
        telemetry.send_us += send_started.elapsed().as_micros() as u64;
    }
    Ok(())
}

#[cfg(windows)]
fn spawn_host_reader(
    mut reader: sensor_transport::connection::SecureReader,
    commands: SyncSender<HostCommand>,
    control: Control,
    displays: Arc<Vec<Display>>,
    active: Arc<Mutex<InputTarget>>,
    permissions: Permissions,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let commands = HostCommands(commands, control.clone());
        let mut injector = input::Injector::default();
        let mut injected_generation = 0;
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
                    let target = match active.lock() {
                        Ok(target) => target,
                        Err(_) => {
                            let _ = commands.send(HostCommand::Failed(
                                "Remote display state became unavailable.".into(),
                            ));
                            break;
                        }
                    };
                    if target.generation == 0 || input_generation != target.generation {
                        continue;
                    }
                    if injected_generation != target.generation {
                        let _ = injector.release();
                        injected_generation = target.generation;
                    }
                    // Keep the target locked through injection: generation and
                    // coordinates must always describe the same selected display.
                    if let Err(error) = injector.apply(&event, &target.display, &local_consent) {
                        let _ = commands.send(HostCommand::Failed(error.to_string()));
                        break;
                    }
                }
                Message::Desktop(
                    message @ (DesktopMessage::SelectDisplay(_)
                    | DesktopMessage::SelectVideoProfile(_)),
                ) => {
                    let index = match message {
                        DesktopMessage::SelectDisplay(index) => index,
                        _ => active
                            .lock()
                            .map(|target| target.display.index)
                            .unwrap_or(u32::MAX),
                    };
                    if !displays.iter().any(|display| display.index == index) {
                        let _ =
                            commands.send(HostCommand::Failed("Unknown monitor selected.".into()));
                        break;
                    }
                    if let Err(error) = injector.release() {
                        let _ = commands.send(HostCommand::Failed(error.to_string()));
                        break;
                    }
                    if let Ok(mut target) = active.lock() {
                        // Ignore in-flight coordinates until capture and the
                        // advertised format have switched together.
                        target.generation = 0;
                    } else {
                        let _ = commands.send(HostCommand::Failed(
                            "Remote display state became unavailable.".into(),
                        ));
                        break;
                    }
                    let command = match message {
                        DesktopMessage::SelectVideoProfile(profile) => {
                            HostCommand::SelectVideoProfile(profile)
                        }
                        _ => HostCommand::SelectDisplay(index),
                    };
                    if commands.send(command).is_err() {
                        break;
                    }
                }
                Message::Desktop(DesktopMessage::Ping(value)) => {
                    if commands.send(HostCommand::Pong(value)).is_err() {
                        break;
                    }
                }
                Message::Desktop(DesktopMessage::FrameAck {
                    generation,
                    sequence,
                    decode_us,
                    rtt_us,
                }) => {
                    if commands
                        .send(HostCommand::Ack {
                            generation,
                            sequence,
                            decode_us,
                            rtt_us,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Message::Desktop(DesktopMessage::ClipboardText(text)) => {
                    if local_consent
                        .require(sensor_session::permissions::Permission::ClipboardText)
                        .is_err()
                        || sensor_media::validate_clipboard(&text).is_err()
                    {
                        let _ = commands.send(HostCommand::Failed(
                            "Clipboard permission denied or invalid text.".into(),
                        ));
                        break;
                    }
                    if commands.send(HostCommand::Clipboard(text)).is_err() {
                        break;
                    }
                }
                Message::Desktop(DesktopMessage::Close) | Message::Close => {
                    let _ = injector.release();
                    if commands.send(HostCommand::Close).is_ok() {
                        // Dropping either secure half aborts the shared socket.
                        // Keep the reader alive until the writer acknowledges
                        // Close and the host's shutdown guard releases us.
                        while !control.is_stopped() {
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
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
    _internet: bool,
) -> Result<(), sensor_client::EndpointError> {
    // Every return path must abort the blocking reader and release held input.
    let _stop = DesktopStop(control.clone());
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
    writer.set_timeout(Duration::from_millis(250))?;
    desktop_message(&mut writer, DesktopMessage::Displays(displays.clone()))?;

    let active = Arc::new(Mutex::new(InputTarget {
        generation,
        display: displays[0].clone(),
    }));
    let (commands, command_receiver) = mpsc::sync_channel(64);
    let reader_thread = spawn_host_reader(
        reader,
        commands,
        control.clone(),
        Arc::new(displays.clone()),
        active.clone(),
        permissions,
    );

    let ceiling = std::env::var("SENSOR_VIDEO_MAX_BITRATE")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(sensor_media::adaptive::DEFAULT_MAX_BITRATE);
    let mut adaptive = sensor_media::adaptive::Adaptive::new(ceiling);
    let mut prefer_hardware = true;
    let setup = |index: u32,
                 generation: u64,
                 adaptive: &sensor_media::adaptive::Adaptive,
                 prefer_hardware| {
        let capture = desktop::Capture::new(index, consent).map_err(|error| {
            sensor_client::EndpointError::Desktop(format!("Capture monitor {index}: {error}"))
        })?;
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
        let target = adaptive
            .target(output_width, output_height)
            .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
        let sensor_media::adaptive::Target {
            width,
            height,
            fps,
            bitrate,
        } = target;
        let encoder = codec::H264Encoder::new(width, height, fps, bitrate, prefer_hardware)
            .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
        let format = VideoFormat {
            generation,
            display: capture.display.clone(),
            width,
            height,
            fps_limit: fps,
            bitrate,
            encoder: encoder.name.clone(),
            hardware: encoder.hardware,
        };
        format
            .validate()
            .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
        Ok::<_, sensor_client::EndpointError>((capture, encoder, format))
    };

    let (initial_capture, initial_encoder, mut format) =
        setup(selected, generation, &adaptive, prefer_hardware)?;
    let mut capture = Some(initial_capture);
    let mut encoder = Some(initial_encoder);
    desktop_message(&mut writer, DesktopMessage::Format(format.clone()))?;
    let mut last_frame = Instant::now() - Duration::from_millis(34);
    let mut last_cursor_at = Instant::now() - Duration::from_millis(101);
    let mut last_cursor = None;
    let capture_started = Instant::now();
    let mut clipboard = sensor_windows::clipboard::TextClipboard::default();
    let mut clipboard_at = Instant::now();
    let mut flight = sensor_media::adaptive::FlightWindow::default();
    let mut telemetry = sensor_media::adaptive::Telemetry::default();
    let mut feedback = sensor_media::adaptive::Feedback::default();
    let mut measured_at = Instant::now();
    let mut previous = telemetry.clone();
    let mut reconfigure = None;
    let result = (|| loop {
        if control.is_stopped() {
            return Ok(());
        }
        // Drain input acknowledgements and control before any new video work.
        for _ in 0..64 {
            let Ok(command) = command_receiver.try_recv() else {
                break;
            };
            match command {
                command @ (HostCommand::SelectDisplay(_) | HostCommand::SelectVideoProfile(_)) => {
                    let index = match command {
                        HostCommand::SelectDisplay(index) => index,
                        HostCommand::SelectVideoProfile(next) => {
                            adaptive.profile = next;
                            format.display.index
                        }
                        _ => unreachable!(),
                    };
                    if index >= displays.len() as u32 {
                        return Err(sensor_client::EndpointError::Desktop(
                            "Unknown monitor selected.".into(),
                        ));
                    }
                    reconfigure = Some(index);
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
                HostCommand::Clipboard(text) => {
                    let _ = clipboard.apply(&text, control.clipboard_enabled(), consent);
                }
                HostCommand::Ack {
                    generation: ack_generation,
                    sequence,
                    decode_us,
                    rtt_us,
                } => {
                    if ack_generation == generation {
                        if let Some(delay) = flight
                            .acknowledge(sequence, capture_started.elapsed().as_millis() as u64)
                        {
                            feedback.delivery_ms = feedback.delivery_ms.max(delay);
                            feedback.decode_us = feedback.decode_us.max(decode_us.min(5_000_000));
                            feedback.rtt_ms = rtt_us.min(10_000_000) / 1000;
                            feedback.acknowledged += 1;
                            telemetry.delivery_ms = delay;
                        }
                    }
                }
            }
        }
        let now_ms = capture_started.elapsed().as_millis() as u64;
        if flight.oldest_ms(now_ms) > 5_000 {
            return Err(sensor_client::EndpointError::Desktop(
                "Video delivery stalled for 5 seconds; reconnect after checking the network."
                    .into(),
            ));
        }
        if measured_at.elapsed() >= Duration::from_secs(1) {
            feedback.delivery_ms = feedback.delivery_ms.max(flight.oldest_ms(now_ms));
            let captures = telemetry.captured.saturating_sub(previous.captured).max(1);
            feedback.prepare_encode_us =
                (telemetry.capture_us + telemetry.prepare_us + telemetry.encode_us)
                    .saturating_sub(previous.capture_us + previous.prepare_us + previous.encode_us)
                    / captures;
            adaptive.observe(feedback);
            let target = adaptive
                .target(format.display.width, format.display.height)
                .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?;
            if (target.width, target.height, target.fps)
                != (format.width, format.height, format.fps_limit)
            {
                reconfigure = Some(format.display.index);
            } else if target.bitrate != format.bitrate {
                if encoder
                    .as_mut()
                    .is_some_and(|value| value.set_bitrate(target.bitrate).is_ok())
                {
                    format.bitrate = target.bitrate;
                } else {
                    reconfigure = Some(format.display.index);
                }
            }
            telemetry.elapsed_ms = now_ms;
            telemetry.in_flight_bytes = flight.bytes() as u64;
            telemetry.profile = adaptive.profile;
            telemetry.congestion = adaptive.congested;
            *control.telemetry.lock().map_err(|_| {
                sensor_client::EndpointError::Desktop("Telemetry unavailable".into())
            })? = Some(telemetry.clone());
            desktop_message(&mut writer, DesktopMessage::Telemetry(telemetry.clone()))?;
            previous = telemetry.clone();
            feedback = sensor_media::adaptive::Feedback::default();
            measured_at = Instant::now();
        }
        if let Some(index) = reconfigure.take() {
            // Drop old reference chain and advertise a new generation before
            // accepting input or transmitting output from the replacement MFT.
            drop(capture.take());
            drop(encoder.take());
            generation += 1;
            let (next_capture, next_encoder, next_format) =
                setup(index, generation, &adaptive, prefer_hardware)?;
            capture = Some(next_capture);
            encoder = Some(next_encoder);
            format = next_format;
            flight = sensor_media::adaptive::FlightWindow::default();
            desktop_message(&mut writer, DesktopMessage::Format(format.clone()))?;
            *active.lock().map_err(|_| {
                sensor_client::EndpointError::Desktop("Remote display unavailable".into())
            })? = InputTarget {
                generation,
                display: format.display.clone(),
            };
        }
        // Poll async hardware output even when the desktop is static.
        let pending = match encoder.as_mut().expect("configured encoder").available() {
            Ok(pending) => pending,
            Err(_) if prefer_hardware && format.hardware => {
                prefer_hardware = false;
                reconfigure = Some(format.display.index);
                continue;
            }
            Err(error) => return Err(sensor_client::EndpointError::Desktop(error.to_string())),
        };
        let send_at = Instant::now();
        send_video_packets(
            &mut writer,
            pending,
            generation,
            control,
            &mut flight,
            &mut telemetry,
        )?;
        feedback.write_stall_ms = feedback
            .write_stall_ms
            .max(send_at.elapsed().as_millis() as u64);
        if clipboard_at.elapsed() >= Duration::from_millis(250) {
            if let Some(text) = clipboard.poll(control.clipboard_enabled(), consent) {
                desktop_message(&mut writer, DesktopMessage::ClipboardText(text))?;
            }
            clipboard_at = Instant::now();
        }
        let capture = capture.as_mut().ok_or_else(|| {
            sensor_client::EndpointError::Desktop("Capture is not configured.".into())
        })?;
        let encoder = encoder.as_mut().ok_or_else(|| {
            sensor_client::EndpointError::Desktop("Encoder is not configured.".into())
        })?;
        if last_frame.elapsed() >= Duration::from_secs_f64(1.0 / format.fps_limit as f64) {
            let frame_started = Instant::now();
            // Never drop encoded P-frames: skip capture before creating codec
            // dependencies. Input is processed by its independent reader.
            if !flight.can_submit(now_ms, adaptive.bitrate()) {
                telemetry.skipped += 1;
                feedback.saturated |= flight.oldest_ms(now_ms) >= 500;
                last_frame = frame_started;
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            match capture
                .next(consent)
                .map_err(|error| sensor_client::EndpointError::Desktop(error.to_string()))?
            {
                Some(frame) => {
                    telemetry.capture_us += frame_started.elapsed().as_micros() as u64;
                    telemetry.captured += 1;
                    let prepare_at = Instant::now();
                    control.captured_frames.fetch_add(1, Ordering::Relaxed);
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
                    telemetry.prepare_us += prepare_at.elapsed().as_micros() as u64;
                    let encode_at = Instant::now();
                    flight.submit(timestamp as u64, now_ms);
                    let packets = match encoder.encode(&nv12, timestamp) {
                        Ok(packets) => packets,
                        Err(_) if prefer_hardware && format.hardware => {
                            prefer_hardware = false;
                            reconfigure = Some(format.display.index);
                            continue;
                        }
                        Err(error) => {
                            return Err(sensor_client::EndpointError::Desktop(error.to_string()))
                        }
                    };
                    telemetry.encode_us += encode_at.elapsed().as_micros() as u64;
                    let send_at = Instant::now();
                    send_video_packets(
                        &mut writer,
                        packets,
                        generation,
                        control,
                        &mut flight,
                        &mut telemetry,
                    )?;
                    feedback.write_stall_ms = feedback
                        .write_stall_ms
                        .max(send_at.elapsed().as_millis() as u64);
                    last_frame = frame_started;
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
    control.abort();
    drop(writer);
    drop(command_receiver);
    let _ = reader_thread.join();
    result
}

#[cfg(windows)]
fn execute_remote(
    route: Route,
    control_mode: bool,
    clipboard_mode: bool,
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
    let stream = connect_route(&route, &identity, peer, control)
        .map_err(|error| format!("Could not reach the remote device: {error}"))?;
    control.socket(&stream)?;
    let mut connection = SecureConnection::initiate(stream, &identity, peer, DEFAULT_TIMEOUT)
        .map_err(|error| format!("Remote identity handshake failed: {error}. Check that the other device is online and uses a compatible SENSOR version."))?;
    connection
        .set_timeout(Duration::from_secs(120))
        .map_err(|error| error.to_string())?;
    let mode = match (control_mode, clipboard_mode) {
        (true, true) => Mode::RemoteControlClipboard,
        (false, true) => Mode::ScreenViewClipboard,
        (true, false) => Mode::RemoteControl,
        (false, false) => Mode::ScreenView,
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
    writer
        .set_timeout(Duration::from_millis(250))
        .map_err(|error| error.to_string())?;
    let (commands, command_receiver) = mpsc::sync_channel(256);
    let (clipboard_send, clipboard_receive) = mpsc::sync_channel::<String>(1);
    control.grant_clipboard(clipboard_mode);
    control.set_remote_sender(commands.clone());
    let writer_control = control.clone();
    let session_clock = Instant::now();
    let writer_thread = thread::spawn(move || {
        let mut last_ping = Instant::now();
        let mut clipboard_at = Instant::now();
        let mut clipboard = sensor_windows::clipboard::TextClipboard::default();
        let mut consent = Consent::pending(mode.permissions());
        if consent.accept(mode.permissions()).is_err() {
            writer_control.abort();
            return;
        }
        loop {
            if writer_control.is_stopped() {
                break;
            }
            if let Ok(text) = clipboard_receive.try_recv() {
                let _ = clipboard.apply(&text, writer_control.clipboard_enabled(), &consent);
            }
            if clipboard_at.elapsed() >= Duration::from_millis(250) {
                if let Some(text) = clipboard.poll(writer_control.clipboard_enabled(), &consent) {
                    if writer
                        .send(&Message::Desktop(DesktopMessage::ClipboardText(text)))
                        .is_err()
                    {
                        writer_control.abort();
                        break;
                    }
                }
                clipboard_at = Instant::now();
            }
            if last_ping.elapsed() >= Duration::from_secs(2) {
                let stamp = session_clock.elapsed().as_micros().min(u64::MAX as u128) as u64;
                if writer
                    .send(&Message::Desktop(DesktopMessage::Ping(stamp)))
                    .is_err()
                {
                    writer_control.abort();
                    break;
                }
                last_ping = Instant::now();
            }
            match command_receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(message) => {
                    if writer.send(&Message::Desktop(message)).is_err() {
                        writer_control.abort();
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    let result = (|| {
        let mut assembler = sensor_media::Assembler::default();
        let mut decoder: Option<codec::H264Decoder> = None;
        let mut generation = 0;
        loop {
            let message: Message = match reader.receive() {
                Ok(message) => message,
                Err(_error) if control.is_stopped() => break Ok(()),
                Err(error) => return Err(error.to_string()),
            };
            match message {
                Message::Desktop(DesktopMessage::Displays(displays)) => {
                    if displays.is_empty() || displays.len() > 64 {
                        return Err("Invalid monitor list".into());
                    }
                    for display in &displays {
                        display.validate().map_err(|error| error.to_string())?;
                    }
                    events
                        .send(Event::RemoteDisplays(displays))
                        .map_err(|_| "Window closed".to_owned())?;
                }
                Message::Desktop(DesktopMessage::Format(format)) => {
                    format.validate().map_err(|error| error.to_string())?;
                    generation = format.generation;
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
                    control
                        .video_bytes
                        .fetch_add(frame.bytes.len() as u64, Ordering::Relaxed);
                    let decoder = decoder
                        .as_mut()
                        .ok_or_else(|| "Received video before its format.".to_owned())?;
                    let sequence = frame.timestamp_100ns as u64;
                    let decode_at = Instant::now();
                    for frame in decoder.decode(&frame).map_err(|error| error.to_string())? {
                        control.publish_frame(frame);
                    }
                    commands
                        .try_send(DesktopMessage::FrameAck {
                            generation,
                            sequence,
                            decode_us: decode_at.elapsed().as_micros() as u64,
                            rtt_us: control.rtt_micros.load(Ordering::Relaxed),
                        })
                        .map_err(|_| "Frame feedback queue stalled".to_owned())?;
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
                Message::Desktop(DesktopMessage::Pong(stamp)) => {
                    let now = session_clock.elapsed().as_micros().min(u64::MAX as u128) as u64;
                    if let Some(rtt) = now.checked_sub(stamp) {
                        control.rtt_micros.store(rtt, Ordering::Relaxed);
                    }
                }
                Message::Desktop(DesktopMessage::ClipboardText(text)) => {
                    if !clipboard_mode || sensor_media::validate_clipboard(&text).is_err() {
                        return Err("Clipboard permission denied or invalid text.".into());
                    }
                    if control.clipboard_enabled() {
                        let _ = clipboard_send.try_send(text);
                    }
                }
                Message::Desktop(DesktopMessage::Error(error)) => return Err(error),
                Message::Desktop(DesktopMessage::Telemetry(value)) => {
                    *control
                        .telemetry
                        .lock()
                        .map_err(|_| "Telemetry unavailable".to_owned())? = Some(value);
                }
                Message::Desktop(DesktopMessage::Input { .. })
                | Message::Desktop(DesktopMessage::SelectDisplay(_))
                | Message::Desktop(DesktopMessage::SelectVideoProfile(_)) => {
                    return Err("Peer sent an invalid desktop command.".into())
                }
                Message::Desktop(DesktopMessage::FrameAck { .. }) => {
                    return Err("Invalid viewer feedback direction".into())
                }
                _ => return Err("Unexpected message in remote desktop session.".into()),
            }
        }
    })();
    control.clear_remote_sender();
    control.abort();
    drop(commands);
    let _ = writer_thread.join();
    result.map(|()| "Remote desktop session closed.".into())
}

#[cfg(not(windows))]
fn execute_remote(
    _route: Route,
    _control_mode: bool,
    _clipboard_mode: bool,
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
    fn local_viewer_stop_sends_close_blocks_new_input_and_has_a_deadline() {
        let control = Control::default();
        let (sender, receiver) = mpsc::sync_channel(2);
        *control.remote_sender.lock().unwrap() = Some(sender);
        control.stop();
        assert!(control.was_stopped_locally());
        assert!(matches!(
            receiver.recv_timeout(Duration::from_millis(50)).unwrap(),
            DesktopMessage::Close
        ));
        assert!(!control.send_remote(DesktopMessage::Ping(1)));
        let until = Instant::now() + Duration::from_secs(2);
        while !control.is_stopped() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(control.is_stopped());
    }
    #[test]
    fn automatic_acceptance_cannot_be_combined_with_unknown_peers() {
        for (accept_any, public_key) in [(true, [1; 32]), (false, [0; 32])] {
            let root = tempfile::tempdir().unwrap();
            let config = root.path().join("must-not-be-created");
            let (events, _receiver) = mpsc::sync_channel(4);
            let identity = DeviceIdentity::generate();
            let result = execute(
                Task::Host {
                    route: Route::Direct("127.0.0.1:0".parse().unwrap()),
                    receive_dir: config.clone(),
                    auto_accept: true,
                    accept_any,
                },
                identity.clone(),
                ExpectedPeer {
                    device_id: identity.device_id(),
                    public_key,
                },
                config.clone(),
                &events,
                &Control::default(),
            );
            assert!(result.unwrap_err().contains("explicitly pinned"));
            assert!(!config.exists());
        }
    }
    #[cfg(windows)]
    #[test]
    fn full_host_queue_aborts_without_blocking_or_marking_local_stop() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let control = Control::default();
        let commands = HostCommands(sender, control.clone());
        assert!(commands.send(HostCommand::Pong(1)).is_ok());
        assert!(commands.send(HostCommand::Pong(2)).is_err());
        assert!(control.is_stopped());
        assert!(!control.was_stopped_locally());
    }
    #[test]
    fn internal_cleanup_does_not_disable_listener_reconnection() {
        let control = Control::default();
        control.abort();
        assert!(control.is_stopped());
        assert!(!control.was_stopped_locally());
        control.stop();
        assert!(control.was_stopped_locally());
    }
    #[test]
    fn rejected_handshake_closes_retained_socket_without_stopping_listener() {
        use std::io::Read;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let control = Control::default();
        control.socket(&stream).unwrap();
        drop(stream);
        control.close_socket();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
        assert!(!control.is_stopped());
        assert!(!control.was_stopped_locally());
        assert!(control.socket.lock().unwrap().is_none());
    }
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
