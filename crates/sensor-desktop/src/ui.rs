use eframe::egui::{self, Color32, FontId, RichText, Stroke, Vec2};
use sensor_client::Mode;
use sensor_desktop::{
    hex, parse_hex, peer,
    settings::{self, Contact},
    worker::{self, Event, Job, Task},
};
use sensor_files::TransferId;
use sensor_identity::{DeviceIdentity, IdentityFileStore};
use sensor_media::{DecodedFrame, DesktopMessage, Display, Input, MouseButton, VideoFormat};
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
    let receive = config.join("Received Files");
    std::fs::create_dir_all(&receive)?;
    let logo = image::load_from_memory(LOGO)?.to_rgba8();
    let icon = egui::IconData {
        rgba: logo.as_raw().clone(),
        width: logo.width(),
        height: logo.height(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 790.0])
            .with_min_inner_size([880.0, 660.0])
            .with_icon(icon)
            .with_app_id("SENSOR.Remote"),
        renderer: eframe::Renderer::Wgpu,
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
            Ok(Box::new(App {
                _lock: lock,
                config,
                identity,
                logo: texture,
                page: Page::Connect,
                peer_id: String::new(),
                peer_key: String::new(),
                address: "127.0.0.1:5909".into(),
                bind: "127.0.0.1:5909".into(),
                confirmed: false,
                receive,
                file: None,
                resume: String::new(),
                alias,
                contact_name: String::new(),
                contacts,
                status: "Offline • No listener started".into(),
                notice: None,
                job: None,
                consent: None,
                compose: None,
                draft: String::new(),
                history: VecDeque::new(),
                progress: None,
                started: None,
                verified_audit: None,
                remote_frame: None,
                remote_texture: None,
                remote_displays: Vec::new(),
                remote_format: None,
                remote_cursor: None,
                remote_generation: 0,
                remote_control: false,
                remote_buttons: [false; 3],
                remote_focused: false,
                remote_modifiers: [false; 3],
            }))
        }),
    )?;
    Ok(())
}

