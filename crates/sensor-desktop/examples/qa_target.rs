//! Visible synthetic native fixture, excluded from the shipped application.
#[cfg(windows)]
fn main() -> eframe::Result {
    use eframe::egui;
    struct Target {
        path: std::path::PathBuf,
        text: String,
        frames: u64,
        clicked: u64,
        wheel: bool,
    }
    impl eframe::App for Target {
        fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
            self.frames += 1;
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.style_mut().override_font_id = Some(egui::FontId::proportional(22.0));
                ui.heading("SENSOR · real remote-input test");
                ui.label("This window contains synthetic test data only.");
                let pulse = if (self.frames / 8).is_multiple_of(2) { egui::Color32::from_rgb(10,190,180) } else { egui::Color32::from_rgb(30,60,95) };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(360.0,100.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 8.0, pulse);
                let moving = egui::Rect::from_min_size(rect.min + egui::vec2((self.frames % 330) as f32, 25.0), egui::vec2(20.0, 50.0));
                ui.painter().rect_filled(moving, 3.0, egui::Color32::WHITE);
                ui.label("Remote test input:");
                let response = ui.add(egui::TextEdit::singleline(&mut self.text).desired_width(360.0));
                // Explicit cloud fixture only: focus its own editable surface.
                // No OS security dialog or other application's input is touched.
                if self.frames == 1 && std::env::var_os("SENSOR_QA_CLOUD_FOCUS").is_some() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    response.request_focus();
                }
                if response.clicked() { self.clicked += 1; }
                self.wheel |= ctx.input(|i| i.raw_scroll_delta.y != 0.0);
                ui.label(format!("Verified clicks: {} | Wheel received: {} | Exact text: {}", self.clicked, self.wheel, self.text == "SENSOR QA مرحبا 123"));
                let viewport = ctx.input(|i| i.viewport().clone());
                if let Some(inner) = viewport.inner_rect {
                    let position = (inner.min.to_vec2() + response.rect.center().to_vec2()) * ctx.pixels_per_point();
                    let snapshot = serde_json::json!({
                        "focused": viewport.focused == Some(true), "text_focused": response.has_focus(),
                        "text_center": [position.x, position.y], "typed_expected": self.text == "SENSOR QA مرحبا 123",
                        "clicks": self.clicked, "wheel": self.wheel, "painted_frames": self.frames,
                    });
                    if let Some(parent) = self.path.parent() {
                        if let Ok(mut file) = tempfile::NamedTempFile::new_in(parent) {
                            if serde_json::to_writer(file.as_file_mut(), &snapshot).is_ok() { let _ = file.persist(&self.path); }
                        }
                    }
                }
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(
                if std::env::var_os("SENSOR_QA_CLOUD_FOCUS").is_some() {
                    16
                } else {
                    66
                },
            ));
        }
    }
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("Usage: qa_target.exe <temporary-state.json>");
        return Ok(());
    };
    eframe::run_native(
        "SENSOR QA target — synthetic data",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([440.0, 340.0])
                .with_maximized(std::env::var_os("SENSOR_QA_CLOUD_FOCUS").is_some())
                .with_position([120.0, 120.0]),
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(move |_| {
            Ok(Box::new(Target {
                path: path.into(),
                text: String::new(),
                frames: 0,
                clicked: 0,
                wheel: false,
            }))
        }),
    )
}
#[cfg(not(windows))]
fn main() {
    eprintln!("The native QA target requires Windows.");
}
