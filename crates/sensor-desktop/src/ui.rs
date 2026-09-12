use crate::remote_input;
use eframe::egui::{self, Color32, FontId, RichText, Stroke, Vec2};
use sensor_client::Mode;
use sensor_core::DeviceId;
use sensor_desktop::{
    hex, parse_hex, peer,
    settings::{self, Contact},
    worker::{self, Event, Job, RelayRoute, RenderRoute, Route, Task},
};
use sensor_files::TransferId;
use sensor_identity::{DeviceIdentity, IdentityFileStore};
use sensor_media::{DecodedFrame, DesktopMessage, Display, Input, VideoFormat};
use sensor_session::ExpectedPeer;
use sensor_windows::UserDpapi;
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    path::PathBuf,
    sync::{mpsc::SyncSender, Arc},
    time::{Duration, Instant},
};

const NAVY: Color32 = Color32::from_rgb(20, 46, 66);
const TEAL: Color32 = Color32::from_rgb(20, 185, 188);
const MUTED: Color32 = Color32::from_rgb(94, 113, 128);
const PAPER: Color32 = Color32::from_rgb(244, 247, 250);
const BORDER: Color32 = Color32::from_rgb(221, 230, 235);
const LOGO: &[u8] = include_bytes!("../../../assets/sensor-logo.jpeg");

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Connect,
    Files,
    Chat,
    Contacts,
    Diagnostics,
    Remote,
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Same app, UI, identity and encrypted protocol on every OS. Only the
    // presentation/capture backend changes. Never load DX12 on Windows 7.
    let os = sensor_windows::desktop::os_version()?;
    let platform = sensor_desktop::platform::Platform::select(
        os,
        cfg!(target_vendor = "win7"),
        std::env::var("SENSOR_UI_RENDERER").ok().as_deref(),
    )?;
    let renderer = match platform.presentation {
        sensor_desktop::platform::Presentation::OpenGl => eframe::Renderer::Glow,
        sensor_desktop::platform::Presentation::Direct3D12 => eframe::Renderer::Wgpu,
    };
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let config = match arguments.as_slice() {
        [] => PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is unavailable")?)
            .join("SENSOR Technology")
            .join("Remote"),
        [flag, path] if flag == "--config" => PathBuf::from(path),
        _ => return Err("Usage: SENSOR-Remote.exe [--config <directory>]".into()),
    };
    std::fs::create_dir_all(&config)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config.join("desktop.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| "SENSOR is already open for this profile. Close that window first.")?;
    let identity =
        IdentityFileStore::new(config.join("identity.bin"), UserDpapi).load_or_create()?;
    let contacts = settings::load(&config.join("contacts.json"))?;
    let unattended = sensor_desktop::unattended::load(
        &config.join("unattended.bin"),
        &UserDpapi,
        &identity.keypair().public_key(),
    )?;
    let receive = config.join("Received Files");
    std::fs::create_dir_all(&receive)?;
    let render_server = settings::resolve_network(
        std::env::var("SENSOR_SERVER")
            .or_else(|_| std::env::var("SENSOR_WS"))
            .ok(),
        &config.join("sensor-network.json"),
        &std::env::current_exe()?.with_file_name("sensor-network.json"),
    )?
    .unwrap_or_default();
    let render_mode = std::env::var("SENSOR_MODE")
        .map(|mode| mode.eq_ignore_ascii_case("RENDER_TEST"))
        .unwrap_or(!render_server.is_empty());
    let logo = image::load_from_memory(LOGO)?.to_rgba8();
    let icon = egui::IconData {
        rgba: logo.as_raw().clone(),
        width: logo.width(),
        height: logo.height(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1050.0, 650.0])
            .with_min_inner_size([800.0, 540.0])
            .with_icon(icon)
            .with_app_id("SENSOR.Remote"),
        renderer,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "SENSOR Remote Access",
        options,
        Box::new(move |cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.visuals = egui::Visuals::light();
            style.visuals.override_text_color = Some(NAVY);
            style.visuals.panel_fill = PAPER;
            style.visuals.window_fill = Color32::WHITE;
            style.visuals.selection.bg_fill = TEAL;
            style.visuals.widgets.inactive.bg_fill = Color32::WHITE;
            style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
            style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(225, 245, 245);
            style.visuals.widgets.active.bg_fill = Color32::from_rgb(192, 235, 234);
            style.spacing.item_spacing = Vec2::new(12.0, 12.0);
            style.spacing.button_padding = Vec2::new(16.0, 10.0);
            style.spacing.interact_size.y = 36.0;
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(15.0));
            cc.egui_ctx.set_style(style);
            let texture = cc.egui_ctx.load_texture(
                "SENSOR supplied logo",
                egui::ColorImage::from_rgba_unmultiplied(
                    [logo.width() as usize, logo.height() as usize],
                    &logo,
                ),
                egui::TextureOptions::LINEAR,
            );
            let alias = identity
                .alias()
                .map(|a| a.as_str())
                .unwrap_or("")
                .to_owned();
            let mut app = App {
                platform,
                _lock: lock,
                config,
                identity,
                logo: texture,
                page: Page::Connect,
                peer_id: String::new(),
                peer_key: String::new(),
                address: "127.0.0.1:5909".into(),
                bind: "0.0.0.0:5909".into(),
                use_relay: false,
                relay_address: "127.0.0.1:5910".into(),
                relay_key: String::new(),
                use_render: render_mode,
                render_server,
                auto_accept: false,
                allow_unpinned: true,
                confirmed: false,
                receive,
                file: None,
                resume: String::new(),
                alias,
                contact_name: String::new(),
                contacts,
                unattended,
                status: "Offline • No listener started".into(),
                notice: None,
                job: None,
                listener: None,
                listener_enabled: false,
                listener_route: None,
                listener_busy: false,
                listener_registered: false,
                consent: None,
                compose: None,
                compose_listener: false,
                draft: String::new(),
                history: VecDeque::new(),
                progress: None,
                started: None,
                verified_audit: None,
                update_check: None,
                remote_frame: None,
                remote_texture: None,
                uploaded_frame: None,
                remote_displays: Vec::new(),
                remote_format: None,
                remote_cursor: None,
                remote_generation: 0,
                remote_control: false,
                request_clipboard: false,
                actual_size: false,
                fullscreen: false,
                video_profile: sensor_media::VideoProfile::Auto,
                fps_sample: (Instant::now(), 0, 0.0),
                rate_sample: (0, 0, 0.0, 0.0),
                encoder_sample: (0, 0, 0.0),
                remote_source_listener: false,
                remote_buttons: [false; 3],
                remote_focused: false,
                remote_modifiers: [false; 3],
                remote_close_at: None,
            };
            if app.use_render && !app.render_server.trim().is_empty() {
                app.start_render_listener();
            }
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}

struct ConsentPrompt {
    peer: ExpectedPeer,
    mode: Mode,
    reply: SyncSender<bool>,
    opened: Instant,
    source_listener: bool,
}
struct App {
    platform: sensor_desktop::platform::Platform,
    _lock: File,
    config: PathBuf,
    identity: DeviceIdentity,
    logo: egui::TextureHandle,
    page: Page,
    peer_id: String,
    peer_key: String,
    address: String,
    bind: String,
    use_relay: bool,
    relay_address: String,
    relay_key: String,
    use_render: bool,
    render_server: String,
    auto_accept: bool,
    allow_unpinned: bool,
    confirmed: bool,
    receive: PathBuf,
    file: Option<PathBuf>,
    resume: String,
    alias: String,
    contact_name: String,
    contacts: Vec<Contact>,
    unattended: Option<sensor_desktop::unattended::Grant>,
    status: String,
    notice: Option<String>,
    job: Option<Job>,
    listener: Option<Job>,
    listener_enabled: bool,
    listener_route: Option<Route>,
    listener_busy: bool,
    listener_registered: bool,
    consent: Option<ConsentPrompt>,
    compose: Option<SyncSender<Option<String>>>,
    compose_listener: bool,
    draft: String,
    history: VecDeque<(bool, String)>,
    progress: Option<(u64, u64)>,
    started: Option<Instant>,
    verified_audit: Option<String>,
    update_check: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    remote_frame: Option<Arc<DecodedFrame>>,
    remote_texture: Option<egui::TextureHandle>,
    uploaded_frame: Option<Arc<DecodedFrame>>,
    remote_displays: Vec<Display>,
    remote_format: Option<VideoFormat>,
    remote_cursor: Option<(i32, i32, bool)>,
    remote_generation: u64,
    remote_control: bool,
    request_clipboard: bool,
    actual_size: bool,
    fullscreen: bool,
    video_profile: sensor_media::VideoProfile,
    fps_sample: (Instant, u64, f64),
    rate_sample: (u64, u64, f64, f64),
    encoder_sample: (u64, u64, f64),
    remote_source_listener: bool,
    remote_buttons: [bool; 3],
    remote_focused: bool,
    remote_modifiers: [bool; 3],
    remote_close_at: Option<Instant>,
}