struct ConsentPrompt {
    peer: ExpectedPeer,
    mode: Mode,
    reply: SyncSender<bool>,
    opened: Instant,
}
struct App {
    _lock: File,
    config: PathBuf,
    identity: DeviceIdentity,
    logo: egui::TextureHandle,
    page: Page,
    peer_id: String,
    peer_key: String,
    address: String,
    bind: String,
    confirmed: bool,
    receive: PathBuf,
    file: Option<PathBuf>,
    resume: String,
    alias: String,
    contact_name: String,
    contacts: Vec<Contact>,
    status: String,
    notice: Option<String>,
    job: Option<Job>,
    consent: Option<ConsentPrompt>,
    compose: Option<SyncSender<Option<String>>>,
    draft: String,
    history: VecDeque<(bool, String)>,
    progress: Option<(u64, u64)>,
    started: Option<Instant>,
    verified_audit: Option<String>,
    remote_frame: Option<Arc<DecodedFrame>>,
    remote_texture: Option<egui::TextureHandle>,
    remote_displays: Vec<Display>,
    remote_format: Option<VideoFormat>,
    remote_cursor: Option<(i32, i32, bool)>,
    remote_generation: u64,
    remote_control: bool,
    remote_buttons: [bool; 3],
    remote_focused: bool,
    remote_modifiers: [bool; 3],
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
    fn ready(&self) -> bool {
        self.job.is_none() && self.confirmed && peer(&self.peer_id, &self.peer_key).is_ok()
    }
    fn begin(&mut self, task: Task) {
        self.remote_control = matches!(&task, Task::Remote { control: true, .. });
        if matches!(&task, Task::Remote { .. }) {
            self.page = Page::Remote;
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
        match peer(&self.peer_id, &self.peer_key) {
            Ok(peer) if self.ready() => {
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
        let mut disconnected = false;
        if let Some(job) = &self.job {
            loop {
                match job.events.try_recv() {
                    Ok(event) => events.push(event),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for event in events {
            match event {
                Event::Status(status) => self.status = status,
                Event::Consent(peer, mode, reply) => {
                    if matches!(mode, Mode::ScreenView | Mode::RemoteControl) {
                        self.page = Page::Remote;
                        self.remote_control = matches!(mode, Mode::RemoteControl);
                    }
                    self.consent = Some(ConsentPrompt {
                        peer,
                        mode,
                        reply,
                        opened: Instant::now(),
                    })
                }
                Event::Chat(text) => self.push_chat(false, text),
                Event::Compose(sender) => {
                    self.compose = Some(sender);
                    self.page = Page::Chat;
                }
                Event::RemoteDisplays(displays) => self.remote_displays = displays,
                Event::RemoteFormat(format) => {
                    self.remote_generation = format.generation;
                    self.remote_format = Some(format);
                }
                Event::RemoteCursor { x, y, visible } => {
                    self.remote_cursor = Some((x, y, visible));
                }
                Event::Progress { id, bytes, total } => {
                    self.progress = Some((bytes, total));
                    if let Some(id) = id {
                        self.resume = hex(&id.0);
                    }
                }
                Event::Finished(result) => {
                    let stopped = self.job.as_ref().is_some_and(|j| j.control.is_stopped());
                    self.job = None;
                    self.consent = None;
                    self.compose = None;
                    self.started = None;
                    self.remote_frame = None;
                    self.remote_texture = None;
                    self.remote_format = None;
                    self.remote_displays.clear();
                    self.remote_focused = false;
                    self.remote_buttons = [false; 3];
                    self.remote_modifiers = [false; 3];
                    self.status = "Offline • Session closed".into();
                    self.notice = Some(match result {
                        Ok(message) => message,
                        Err(_) if stopped => "Stopped locally. Incomplete files are retained for an explicitly accepted resume.".into(),
                        Err(error) => format!("Session failed: {error}"),
                    });
                }
            }
        }
        if disconnected && self.job.is_some() {
            // A worker panic cannot leave the UI falsely reporting an active session.
            self.stop();
            self.job = None;
            self.started = None;
            self.status = "Offline • Worker stopped unexpectedly".into();
        }
        if let Some(job) = &self.job {
            if let Some(frame) = job.control.latest_frame() {
                self.remote_frame = Some(frame);
            }
        }
    }

    fn send_remote(&mut self, event: Input) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        let generation = self.remote_generation;
        if job
            .control
            .send_remote(DesktopMessage::Input { generation, event })
        {
            true
        } else {
            self.notice = Some(
                "The remote input channel stopped; the session is being closed safely.".into(),
            );
            false
        }
    }
    fn release_remote_input(&mut self) {
        if self.remote_control && self.job.is_some() {
            let _ = self.send_remote(Input::ReleaseAll);
        }
        self.remote_buttons = [false; 3];
        self.remote_focused = false;
        self.remote_modifiers = [false; 3];
    }
    fn select_remote_display(&mut self, index: u32) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        if job
            .control
            .send_remote(DesktopMessage::SelectDisplay(index))
        {
            true
        } else {
            self.notice = Some("The remote monitor-selection channel is unavailable.".into());
            false
        }
    }
    fn peer_form(&mut self, ui: &mut egui::Ui) {
        let mut changed = field(ui, "REMOTE DEVICE ID", &mut self.peer_id, "123 456 789");
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
            &mut self.confirmed,
            "I verified this public key with the other device owner.",
        );
        ui.label(RichText::new("The ID alone does not prove identity. Compare the full key over a trusted channel.").size(12.0).color(MUTED));
    }
    fn connect(&mut self, ui: &mut egui::Ui) {
        title(
            ui,
            "YOUR WORKSPACE",
            "Connect with confidence.",
            "Choose a trusted device. Every incoming session needs your approval.",
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
            ui.separator();
            ui.columns(2, |columns| {
                columns[0].label(RichText::new("Connect to a device").strong());
                field(
                    &mut columns[0],
                    "REMOTE IP : PORT",
                    &mut self.address,
                    "192.168.1.10:5909",
                );
                if primary(&mut columns[0], "Start encrypted chat", self.ready()) {
                    match self.address.parse() {
                        Ok(address) => self.begin(Task::Chat { address }),
                        Err(_) => {
                            self.notice = Some("Use a valid remote IP address and port.".into())
                        }
                    }
                }
                if primary(&mut columns[0], "View remote desktop", self.ready()) {
                    match self.address.parse() {
                        Ok(address) => self.begin(Task::Remote {
                            address,
                            control: false,
                        }),
                        Err(_) => {
                            self.notice = Some("Use a valid remote IP address and port.".into())
                        }
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
                    match self.address.parse() {
                        Ok(address) => self.begin(Task::Remote {
                            address,
                            control: true,
                        }),
                        Err(_) => {
                            self.notice = Some("Use a valid remote IP address and port.".into())
                        }
                    }
                }
                columns[1].label(RichText::new("Receive a connection").strong());
                field(
                    &mut columns[1],
                    "LOCAL LISTEN IP : PORT",
                    &mut self.bind,
                    "127.0.0.1:5909",
                );
                if primary(&mut columns[1], "Start listening", self.ready()) {
                    match self.bind.parse() {
                        Ok(address) => self.begin(Task::Host {
                            address,
                            receive_dir: self.receive.clone(),
                        }),
                        Err(_) => {
                            self.notice = Some("Use a valid local IP address and port.".into())
                        }
                    }
                }
            });
            ui.label(RichText::new("Remote desktop is attended: the other Windows user must approve View or Control. 127.0.0.1 is this PC only; choose a reachable LAN address for another PC.").size(12.0).color(MUTED));
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
                field(
                    ui,
                    "REMOTE IP : PORT",
                    &mut self.address,
                    "192.168.1.10:5909",
                );
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
                match (self.address.parse(), resume, self.file.clone()) {
                    (Ok(address), Ok(resume), Some(file)) => self.begin(Task::Send {
                        address,
                        file,
                        resume,
                    }),
                    (_, Err(e), _) => self.notice = Some(e),
                    _ => self.notice = Some("Use a valid remote IP address and port.".into()),
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
        let mode_label = if self.remote_control {
            "control enabled"
        } else {
            "view only"
        };
        title(
            ui,
            "REMOTE DESKTOP",
            "See the other Windows screen.",
            "Attended H.264 video with explicit local consent.",
        );
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
                    "{} • {}×{} stream • {} fps limit • {} • {}",
                    format.display.name,
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
            let available = ui.available_size();
            let aspect = frame.width as f32 / frame.height as f32;
            let width = available.x.max(120.0);
            let height = (width / aspect).min(available.y.max(160.0));
            let size = Vec2::new(width.min(height * aspect), height);
            let response = ui.add(
                egui::Image::new((
                    self.remote_texture.as_ref().expect("texture created").id(),
                    size,
                ))
                .fit_to_exact_size(size)
                .sense(egui::Sense::click_and_drag()),
            );
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
                    let x = ((pos.x - response.rect.left()) / response.rect.width()
                        * display.width as f32)
                        .floor()
                        .clamp(0.0, display.width.saturating_sub(1) as f32)
                        as u32;
                    let y = ((pos.y - response.rect.top()) / response.rect.height()
                        * display.height as f32)
                        .floor()
                        .clamp(0.0, display.height.saturating_sub(1) as f32)
                        as u32;
                    if hovered {
                        self.send_remote(Input::Move { x, y });
                    }
                }
                let pointer = ui.ctx().input(|input| {
                    [
                        input.pointer.primary_down(),
                        input.pointer.secondary_down(),
                        input.pointer.middle_down(),
                    ]
                });
                for (index, down) in pointer.into_iter().enumerate() {
                    let allowed = hovered || self.remote_buttons[index];
                    if allowed && down != self.remote_buttons[index] {
                        let button = match index {
                            0 => MouseButton::Left,
                            1 => MouseButton::Right,
                            _ => MouseButton::Middle,
                        };
                        if self.send_remote(Input::Button { button, down }) {
                            self.remote_buttons[index] = down;
                        }
                    }
                }
                let focused = response.has_focus();
                if focused {
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
                let scroll = ui.ctx().input(|input| input.smooth_scroll_delta);
                if hovered && (scroll.x != 0.0 || scroll.y != 0.0) {
                    if scroll.y != 0.0 {
                        self.send_remote(Input::Wheel {
                            delta: (scroll.y * 120.0).round().clamp(-12_000.0, 12_000.0) as i32,
                            horizontal: false,
                        });
                    }
                    if scroll.x != 0.0 {
                        self.send_remote(Input::Wheel {
                            delta: (scroll.x * 120.0).round().clamp(-12_000.0, 12_000.0) as i32,
                            horizontal: true,
                        });
                    }
                }
                if focused {
                    let events = ui.ctx().input(|input| input.events.clone());
                    for event in events {
                        match event {
                            egui::Event::Text(text) if !text.is_empty() => {
                                self.send_remote(Input::Text(text));
                            }
                            egui::Event::Key {
                                key,
                                pressed,
                                repeat,
                                modifiers,
                                ..
                            } if !repeat || !pressed => {
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
                            _ => {}
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
                });
            }
        });
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
            ui.label("Transport in this window: direct TCP. Mutual pinned-key authentication; X25519 + Ed25519 + ChaCha20-Poly1305.");
            ui.label("Window rendering: native egui / wgpu. No browser or WebView.");
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
            ui.label(RichText::new("Release status: not production ready").strong());
            ui.label("Available here: persistent identity, attended encrypted chat, integrity-checked file send/receive, explicit reconnect-and-resume, local contacts, signed incoming-session audit.");
            ui.label("Not implemented: remote screen/control, hardware video pipeline, unattended service, UAC/login screen, audio, clipboard, printing, Auto Print, VPN, signed installers/updates. Relay is currently library-only, not selectable in this window. No Internet ID lookup or NAT traversal.");
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
        self.poll();
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        egui::TopBottomPanel::bottom("status-bar")
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(12))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(if self.job.is_some() { TEAL } else { MUTED }, "●");
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
                        "Nothing listens until you start it. Closing SENSOR ends the session.",
                    )
                    .size(12.0)
                    .color(MUTED),
                );
                ui.add_space(12.0);
                if self.job.is_some()
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
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(PAPER).inner_margin(28))
            .show(ctx, |ui| {
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