fn card(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(12)
        .inner_margin(22)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui);
        });
}
fn title(ui: &mut egui::Ui, eyebrow: &str, heading: &str, description: &str) {
    ui.label(RichText::new(eyebrow).size(11.0).strong().color(TEAL));
    ui.label(RichText::new(heading).size(29.0).strong());
    ui.label(RichText::new(description).color(MUTED));
    ui.add_space(12.0);
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) -> bool {
    ui.label(RichText::new(label).size(12.0).strong());
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY)
            .char_limit(256),
    )
    .changed()
}
fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(Color32::WHITE))
            .fill(NAVY)
            .min_size(Vec2::new(170.0, 42.0)),
    )
    .clicked()
}

fn virtual_key(key: egui::Key) -> Option<u16> {
    use egui::Key;
    Some(match key {
        Key::ArrowDown => 0x28,
        Key::ArrowLeft => 0x25,
        Key::ArrowRight => 0x27,
        Key::ArrowUp => 0x26,
        Key::Escape => 0x1B,
        Key::Tab => 0x09,
        Key::Backspace => 0x08,
        Key::Enter => 0x0D,
        Key::Space => 0x20,
        Key::Insert => 0x2D,
        Key::Delete => 0x2E,
        Key::Home => 0x24,
        Key::End => 0x23,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::Colon => 0xBA,
        Key::Comma => 0xBC,
        Key::Backslash | Key::Pipe => 0xDC,
        Key::Slash | Key::Questionmark => 0xBF,
        Key::Exclamationmark => 0x31,
        Key::OpenBracket | Key::OpenCurlyBracket => 0xDB,
        Key::CloseBracket | Key::CloseCurlyBracket => 0xDD,
        Key::Backtick => 0xC0,
        Key::Minus => 0xBD,
        Key::Period => 0xBE,
        Key::Plus | Key::Equals => 0xBB,
        Key::Semicolon => 0xBA,
        Key::Quote => 0xDE,
        Key::Num0 => 0x30,
        Key::Num1 => 0x31,
        Key::Num2 => 0x32,
        Key::Num3 => 0x33,
        Key::Num4 => 0x34,
        Key::Num5 => 0x35,
        Key::Num6 => 0x36,
        Key::Num7 => 0x37,
        Key::Num8 => 0x38,
        Key::Num9 => 0x39,
        Key::A => 0x41,
        Key::B => 0x42,
        Key::C => 0x43,
        Key::D => 0x44,
        Key::E => 0x45,
        Key::F => 0x46,
        Key::G => 0x47,
        Key::H => 0x48,
        Key::I => 0x49,
        Key::J => 0x4A,
        Key::K => 0x4B,
        Key::L => 0x4C,
        Key::M => 0x4D,
        Key::N => 0x4E,
        Key::O => 0x4F,
        Key::P => 0x50,
        Key::Q => 0x51,
        Key::R => 0x52,
        Key::S => 0x53,
        Key::T => 0x54,
        Key::U => 0x55,
        Key::V => 0x56,
        Key::W => 0x57,
        Key::X => 0x58,
        Key::Y => 0x59,
        Key::Z => 0x5A,
        Key::F1 => 0x70,
        Key::F2 => 0x71,
        Key::F3 => 0x72,
        Key::F4 => 0x73,
        Key::F5 => 0x74,
        Key::F6 => 0x75,
        Key::F7 => 0x76,
        Key::F8 => 0x77,
        Key::F9 => 0x78,
        Key::F10 => 0x79,
        Key::F11 => 0x7A,
        Key::F12 => 0x7B,
        Key::F13 => 0x7C,
        Key::F14 => 0x7D,
        Key::F15 => 0x7E,
        Key::F16 => 0x7F,
        Key::F17 => 0x80,
        Key::F18 => 0x81,
        Key::F19 => 0x82,
        Key::F20 => 0x83,
        Key::F21 => 0x84,
        Key::F22 => 0x85,
        Key::F23 => 0x86,
        Key::F24 => 0x87,
        Key::F25 => 0x88,
        Key::F26 => 0x89,
        Key::F27 => 0x8A,
        Key::F28 => 0x8B,
        Key::F29 => 0x8C,
        Key::F30 => 0x8D,
        Key::F31 => 0x8E,
        Key::F32 => 0x8F,
        Key::F33 => 0x90,
        Key::F34 => 0x91,
        Key::F35 => 0x92,
        Key::BrowserBack => 0xA6,
        Key::Copy | Key::Cut | Key::Paste => return None,
    })
}

impl App {
    fn session_active(&self) -> bool {
        self.job.is_some() || self.listener.is_some()
    }

    fn ready(&self) -> bool {
        self.job.is_none()
            && !self.listener_busy
            && (self.confirmed
                || (self.use_render && self.allow_unpinned && self.peer_key.trim().is_empty()))
            && (peer(&self.peer_id, &self.peer_key).is_ok()
                || (self.allow_unpinned
                    && !self.peer_id.trim().is_empty()
                    && self.peer_key.trim().is_empty()
                    && self.unpinned_peer(&self.peer_id).is_ok()))
    }
    fn ready_host(&self) -> bool {
        self.job.is_none()
            && self.listener.is_none()
            && self.confirmed
            && (self.ready()
                || (self.allow_unpinned
                    && self.peer_key.trim().is_empty()
                    && self.peer_id.trim().is_empty()))
    }
    fn unpinned_peer(&self, id: &str) -> Result<ExpectedPeer, String> {
        settings::known_peer(id, "", &self.contacts)
    }
    fn expected_peer(&self, host_any: bool) -> Result<ExpectedPeer, String> {
        if self.allow_unpinned && self.peer_key.trim().is_empty() {
            if host_any && self.peer_id.trim().is_empty() {
                return Ok(ExpectedPeer {
                    device_id: DeviceId::new(100_000_000)
                        .ok_or_else(|| "Could not create wildcard device ID.".to_owned())?,
                    public_key: [0; 32],
                });
            }
            return self.unpinned_peer(&self.peer_id);
        }
        settings::known_peer(&self.peer_id, &self.peer_key, &self.contacts)
    }
    fn relay_route(&self) -> Result<Route, String> {
        if self.allow_unpinned && self.peer_key.trim().is_empty() {
            return Err(
                "Provisioned relay mode requires both endpoint public keys; use Direct TCP for first connection trust.".into(),
            );
        }
        Ok(Route::Relay(RelayRoute {
            address: self
                .relay_address
                .trim()
                .parse()
                .map_err(|_| "Use a valid relay IP address and port.".to_owned())?,
            relay_key: parse_hex(&self.relay_key)?,
        }))
    }
    fn render_route(&self) -> Result<Route, String> {
        let server = self.render_server.trim();
        sensor_render::websocket_url(server).map_err(|e| e.to_string())?;
        if server.is_empty() {
            return Err(
                "Enter SENSOR_SERVER, for example https://sensor-test.onrender.com.".into(),
            );
        }
        if !(server.starts_with("https://")
            || server.starts_with("http://")
            || server.starts_with("wss://")
            || server.starts_with("ws://"))
        {
            return Err("SENSOR_SERVER must start with https:// or wss://.".into());
        }
        Ok(Route::Render(RenderRoute {
            server: server.to_owned(),
        }))
    }
    fn spawn_render_listener(&mut self, route: Route) {
        if self.listener.is_some() {
            return;
        }
        let peer = match DeviceId::new(100_000_000) {
            Some(device_id) => ExpectedPeer {
                device_id,
                public_key: [0; 32],
            },
            None => {
                self.listener_enabled = false;
                self.notice = Some("Could not create the Render listener identity.".into());
                return;
            }
        };
        self.listener_route = Some(route.clone());
        self.listener_registered = false;
        self.status = "Connecting to SENSOR service...".into();
        self.listener = Some(worker::start(
            Task::Host {
                route,
                receive_dir: self.receive.clone(),
                auto_accept: false,
                accept_any: true,
            },
            self.identity.clone(),
            peer,
            self.config.clone(),
        ));
    }
    fn start_render_listener(&mut self) {
        match self.render_route() {
            Ok(route) => {
                if let Err(error) = settings::save_network(
                    &self.config.join("sensor-network.json"),
                    &self.render_server,
                ) {
                    self.notice = Some(error);
                    return;
                }
                self.listener_enabled = true;
                self.notice = None;
                self.spawn_render_listener(route);
            }
            Err(error) => {
                self.listener_enabled = false;
                self.notice = Some(error);
            }
        }
    }
    fn clear_session_state(&mut self, source_listener: bool) {
        if source_listener {
            self.listener_busy = false;
        }
        if self
            .consent
            .as_ref()
            .is_some_and(|prompt| prompt.source_listener == source_listener)
        {
            if let Some(prompt) = self.consent.take() {
                let _ = prompt.reply.try_send(false);
            }
        }
        if self.compose.is_some() && self.compose_listener == source_listener {
            if let Some(sender) = self.compose.take() {
                let _ = sender.try_send(None);
            }
        }
        if self.remote_source_listener == source_listener {
            self.remote_close_at = None;
            self.remote_frame = None;
            self.remote_texture = None;
            self.uploaded_frame = None;
            self.remote_format = None;
            self.remote_displays.clear();
            self.remote_cursor = None;
            self.remote_focused = false;
            self.remote_buttons = [false; 3];
            self.remote_modifiers = [false; 3];
            self.video_profile = sensor_media::VideoProfile::Auto;
            self.fps_sample = (Instant::now(), 0, 0.0);
            self.rate_sample = (0, 0, 0.0, 0.0);
            self.encoder_sample = (0, 0, 0.0);
            self.remote_control = false;
            self.remote_source_listener = false;
        }
    }
    fn outbound_route(&self) -> Result<Route, String> {
        if self.use_render {
            self.render_route()
        } else if self.use_relay {
            self.relay_route()
        } else {
            Ok(Route::Direct(self.address.trim().parse().map_err(
                |_| "Use a valid remote IP address and port.".to_owned(),
            )?))
        }
    }
    fn listen_route(&self) -> Result<Route, String> {
        if self.use_render {
            self.render_route()
        } else if self.use_relay {
            self.relay_route()
        } else {
            Ok(Route::Direct(self.bind.trim().parse().map_err(|_| {
                "Use a valid local IP address and port.".to_owned()
            })?))
        }
    }
    fn begin(&mut self, task: Task) {
        if matches!(
            &task,
            Task::Host {
                auto_accept: true,
                accept_any: true,
                ..
            }
        ) {
            self.notice = Some(
                "Auto-accept is available only with a pinned public key. Turn off first-connection trust before enabling it.".into(),
            );
            return;
        }
        self.remote_control = matches!(&task, Task::Remote { control: true, .. });
        if matches!(&task, Task::Remote { .. }) {
            self.remote_close_at = None;
            self.page = Page::Remote;
            self.remote_source_listener = false;
            self.remote_frame = None;
            self.remote_texture = None;
            self.remote_displays.clear();
            self.remote_format = None;
            self.remote_cursor = None;
            self.remote_generation = 0;
            self.remote_buttons = [false; 3];
            self.remote_focused = false;
            self.remote_modifiers = [false; 3];
        }
        let host_any = matches!(
            &task,
            Task::Host {
                accept_any: true,
                ..
            }
        );
        let task_ready = if host_any {
            self.ready_host()
        } else {
            self.ready()
        };
        match self.expected_peer(host_any) {
            Ok(peer) if task_ready => {
                self.notice = None;
                self.progress = None;
                self.started = Some(Instant::now());
                self.status = "Starting...".into();
                self.verified_audit = None;
                self.job = Some(worker::start(
                    task,
                    self.identity.clone(),
                    peer,
                    self.config.clone(),
                ));
            }
            Ok(_) => {
                self.notice =
                    Some("Verify the peer key and finish the current session first.".into())
            }
            Err(e) => self.notice = Some(e),
        }
    }
    fn stop(&mut self) {
        self.release_remote_input();
        if let Some(prompt) = self.consent.take() {
            let _ = prompt.reply.try_send(false);
        }
        if let Some(sender) = self.compose.take() {
            let _ = sender.try_send(None);
        }
        if let Some(job) = &self.job {
            job.control.stop();
        }
        self.listener_enabled = false;
        if let Some(listener) = &self.listener {
            listener.control.stop();
        }
        if self.job.is_some() || self.listener.is_some() {
            self.status = "Stopping session...".into();
        }
    }
    fn push_chat(&mut self, local: bool, text: String) {
        if self.history.len() == 200 {
            self.history.pop_front();
        }
        self.history.push_back((local, text));
    }
    fn poll(&mut self) {
        let mut events = Vec::new();
        let mut disconnected_job = false;
        let mut disconnected_listener = false;
        if let Some(job) = &self.job {
            loop {
                match job.events.try_recv() {
                    Ok(event) => events.push((false, event)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected_job = true;
                        break;
                    }
                }
            }
        }
        if let Some(listener) = &self.listener {
            loop {
                match listener.events.try_recv() {
                    Ok(event) => events.push((true, event)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected_listener = true;
                        break;
                    }
                }
            }
        }
        for (source_listener, event) in events {
            match event {
                Event::Online(online) => {
                    if source_listener {
                        self.listener_registered = online;
                        if self.job.is_none() {
                            self.status = if online {
                                "Online • Ready for an attended connection"
                            } else {
                                "Connecting • Internet listener"
                            }
                            .into();
                        }
                    }
                }
                Event::Status(status) => {
                    if !source_listener || self.job.is_none() {
                        self.status = status;
                    }
                }
                Event::Consent(peer, mode, reply) => {
                    if let Err(error) = settings::known_peer(
                        &peer.device_id.to_string(),
                        &hex(&peer.public_key),
                        &self.contacts,
                    ) {
                        let _ = reply.try_send(false);
                        self.notice = Some(format!("Incoming identity check failed: {error}"));
                        continue;
                    }
                    if self.consent.is_some()
                        || (source_listener && (self.job.is_some() || self.listener_busy))
                    {
                        let _ = reply.try_send(false);
                    } else {
                        if source_listener {
                            self.listener_busy = true;
                        }
                        if mode.is_desktop() {
                            self.page = Page::Remote;
                            self.remote_control = mode.controls_input();
                            self.remote_source_listener = source_listener;
                        }
                        if source_listener
                            && self.unattended.as_ref().is_some_and(|grant| {
                                sensor_desktop::updates::now()
                                    .is_ok_and(|now| grant.permits(peer, mode, now))
                            })
                        {
                            self.notice = Some(format!("Unattended signed-in-user session accepted for verified device {}. Stop/disconnect remains available. Clipboard and files are not included.",peer.device_id));
                            let _ = reply.try_send(true);
                            continue;
                        }
                        self.consent = Some(ConsentPrompt {
                            peer,
                            mode,
                            reply,
                            opened: Instant::now(),
                            source_listener,
                        });
                    }
                }
                Event::Chat(text) => {
                    if self.job.is_some() || self.listener.is_some() {
                        self.push_chat(false, text);
                    }
                }
                Event::Compose(sender) => {
                    if self.compose.is_none() && !(source_listener && self.job.is_some()) {
                        self.compose = Some(sender);
                        self.compose_listener = source_listener;
                        self.page = Page::Chat;
                    } else {
                        let _ = sender.try_send(None);
                    }
                }
                Event::RemoteDisplays(displays) => {
                    self.remote_source_listener = source_listener;
                    self.remote_displays = displays;
                }
                Event::RemoteFormat(format) => {
                    self.remote_source_listener = source_listener;
                    self.remote_generation = format.generation;
                    self.remote_format = Some(format);
                }
                Event::RemoteCursor { x, y, visible } => {
                    self.remote_source_listener = source_listener;
                    self.remote_cursor = Some((x, y, visible));
                }
                Event::Progress { id, bytes, total } => {
                    self.progress = Some((bytes, total));
                    if let Some(id) = id {
                        self.resume = hex(&id.0);
                    }
                }
                Event::Finished(result) => {
                    if source_listener {
                        disconnected_listener = false;
                        self.listener_registered = false;
                        let stopped = self
                            .listener
                            .as_ref()
                            .is_some_and(|listener| listener.control.was_stopped_locally());
                        self.listener = None;
                        self.listener_busy = false;
                        self.clear_session_state(true);
                        if self.listener_enabled && !stopped {
                            if let Some(route) = self.listener_route.clone() {
                                self.spawn_render_listener(route);
                            }
                        } else if self.job.is_none() {
                            self.status = "Offline • Render listener stopped".into();
                            if let Err(error) = result {
                                self.notice = Some(format!("Render listener failed: {error}"));
                            }
                        }
                    } else {
                        let stopped = self
                            .job
                            .as_ref()
                            .is_some_and(|job| job.control.was_stopped_locally());
                        self.job = None;
                        self.clear_session_state(false);
                        self.started = None;
                        if self.listener_registered {
                            self.status = "Online • Listener ready".into();
                        } else {
                            self.status = "Offline • Session closed".into();
                        }
                        self.notice = Some(match result {
                            Ok(message) => message,
                            Err(_) if stopped => "Stopped locally. Incomplete files are retained for an explicitly accepted resume.".into(),
                            Err(error) => format!("Session failed: {error}"),
                        });
                    }
                }
            }
        }
        if disconnected_job && self.job.is_some() {
            // A worker panic cannot leave the UI falsely reporting an active session.
            self.job = None;
            self.clear_session_state(false);
            self.started = None;
            self.status = if self.listener_registered {
                "Online • Listener ready".into()
            } else {
                "Offline • Worker stopped unexpectedly".into()
            };
            self.notice = Some("The foreground worker stopped unexpectedly.".into());
        }
        if disconnected_listener && self.listener.is_some() {
            self.listener_registered = false;
            self.listener = None;
            self.listener_busy = false;
            self.clear_session_state(true);
            if self.listener_enabled {
                if let Some(route) = self.listener_route.clone() {
                    self.spawn_render_listener(route);
                }
            } else if self.job.is_none() {
                self.status = "Offline • Render listener stopped".into();
            }
        }
        let control = if self.remote_source_listener {
            self.listener.as_ref().map(|job| job.control.clone())
        } else {
            self.job.as_ref().map(|job| job.control.clone())
        };
        if let Some(control) = control {
            if let Some(frame) = control.latest_frame() {
                self.remote_frame = Some(frame);
            }
        }
    }

    fn active_remote_control(&self) -> Option<worker::Control> {
        if self.remote_source_listener {
            self.listener.as_ref().map(|job| job.control.clone())
        } else {
            self.job.as_ref().map(|job| job.control.clone())
        }
    }

    fn send_remote(&mut self, event: Input) -> bool {
        let Some(control) = self.active_remote_control() else {
            return false;
        };
        let generation = self.remote_generation;
        if control.send_remote(DesktopMessage::Input { generation, event }) {
            true
        } else {
            self.notice = Some(
                "The remote input channel stopped; the session is being closed safely.".into(),
            );
            false
        }
    }
    fn release_remote_input(&mut self) {
        if self.remote_control && (self.job.is_some() || self.listener.is_some()) {
            let _ = self.send_remote(Input::ReleaseAll);
        }
        self.remote_buttons = [false; 3];
        self.remote_focused = false;
        self.remote_modifiers = [false; 3];
    }
    fn select_remote_display(&mut self, index: u32) -> bool {
        let control = if self.remote_source_listener {
            self.listener.as_ref().map(|job| job.control.clone())
        } else {
            self.job.as_ref().map(|job| job.control.clone())
        };
        let Some(control) = control else {
            return false;
        };
        if control.send_remote(DesktopMessage::SelectDisplay(index)) {
            true
        } else {
            self.notice = Some("The remote monitor-selection channel is unavailable.".into());
            false
        }
    }
    fn peer_form(&mut self, ui: &mut egui::Ui) {
        let mut changed = field(ui, "REMOTE DEVICE ID", &mut self.peer_id, "123 456 789");
        if self.use_render {
            ui.label(RichText::new("Enter the ID shown on the other computer. Its owner must approve. Saved device keys are checked automatically.").size(12.0).color(MUTED));
        }
        ui.collapsing("Advanced identity verification", |ui| {
        changed |= field(
            ui,
            "VERIFIED PUBLIC KEY",
            &mut self.peer_key,
            "Paste the other device's 64-character public key",
        );
        if changed {
            self.confirmed = false;
        }
        ui.checkbox(
            &mut self.allow_unpinned,
            "Allow a first connection without a pinned key",
        );
        if self.allow_unpinned {
            ui.label(RichText::new("The receiver will show the signed peer fingerprint for visible approval. Leave the key empty only for first connection; save the displayed key afterward.").size(12.0).color(Color32::from_rgb(167, 99, 26)));
        }
        ui.checkbox(
            &mut self.confirmed,
            if self.allow_unpinned {
                "I understand the first connection must be approved visibly."
            } else {
                "I verified this public key with the other device owner."
            },
        );
        ui.label(RichText::new(if self.allow_unpinned { "A device ID alone does not prove identity; approve only a person you recognize and compare the full fingerprint before future connections." } else { "The ID alone does not prove identity. Compare the full key over a trusted channel." }).size(12.0).color(MUTED));
        });
    }
    fn connect(&mut self, ui: &mut egui::Ui) {
        title(
            ui,
            "YOUR WORKSPACE",
            "Connect with confidence.",
            "Connect by ID. Unknown devices need approval; verified devices can have a saved unattended grant.",
        );
        card(ui, |ui| {
            ui.label(RichText::new("This device").strong());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.identity.device_id().to_string())
                        .size(36.0)
                        .strong(),
                );
                if ui.button("Copy ID").clicked() {
                    ui.ctx().copy_text(self.identity.device_id().to_string());
                }
                if ui.button("Copy public key").clicked() {
                    ui.ctx()
                        .copy_text(hex(&self.identity.keypair().public_key()));
                }
            });
            ui.label(
                RichText::new("Persistent Windows-user identity • Private key protected by DPAPI")
                    .size(12.0)
                    .color(MUTED),
            );
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.add_enabled_ui(self.job.is_none(), |ui| self.peer_form(ui));
            if self.use_render {
                ui.horizontal_wrapped(|ui| {
                    for (label, control) in [
                        ("Control remote desktop", true),
                        ("View remote desktop", false),
                    ] {
                        if primary(ui, label, self.ready()) {
                            match self.outbound_route() {
                                Ok(route) => self.begin(Task::Remote {
                                    route,
                                    control,
                                    clipboard: self.request_clipboard,
                                }),
                                Err(error) => self.notice = Some(error),
                            }
                        }
                    }
                });
            }
            ui.separator();
            ui.collapsing("Network settings", |ui| {
            ui.horizontal(|ui| {
                if ui
                    .radio(!self.use_relay && !self.use_render, "Direct TCP")
                    .clicked()
                {
                    self.use_relay = false;
                    self.use_render = false;
                }
                if ui
                    .radio(self.use_relay && !self.use_render, "Provisioned relay")
                    .clicked()
                {
                    self.use_relay = true;
                    self.use_render = false;
                }
                if ui
                    .radio(self.use_render, "Internet (HTTPS/WSS)")
                    .clicked()
                {
                    self.use_render = true;
                    self.use_relay = false;
                }
            });
            if self.use_render {
                field(
                    ui,
                    "SENSOR SERVER",
                    &mut self.render_server,
                    "https://your-sensor-server.example",
                );
                ui.label(RichText::new("The configured server registers this device by ID and relays end-to-end encrypted sessions. The current hosting allowance is temporary.").size(12.0).color(MUTED));
            } else if self.use_relay {
                field(
                    ui,
                    "RELAY IP : PORT",
                    &mut self.relay_address,
                    "relay.example.com:5910",
                );
                field(
                    ui,
                    "RELAY PUBLIC KEY",
                    &mut self.relay_key,
                    "64-character key from the relay operator",
                );
                ui.label(RichText::new("Both endpoints must use the same relay address/key, and the relay operator must provision both verified endpoint keys.").size(12.0).color(MUTED));
            }
            });
            ui.separator();
            ui.columns(2, |columns| {
                columns[0].label(RichText::new("Connect to a device").strong());
                if self.use_render {
                    columns[0].label(
                        RichText::new("Connect over the Internet by device ID. No LAN address or port forwarding required.")
                            .size(12.0)
                            .color(MUTED),
                    );
                } else if !self.use_relay {
                    field(
                        &mut columns[0],
                        "REMOTE IP : PORT",
                        &mut self.address,
                        "192.168.1.10:5909",
                    );
                } else {
                    columns[0].label(
                        RichText::new("The relay address above is used for this connection.")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
                if primary(&mut columns[0], "Start encrypted chat", self.ready()) {
                    match self.outbound_route() {
                        Ok(route) => self.begin(Task::Chat { route }),
                        Err(error) => self.notice = Some(error),
                    }
                }
                if primary(&mut columns[0], "View remote desktop", self.ready()) {
                    match self.outbound_route() {
                        Ok(route) => self.begin(Task::Remote {
                            route,
                            control: false,
                            clipboard: self.request_clipboard,
                        }),
                        Err(error) => self.notice = Some(error),
                    }
                }
                if columns[0]
                    .add_enabled(
                        self.ready(),
                        egui::Button::new("Control remote desktop")
                            .min_size(Vec2::new(170.0, 42.0)),
                    )
                    .clicked()
                {
                    match self.outbound_route() {
                        Ok(route) => self.begin(Task::Remote {
                            route,
                            control: true,
                            clipboard: self.request_clipboard,
                        }),
                        Err(error) => self.notice = Some(error),
                    }
                }
                columns[0].checkbox(&mut self.request_clipboard, "Request text clipboard sharing");
                columns[0].label(RichText::new("Off by default. The host must approve clipboard access.").size(12.0).color(MUTED));
                columns[1].label(RichText::new("Receive a connection").strong());
                if self.use_render {
                    columns[1].label(
                        RichText::new("Share your ID. Keep SENSOR open and approve only requests you expect.")
                            .size(12.0)
                            .color(MUTED),
                    );
                } else if !self.use_relay {
                    field(
                        &mut columns[1],
                        "LOCAL LISTEN IP : PORT",
                        &mut self.bind,
                        "0.0.0.0:5909",
                    );
                } else {
                    columns[1].label(
                        RichText::new("This endpoint waits for its provisioned relay pair.")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
                columns[1].add_enabled_ui(!self.allow_unpinned && !self.use_render, |ui| {
                    ui.checkbox(
                        &mut self.auto_accept,
                        "Auto-accept this pinned peer while SENSOR is open",
                    );
                });
                columns[1].label(
                    RichText::new(
                        "Use only after verifying the full public key. This is not a Windows service and stops when the app closes.",
                    )
                    .size(12.0)
                    .color(MUTED),
                );
                let can_start_listener = if self.use_render {
                    self.listener.is_none() && self.render_route().is_ok()
                } else {
                    self.ready_host()
                };
                if primary(
                    &mut columns[1],
                    if self.use_render && self.listener_registered {
                        "Online • listening"
                    } else if self.use_render && self.listener.is_some() {
                        "Connecting..."
                    } else {
                        "Start listening"
                    },
                    can_start_listener,
                ) {
                    if self.use_render {
                        self.start_render_listener();
                    } else {
                        match self.listen_route() {
                            Ok(route) => self.begin(Task::Host {
                                route,
                                receive_dir: self.receive.clone(),
                                auto_accept: self.auto_accept,
                                accept_any: self.allow_unpinned && self.peer_key.trim().is_empty(),
                            }),
                            Err(error) => self.notice = Some(error),
                        }
                    }
                }
            });
            ui.label(RichText::new("Keep SENSOR online on both unlocked Windows computers. Approve the first connection, or authorize a verified device in Trusted devices for unattended screen/control.").size(12.0).color(MUTED));
        });
    }
    fn files(&mut self, ui: &mut egui::Ui) {
        title(
            ui,
            "FILE TRANSFER",
            "From your PC. To theirs.",
            "Attended transfer with SHA-256 verification and no overwriting of existing files.",
        );
        card(ui, |ui| {
            ui.add_enabled_ui(self.job.is_none(), |ui| {
                self.peer_form(ui);
                if self.use_render {
                    ui.label(
                        RichText::new("Render HTTPS/WSS is selected on Connect; the SENSOR server above is used for this transfer.")
                            .size(12.0)
                            .color(MUTED),
                    );
                } else if !self.use_relay {
                    field(
                        ui,
                        "REMOTE IP : PORT",
                        &mut self.address,
                        "192.168.1.10:5909",
                    );
                } else {
                    ui.label(
                        RichText::new("Provisioned relay path is selected on Connect.")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button("Choose file...").clicked() {
                        if let Some(file) = rfd::FileDialog::new().pick_file() {
                            self.file = Some(file);
                            self.resume.clear();
                        }
                    }
                    ui.label(
                        self.file
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "No file selected".into()),
                    );
                });
                field(
                    ui,
                    "RESUME TRANSFER ID (OPTIONAL)",
                    &mut self.resume,
                    "Leave empty for a new transfer",
                );
            });
            if primary(ui, "Send file", self.ready() && self.file.is_some()) {
                let resume = if self.resume.trim().is_empty() {
                    Ok(None)
                } else {
                    parse_hex(&self.resume).map(|id| Some(TransferId(id)))
                };
                match (self.outbound_route(), resume, self.file.clone()) {
                    (Ok(route), Ok(resume), Some(file)) => self.begin(Task::Send {
                        route,
                        file,
                        resume,
                    }),
                    (_, Err(e), _) => self.notice = Some(e),
                    (Err(e), _, _) => self.notice = Some(e),
                    _ => self.notice = Some("Choose a file first.".into()),
                }
            }
            if let Some((bytes, total)) = self.progress {
                let ratio = if total == 0 {
                    1.0
                } else {
                    bytes as f32 / total as f32
                };
                ui.add(
                    egui::ProgressBar::new(ratio)
                        .text(format!("{bytes} / {total} bytes acknowledged")),
                );
            }
            ui.label(RichText::new("Maximum offer: 1 GiB. Keep the transfer ID to resume after interruption. A new local acceptance is required.").size(12.0).color(MUTED));
        });
        card(ui, |ui| {
            ui.label(RichText::new("Receive folder on this PC").strong());
            ui.label(self.receive.display().to_string());
            if ui
                .add_enabled(
                    self.job.is_none(),
                    egui::Button::new("Choose receive folder..."),
                )
                .clicked()
            {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_directory(&self.receive)
                    .pick_folder()
                {
                    self.receive = folder;
                }
            }
            ui.label(RichText::new("Start listening from Connect after choosing a folder. Remote names cannot escape this folder.").size(12.0).color(MUTED));
        });
    }
    fn chat(&mut self, ui: &mut egui::Ui) {
        title(ui, "SESSION CHAT", "A conversation. Kept private.", "Messages are encrypted in transit and kept in memory only. This build uses turn-based chat.");
        card(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("chat-history").max_height(330.0).stick_to_bottom(true).show(ui, |ui| {
                ui.set_min_height(270.0);
                if self.history.is_empty() { ui.label(RichText::new("No messages yet. Start a chat from Connect, or accept an incoming chat session.").color(MUTED)); }
                for (local, text) in &self.history {
                    ui.label(RichText::new(if *local { "YOU" } else { "VERIFIED PEER" }).size(10.0).strong().color(TEAL));
                    ui.label(text);
                    ui.add_space(6.0);
                }
            });
            ui.separator();
            ui.add_enabled(
                self.compose.is_some(),
                egui::TextEdit::multiline(&mut self.draft)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .hint_text("Type your message...")
                    .char_limit(sensor_client::MAX_CHAT_BYTES),
            );
            ui.horizontal(|ui| {
                if primary(
                    ui,
                    "Send message",
                    self.compose.is_some()
                        && !self.draft.is_empty()
                        && self.draft.len() <= sensor_client::MAX_CHAT_BYTES,
                ) {
                    if let Some(sender) = self.compose.take() {
                        let text = std::mem::take(&mut self.draft);
                        match sender.try_send(Some(text.clone())) {
                            Ok(()) => self.push_chat(true, text),
                            Err(_) => {
                                self.notice = Some("Session no longer accepts messages.".into())
                            }
                        }
                    }
                }
                ui.label(
                    RichText::new(if self.compose.is_some() {
                        "Your turn to reply"
                    } else {
                        "No reply requested"
                    })
                    .size(12.0)
                    .color(MUTED),
                );
            });
        });
    }
    fn remote(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.remote_close_at.is_none(),
                    egui::Button::new("Disconnect"),
                )
                .clicked()
            {
                self.release_remote_input();
                if self.remote_source_listener {
                    self.stop();
                } else if let Some(job) = &self.job {
                    // Closing an outbound session must not take this device
                    // offline for future incoming connections.
                    // Give the authenticated peer its normal close/ACK path.
                    // The global Stop remains an immediate emergency abort.
                    if job.control.send_remote(DesktopMessage::Close) {
                        self.remote_close_at = Some(Instant::now());
                        self.status = "Disconnecting remote desktop…".into();
                    } else {
                        job.control.stop();
                    }
                }
            }
            if ui
                .button(if self.fullscreen {
                    "Exit full screen"
                } else {
                    "Full screen"
                })
                .on_hover_text("Ctrl+Alt+F")
                .clicked()
            {
                self.release_remote_input();
                self.fullscreen = !self.fullscreen;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            ui.checkbox(&mut self.actual_size, "Actual size");
            if let Some(control) = self.active_remote_control() {
                let mut enabled = control.clipboard_enabled();
                if ui
                    .add_enabled(
                        control.clipboard_allowed(),
                        egui::Checkbox::new(&mut enabled, "Text clipboard"),
                    )
                    .on_hover_text("Requires text clipboard permission approved by the host.")
                    .changed()
                {
                    control.set_clipboard_enabled(enabled);
                }
            }
        });
        // Changing digit counts/congestion labels must not move the video and
        // the remote click target vertically. Small windows can pan this row.
        egui::ScrollArea::horizontal().id_salt("remote-statistics")
            .max_height(24.0).show(ui, |ui| { ui.horizontal(|ui| {
            if let Some(control) = self.active_remote_control() {
                let (frames, bytes, rtt) = control.statistics();
                let elapsed = self.fps_sample.0.elapsed().as_secs_f64();
                if elapsed >= 1.0 {
                    let presented = control.presented_frames();
                    self.rate_sample = (
                        bytes, presented,
                        bytes.saturating_sub(self.rate_sample.0) as f64 * 8.0 / elapsed / 1_000_000.0,
                        presented.saturating_sub(self.rate_sample.1) as f64 / elapsed,
                    );
                    self.fps_sample = (
                        Instant::now(),
                        frames,
                        frames.saturating_sub(self.fps_sample.1) as f64 / elapsed,
                    );
                }
                ui.label(format!(
                    "Decode {:.1} fps · UI {:.1} fps · {:.2} Mbps",
                    self.fps_sample.2,
                    self.rate_sample.3,
                    self.rate_sample.2,
                ));
                if let Some(telemetry) = control.telemetry() {
                    if telemetry.elapsed_ms > self.encoder_sample.0 {
                        self.encoder_sample = (telemetry.elapsed_ms, telemetry.encoded,
                            telemetry.encoded.saturating_sub(self.encoder_sample.1) as f64 * 1000.0 /
                            (telemetry.elapsed_ms - self.encoder_sample.0) as f64);
                    }
                    ui.label(format!("Encode {:.1} fps · skipped {} · pending {} KiB{}",
                        self.encoder_sample.2, telemetry.skipped, telemetry.in_flight_bytes / 1024,
                        if telemetry.congestion { " · reducing load" } else { "" }))
                        .on_hover_text("Measured rates, not target rates. UI counts distinct frames submitted for presentation, not monitor scanout. Skipped counts capture opportunities withheld before encoding; network packet loss is not measurable on TCP.");
                }
                if rtt > 0 {
                    ui.label(format!("RTT {:.1} ms", rtt as f64 / 1000.0));
                }
            }
        }); });
        if self.remote_frame.is_some() && !self.remote_source_listener {
            ui.horizontal_wrapped(|ui| {
                let mut profile = self.video_profile;
                egui::ComboBox::from_id_salt("video-quality")
                    .selected_text(profile.label())
                    .show_ui(ui, |ui| {
                        for choice in [sensor_media::VideoProfile::Auto, sensor_media::VideoProfile::Quality, sensor_media::VideoProfile::Balanced, sensor_media::VideoProfile::Performance] {
                            ui.selectable_value(&mut profile, choice, choice.label());
                        }
                    });
                if profile != self.video_profile {
                    self.release_remote_input();
                    if let Some(control) = self.active_remote_control() {
                        if control.send_remote(DesktopMessage::SelectVideoProfile(profile)) {
                            self.video_profile = profile;
                        }
                    }
                }
                ui.label("Adaptive · up to 1080p60")
                    .on_hover_text("Install this adaptive build on both PCs. Resolution, frame rate and bitrate adjust to measured delivery and codec load. SENSOR_VIDEO_MAX_BITRATE sets the host ceiling (default 16 Mbps). Static screens naturally produce fewer frames.");
            });
        }
        let mode_label = if self.remote_control {
            "control enabled"
        } else {
            "view only"
        };
        if self.remote_frame.is_none() {
            title(
                ui,
                "REMOTE DESKTOP",
                "See the other Windows screen.",
                "Attended H.264 video with explicit local consent.",
            );
        }
        ui.label(
            RichText::new(format!("Permission profile: {mode_label}"))
                .strong()
                .color(TEAL),
        );
        if !self.remote_displays.is_empty() {
            let current = self
                .remote_format
                .as_ref()
                .map(|format| format.display.index)
                .unwrap_or(self.remote_displays[0].index);
            let mut selected = current;
            ui.horizontal(|ui| {
                ui.label(RichText::new("REMOTE MONITOR").size(12.0).strong());
                egui::ComboBox::from_id_salt("remote-monitor")
                    .selected_text(
                        self.remote_displays
                            .iter()
                            .find(|display| display.index == selected)
                            .map(|display| display.name.clone())
                            .unwrap_or_else(|| "Select monitor".into()),
                    )
                    .show_ui(ui, |ui| {
                        for display in &self.remote_displays {
                            ui.selectable_value(&mut selected, display.index, &display.name);
                        }
                    });
            });
            if selected != current {
                self.select_remote_display(selected);
            }
        }
        card(ui, |ui| {
            let format = self.remote_format.clone();
            if let Some(format) = &format {
                ui.label(format!(
                    "{} · capture {}×{} → encoded {}×{} · target {} fps · {} · {}",
                    format.display.name,
                    format.display.width,
                    format.display.height,
                    format.width,
                    format.height,
                    format.fps_limit,
                    format.encoder,
                    if format.hardware {
                        "hardware MFT"
                    } else {
                        "software MFT"
                    }
                ));
            } else {
                ui.label(RichText::new("Waiting for the remote display format…").color(MUTED));
            }
            let Some(frame) = self.remote_frame.clone() else {
                ui.add_space(18.0);
                ui.label(RichText::new("No video frame received yet.").color(MUTED));
                return;
            };
            if self.remote_texture.is_none()
                || !self
                    .uploaded_frame
                    .as_ref()
                    .is_some_and(|previous| Arc::ptr_eq(previous, &frame))
            {
                let dimensions = [frame.width as usize, frame.height as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(dimensions, &frame.rgba);
                if let Some(texture) = &mut self.remote_texture {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.remote_texture = Some(ui.ctx().load_texture(
                        "SENSOR remote desktop",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                self.uploaded_frame = Some(frame.clone());
                if let Some(control) = self.active_remote_control() {
                    control.mark_presented();
                }
            }
            let available = ui.available_size();
            let aspect = frame.width as f32 / frame.height as f32;
            let width = available.x.max(120.0);
            let height = (width / aspect).min(available.y.max(160.0));
            let size = if self.actual_size {
                Vec2::new(frame.width as f32, frame.height as f32) / ui.ctx().pixels_per_point()
            } else {
                Vec2::new(width.min(height * aspect), height)
            };
            let Some(texture) = self.remote_texture.as_ref() else {
                ui.label("The video texture is unavailable.");
                return;
            };
            let response = egui::ScrollArea::both()
                .id_salt("remote-video-pan")
                .scroll_source(if self.remote_control {
                    egui::scroll_area::ScrollSource::SCROLL_BAR
                } else {
                    egui::scroll_area::ScrollSource::ALL
                })
                .auto_shrink([false, false])
                .max_height(available.y.max(160.0))
                .show(ui, |ui| {
                    ui.add(
                        egui::Image::new((texture.id(), size))
                            .fit_to_exact_size(size)
                            .sense(egui::Sense::click_and_drag()),
                    )
                })
                .inner;
            if response.clicked() {
                response.request_focus();
            }
            let display = format
                .as_ref()
                .map(|format| format.display.clone())
                .unwrap_or(Display {
                    index: 0,
                    name: "remote".into(),
                    left: 0,
                    top: 0,
                    width: frame.width,
                    height: frame.height,
                });
            let hovered = response.hovered();
            if self.remote_control {
                if let Some(pos) = response.hover_pos() {
                    let (x, y) = remote_input::pointer_position(
                        pos,
                        response.rect,
                        [display.width, display.height],
                    );
                    if hovered
                        || (self.remote_buttons.iter().any(|held| *held) && response.dragged())
                    {
                        self.send_remote(Input::Move { x, y });
                    }
                }
                // Preserve press/release pairs within one frame: state polling
                // loses rapid clicks completely when the final state is "up".
                let pointer_events = ui.ctx().input(|input| input.events.clone());
                for event in pointer_events {
                    let owns_pointer = match &event {
                        egui::Event::PointerButton { pos, .. } => {
                            ui.ctx().layer_id_at(*pos) == Some(response.layer_id)
                        }
                        _ => false,
                    };
                    if let Some((index, down, packet)) = remote_input::pointer_button(
                        &event,
                        response.rect,
                        response.interact_rect,
                        owns_pointer,
                        [display.width, display.height],
                        &self.remote_buttons,
                    ) {
                        let [position, button] = packet;
                        if self.send_remote(position) && self.send_remote(button) {
                            self.remote_buttons[index] = down;
                        }
                    }
                }
                let focused = response.has_focus();
                let pasting = focused
                    && ui.ctx().input(|input| {
                        input
                            .events
                            .iter()
                            .any(|event| matches!(event, egui::Event::Paste(_)))
                    });
                if pasting {
                    // Ctrl+V is consumed locally by egui as Paste. Do not also
                    // paste unrelated remote clipboard contents or hold Ctrl
                    // while injecting the explicit Unicode payload.
                    let _ = self.send_remote(Input::ReleaseAll);
                    self.remote_buttons = [false; 3];
                    self.remote_modifiers = [false; 3];
                }
                if focused && !pasting {
                    let modifiers = ui.ctx().input(|input| {
                        [
                            input.modifiers.ctrl,
                            input.modifiers.alt,
                            input.modifiers.shift,
                        ]
                    });
                    for (index, down) in modifiers.into_iter().enumerate() {
                        if down != self.remote_modifiers[index] {
                            let virtual_key = [0x11, 0x12, 0x10][index];
                            if self.send_remote(Input::Key { virtual_key, down }) {
                                self.remote_modifiers[index] = down;
                            }
                        }
                    }
                }
                if hovered {
                    let wheels = ui.ctx().input_mut(|input| {
                        input.smooth_scroll_delta = Vec2::ZERO;
                        input.raw_scroll_delta = Vec2::ZERO;
                        input
                            .events
                            .iter()
                            .filter_map(|event| {
                                if let egui::Event::MouseWheel { unit, delta, .. } = event {
                                    Some(remote_input::wheel_delta(*unit, *delta))
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                    });
                    for [horizontal, vertical] in wheels {
                        for (delta, horizontal) in [(horizontal, true), (vertical, false)] {
                            if delta != 0 {
                                self.send_remote(Input::Wheel { delta, horizontal });
                            }
                        }
                    }
                }
                if focused {
                    let events = ui.ctx().input(|input| input.events.clone());
                    for event in events {
                        if let Some(result) = remote_input::text_input(&event) {
                            match result {
                                Ok(packets) => {
                                    for packet in packets {
                                        if !self.send_remote(packet) {
                                            break;
                                        }
                                    }
                                }
                                Err(error) => self.notice = Some(error.into()),
                            }
                            continue;
                        }
                        if let egui::Event::Key {
                            key,
                            pressed,
                            modifiers,
                            ..
                        } = event
                        {
                            if pasting && key == egui::Key::V {
                                continue;
                            }
                            if let Some(key) = virtual_key(key) {
                                let text_key = (0x30..=0x5A).contains(&key)
                                    || key == 0x20
                                    || (0xBA..=0xE2).contains(&key);
                                if !text_key
                                    || modifiers.ctrl
                                    || modifiers.alt
                                    || modifiers.command
                                    || modifiers.mac_cmd
                                {
                                    self.send_remote(Input::Key {
                                        virtual_key: key,
                                        down: pressed,
                                    });
                                }
                            }
                        }
                    }
                }
                if self.remote_focused && !focused {
                    let _ = self.send_remote(Input::ReleaseAll);
                    self.remote_buttons = [false; 3];
                    self.remote_modifiers = [false; 3];
                }
                self.remote_focused = focused;
            }
            if let Some((x, y, visible)) = self.remote_cursor {
                ui.label(format!(
                    "Remote cursor: {} ({x}, {y})",
                    if visible { "visible" } else { "hidden" }
                ));
            }
        });
        if !self.remote_control {
            ui.label(
                RichText::new("View-only session: no keyboard or mouse input is sent.")
                    .color(MUTED),
            );
        } else {
            ui.label(RichText::new("Click the remote screen to focus it. Leaving the viewport releases held keys and buttons.").color(MUTED));
        }
    }
    fn contacts(&mut self, ui: &mut egui::Ui) {
        title(ui, "TRUSTED DEVICES", "Keep your contacts close.", "A local address book. No cloud account, discovery service, or silent trust enrollment.");
        card(ui, |ui| {
            field(
                ui,
                "CONTACT NAME",
                &mut self.contact_name,
                "Office workstation",
            );
            self.peer_form(ui);
            field(
                ui,
                "REMOTE IP : PORT",
                &mut self.address,
                "192.168.1.10:5909",
            );
            if primary(ui, "Save verified contact", self.ready()) {
                let contact = Contact {
                    name: self.contact_name.trim().into(),
                    id: self.peer_id.clone(),
                    key: self.peer_key.clone(),
                    address: self.address.clone(),
                };
                if self
                    .contacts
                    .iter()
                    .any(|c| c.id.replace(' ', "") == contact.id.replace(' ', ""))
                {
                    self.notice = Some("That device already exists. Existing pinned keys are not silently replaced.".into());
                } else {
                    let mut next = self.contacts.clone();
                    next.push(contact);
                    match settings::save(&self.config.join("contacts.json"), &next) {
                        Ok(()) => {
                            self.contacts = next;
                            self.notice = Some("Verified contact saved locally.".into());
                        }
                        Err(error) => self.notice = Some(error),
                    }
                }
            }
        });
        let mut selected = None;
        let mut grant_to = None;
        let mut revoke = false;
        card(ui, |ui| {
            if self.contacts.is_empty() {
                ui.label(RichText::new("No saved contacts.").color(MUTED));
            }
            for (index, contact) in self.contacts.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&contact.name).strong());
                    ui.label(&contact.id);
                    ui.label(&contact.address);
                    if ui
                        .add_enabled(self.job.is_none(), egui::Button::new("Use contact"))
                        .clicked()
                    {
                        selected = Some(index);
                    }
                    if ui
                        .add_enabled(
                            !self.listener_busy && self.job.is_none(),
                            egui::Button::new("Allow unattended view/control for 30 days"),
                        )
                        .clicked()
                    {
                        grant_to = Some(index);
                    }
                });
            }
            ui.label("Unattended access is limited to a verified key and an unlocked, signed-in Windows desktop. It does not enable pre-login, UAC, clipboard or file access. Leave SENSOR running and online.");
            if let Some(grant) = &self.unattended {
                ui.label(format!(
                    "Unattended authority: device {} • expires at Unix UTC {}",
                    grant.device_id, grant.expires_unix
                ));
                if ui
                    .button("Revoke unattended access and stop sessions")
                    .clicked()
                {
                    revoke = true;
                }
            }
        });
        if let Some(index) = grant_to {
            let contact = &self.contacts[index];
            let result = (|| {
                let grant = sensor_desktop::unattended::Grant::new(
                    peer(&contact.id, &contact.key)?,
                    sensor_desktop::updates::now()?,
                )?;
                sensor_desktop::unattended::save(
                    &self.config.join("unattended.bin"),
                    Some(&grant),
                    &UserDpapi,
                    &self.identity.keypair().public_key(),
                )?;
                Ok::<_, String>(grant)
            })();
            match result {
                Ok(grant) => {
                    self.unattended = Some(grant);
                    self.notice = Some("Unattended view/control authorized for this verified device for 30 days. Keep SENSOR open; this is not a boot-time service.".into());
                }
                Err(e) => self.notice = Some(e),
            }
        }
        if revoke {
            // Always stop active work immediately, even if persistence fails.
            self.unattended = None;
            self.stop();
            self.notice = Some(match sensor_desktop::unattended::save(&self.config.join("unattended.bin"), None, &UserDpapi, &self.identity.keypair().public_key()) {Ok(())=>"Unattended authority revoked and current sessions stopped.".into(),Err(e)=>format!("Current authority disabled, but could not save revocation: {e}. Do not restart SENSOR until this is resolved.")});
        }
        if let Some(index) = selected {
            let contact = &self.contacts[index];
            self.peer_id = contact.id.clone();
            self.peer_key = contact.key.clone();
            self.address = contact.address.clone();
            self.confirmed = true;
            self.page = Page::Connect;
        }
    }
    fn diagnostics(&mut self, ui: &mut egui::Ui) {
        title(
            ui,
            "DEVICE & DIAGNOSTICS",
            "See exactly what is running.",
            "Actual local state. No invented online status or performance counters.",
        );
        card(ui, |ui| {
            ui.label(format!(
                "SENSOR Remote Access {} • Windows desktop • Development build",
                env!("CARGO_PKG_VERSION")
            ));
            ui.label(format!("Device ID: {}", self.identity.device_id()));
            ui.label(format!(
                "Public key: {}",
                hex(&self.identity.keypair().public_key())
            ));
            ui.label(format!("Profile directory: {}", self.config.display()));
            ui.label(format!("Network: {}", self.status));
            ui.label("Transport in this window: direct TCP, provisioned relay, or Render HTTPS/WSS. Mutual pinned-key authentication; X25519 + Ed25519 + ChaCha20-Poly1305.");
            ui.label(format!(
                "OS {}.{}.{} • {} runtime build",
                self.platform.os.0,
                self.platform.os.1,
                self.platform.os.2,
                if self.platform.unified_build {
                    "unified Win7/10/11"
                } else {
                    "modern Windows"
                }
            ));
            ui.label(format!(
                "Window rendering: {}. Capture: {}. No browser or WebView.",
                self.platform.renderer_name(),
                self.platform.capture_name()
            ));
            if self.platform.unified_build {
                ui.label("Unified compatibility candidate: a successful build does not certify Windows 7/10. Actual OS acceptance is still required.");
            }
            ui.add_enabled_ui(self.job.is_none(), |ui| {
                field(
                    ui,
                    "LOCAL DEVICE ALIAS",
                    &mut self.alias,
                    "Optional display name",
                );
                if ui.button("Save alias").clicked() {
                    let mut identity = self.identity.clone();
                    let alias = if self.alias.trim().is_empty() {
                        None
                    } else {
                        Some(self.alias.trim().to_owned())
                    };
                    let result = identity.set_alias(alias).and_then(|()| {
                        IdentityFileStore::new(self.config.join("identity.bin"), UserDpapi)
                            .save(&identity)
                    });
                    match result {
                        Ok(()) => {
                            self.identity = identity;
                            self.notice =
                                Some("Alias saved. Device ID and key are unchanged.".into());
                        }
                        Err(error) => self.notice = Some(error.to_string()),
                    }
                }
                if ui.button("Verify signed audit log").clicked() {
                    self.verified_audit = Some(
                        match sensor_audit::verify_file(
                            &self.config.join("audit.jsonl"),
                            &self.identity.keypair().public_key(),
                        ) {
                            Ok(head) => format!(
                                "Verified {} records. Head: {}",
                                head.records,
                                hex(&head.hash)
                            ),
                            Err(error) => {
                                format!("Audit verification unavailable or failed: {error}")
                            }
                        },
                    );
                }
            });
            if let Some(result) = &self.verified_audit {
                ui.label(result);
            }
        });
        card(ui, |ui| {
            ui.label(RichText::new("Publisher-signed updates").strong());
            ui.label("Select a SENSOR release manifest and installer. The compiled publisher key, version, expiry, size and SHA-256 must all match before a verified copy is staged. Nothing is installed automatically.");
            if ui
                .add_enabled(
                    self.update_check.is_none() && !self.listener_busy && self.job.is_none(),
                    egui::Button::new("Verify and stage signed update…"),
                )
                .clicked()
            {
                if let Some(manifest) = rfd::FileDialog::new()
                    .set_title("Select SENSOR signed release manifest")
                    .add_filter("Signed manifest", &["json"])
                    .pick_file()
                {
                    if let Some(installer) = rfd::FileDialog::new()
                        .set_title("Select matching SENSOR installer")
                        .add_filter("Windows installer", &["exe"])
                        .pick_file()
                    {
                        let root = self.config.join("Verified Updates");
                        let (send, receive) = std::sync::mpsc::channel();
                        self.update_check = Some(receive);
                        std::thread::spawn(move || {
                            let result = (|| {
                                let installed =
                                    sensor_desktop::updates::version(env!("CARGO_PKG_VERSION"))?;
                                let (path, release) = sensor_desktop::updates::stage(
                                    &manifest, &installer, &root, installed,
                                )?;
                                Ok(format!("Verified publisher update {}.{}.{}. Close SENSOR, then run this staged installer: {}. Windows Authenticode status is separate.",release.version[0],release.version[1],release.version[2],path.display()))
                            })();
                            let _ = send.send(result);
                        });
                    }
                }
            }
            if self.update_check.is_some() {
                ui.label("Verifying publisher signature and installer bytes…");
            }
        });
        card(ui, |ui| {
            ui.label(RichText::new("Reconnect after Windows sign-in").strong());
            ui.label("Launch this SENSOR executable when this Windows user signs in. It reuses the saved identity and unattended grant. This does not sign you in, unlock Windows, bypass UAC or provide pre-login access.");
            if let Ok(exe) = std::env::current_exe() {
                match sensor_windows::startup::enabled(&exe) {
                    Ok(enabled) => {
                        if ui
                            .button(if enabled {
                                "Disable launch at sign-in"
                            } else {
                                "Enable launch at sign-in"
                            })
                            .clicked()
                        {
                            self.notice = Some(
                                match sensor_windows::startup::configure(&exe, !enabled) {
                                    Ok(()) => {
                                        if enabled {
                                            "SENSOR sign-in startup disabled.".into()
                                        } else {
                                            "SENSOR will launch after this user signs in. Keep this executable at its current location.".into()
                                        }
                                    }
                                    Err(e) => e,
                                },
                            );
                        }
                    }
                    Err(error) => {
                        ui.label(error);
                    }
                }
            }
        });
        card(ui, |ui| {
            ui.label(RichText::new("Release status: not production ready").strong());
            ui.label("Available here: persistent identity, attended Internet screen/control, permission-gated text clipboard, encrypted chat, integrity-checked file transfer with explicit reconnect/resume, local contacts, signed incoming-session audit, and a per-user installer.");
            ui.label("Not implemented: unattended Windows service, UAC/login screen, H.265/AV1, audio, printing, Auto Print, VPN, Authenticode signing, automatic update delivery, durable accounts, NAT traversal, and direct/relay failover. Public routing uses temporary Railway trial infrastructure. Attended DXGI/H.264 view/control requires an unlocked ordinary desktop.");
            ui.label(
                RichText::new("Designed by ENG Mohamed Sayed • SENSOR TECHNOLOGY")
                    .size(12.0)
                    .color(MUTED),
            );
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.page == Page::Remote
            && ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL | egui::Modifiers::ALT, egui::Key::F)
            })
        {
            self.release_remote_input();
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if self.fullscreen
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.release_remote_input();
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        self.poll();
        if self
            .remote_close_at
            .is_some_and(|at| at.elapsed() >= Duration::from_secs(2))
        {
            if let Some(job) = &self.job {
                job.control.stop();
            }
            self.remote_close_at = None;
        }
        if let Some(receiver) = &self.update_check {
            ctx.request_repaint_after(Duration::from_millis(100));
            match receiver.try_recv() {
                Ok(result) => {
                    self.notice = Some(result.unwrap_or_else(|e| format!("Update rejected: {e}")));
                    self.update_check = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.notice = Some(
                        "Update verification worker stopped without a result; nothing installed."
                            .into(),
                    );
                    self.update_check = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.session_active() {
            ctx.request_repaint_after(Duration::from_millis(if self.remote_frame.is_some() {
                16
            } else {
                100
            }));
        }
        egui::TopBottomPanel::bottom("status-bar")
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(12))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    // Always reachable, including short windows and scrolled
                    // pages. Do not hide the local safety control in a sidebar.
                    if self.session_active() && ui.button("Stop / disconnect").clicked() {
                        self.stop();
                    }
                    ui.colored_label(if self.session_active() { TEAL } else { MUTED }, "●");
                    ui.label(RichText::new(&self.status).size(12.0));
                    if let Some(started) = self.started {
                        ui.label(
                            RichText::new(format!("{}s", started.elapsed().as_secs()))
                                .size(12.0)
                                .color(MUTED),
                        );
                    }
                });
            });
        // Give the actual desktop the working area once video arrives.
        // Disconnect and the always-visible safety control remain available.
        if self.page != Page::Remote || self.remote_frame.is_none() {
            egui::SidePanel::left("navigation")
            .exact_width(218.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(20))
            .show(ctx, |ui| {
                ui.add(
                    egui::Image::new((self.logo.id(), self.logo.size_vec2()))
                        .fit_to_exact_size(Vec2::new(178.0, 100.125)),
                );
                ui.label(
                    RichText::new("REMOTE ACCESS")
                        .size(11.0)
                        .strong()
                        .color(MUTED),
                );
                ui.add_space(28.0);
                for (page, label) in [
                    (Page::Connect, "Connect"),
                    (Page::Remote, "Remote desktop"),
                    (Page::Files, "File transfer"),
                    (Page::Chat, "Session chat"),
                    (Page::Contacts, "Trusted devices"),
                    (Page::Diagnostics, "Device & diagnostics"),
                ] {
                    let selected = self.page == page;
                    if ui
                        .add_sized(
                            [178.0, 42.0],
                            egui::Button::new(RichText::new(label).strong().color(if selected {
                                NAVY
                            } else {
                                MUTED
                            }))
                            .fill(if selected {
                                Color32::from_rgb(225, 245, 245)
                            } else {
                                Color32::WHITE
                            })
                            .stroke(Stroke::NONE),
                        )
                        .clicked()
                    {
                        if self.page == Page::Remote && page != Page::Remote {
                            self.release_remote_input();
                        }
                        self.page = page;
                    }
                }
                ui.add_space(28.0);
                ui.separator();
                ui.label(
                    RichText::new("YOU STAY IN CONTROL")
                        .size(10.0)
                        .strong()
                        .color(TEAL),
                );
                ui.label(
                    RichText::new(
                        "Keep SENSOR open and online. Only explicitly authorized verified devices can connect unattended.",
                    )
                    .size(12.0)
                    .color(MUTED),
                );
                ui.add_space(12.0);
                if self.session_active()
                    && ui
                        .add_sized(
                            [178.0, 40.0],
                            egui::Button::new(
                                RichText::new("Stop / disconnect")
                                    .color(Color32::from_rgb(164, 46, 46)),
                            ),
                        )
                        .clicked()
                {
                    self.stop();
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new("Designed by\nENG Mohamed Sayed")
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.label(
                        RichText::new(format!("v{} • Development", env!("CARGO_PKG_VERSION")))
                            .size(11.0)
                            .color(MUTED),
                    );
                });
            });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(PAPER)
                    .inner_margin(if self.page == Page::Remote { 12 } else { 28 }),
            )
            .show(ctx, |ui| {
                if self.page == Page::Remote {
                    ui.add_enabled_ui(self.consent.is_none(), |ui| self.remote(ui));
                    if let Some(notice) = &self.notice {
                        ui.label(notice);
                    }
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("page-scroll")
                    .show(ui, |ui| {
                        ui.add_enabled_ui(self.consent.is_none(), |ui| match self.page {
                            Page::Connect => self.connect(ui),
                            Page::Remote => self.remote(ui),
                            Page::Files => self.files(ui),
                            Page::Chat => self.chat(ui),
                            Page::Contacts => self.contacts(ui),
                            Page::Diagnostics => self.diagnostics(ui),
                        });
                        if let Some(notice) = &self.notice {
                            ui.add_space(8.0);
                            egui::Frame::new()
                                .fill(Color32::from_rgb(232, 239, 245))
                                .corner_radius(8)
                                .inner_margin(16)
                                .show(ui, |ui| {
                                    ui.label(notice);
                                });
                        }
                    });
            });
        let mut answer = None;
        if let Some(prompt) = &self.consent {
            egui::Modal::new(egui::Id::new("incoming-consent")).show(ctx, |ui| {
                ui.set_width(460.0);
                title(ui, "INCOMING REQUEST", "Allow this connection?", "Only grant access if you recognize this person and expect this request.");
                ui.label(RichText::new(format!("Device {}", prompt.peer.device_id)).size(24.0).strong());
                ui.label(format!("Verified key: {}", hex(&prompt.peer.public_key)));
                ui.label(match prompt.mode {
                    Mode::Chat => "Requested access: text chat only. No screen, input, or file access.",
                    Mode::FileTransfer => "Requested access: write transferred files to the selected receive folder. No screen or keyboard/mouse access.",
                    Mode::ScreenView => "Requested access: view this desktop and show the remote pointer. No keyboard or mouse control.",
                    Mode::RemoteControl => "Requested access: view this desktop and control keyboard/mouse through attended input injection.",
                    Mode::ScreenViewClipboard => "Requested access: view this desktop and share text clipboard in both directions (up to 64 KiB). No keyboard/mouse control. Clipboard can be disabled during the session.",
                    Mode::RemoteControlClipboard => "Requested access: view/control this desktop AND share text clipboard in both directions (up to 64 KiB). Clipboard can be disabled during the session.",
                });
                if matches!(prompt.mode, Mode::FileTransfer) { ui.label(format!("Receive folder: {}", self.receive.display())); }
                ui.label(format!("Request expires in {} seconds", 120_u64.saturating_sub(prompt.opened.elapsed().as_secs())));
                ui.horizontal(|ui| {
                    if ui.button("Reject").clicked() { answer = Some(false); }
                    if primary(ui, "Accept this session", true) { answer = Some(true); }
                });
            });
            if prompt.opened.elapsed() >= Duration::from_secs(120) {
                answer = Some(false);
            }
        }
        if let Some(answer) = answer {
            if let Some(prompt) = self.consent.take() {
                if prompt.reply.try_send(answer).is_err() {
                    self.notice = Some("Request expired; no permission was granted.".into());
                }
            }
        }
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.stop();
    }
}
