use eframe::egui;
use input_core::config::FileConfig;
use input_core::ipc::MessageBus;
use input_core::overlay::OverlayConfig;
use overlay_wayland::WaylandRenderer;
use input_core::traits::OverlayRenderer;
use tracing::{error, info};

fn parse_log_level() -> String {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--trace") {
        return "trace".into();
    }
    if args.iter().any(|a| a == "--debug") {
        return "debug".into();
    }
    std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".into())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(parse_log_level())
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let settings_mode = args.iter().any(|a| a == "--settings");

    let file_config = FileConfig::load();
    let overlay_config = file_config.to_overlay_config();

    if settings_mode {
        run_settings_gui(file_config);
    } else {
        run_overlay(overlay_config);
    }
}

// ── Overlay mode (default) ──────────────────────────────────────

fn run_overlay(config: OverlayConfig) {
    info!("Starting EchoInput overlay");

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    let bus = MessageBus::new(4096);

    rt.block_on(async {
        let mut renderer = WaylandRenderer::new(bus.clone());

        if let Err(e) = renderer.start(config).await {
            error!("Failed to start overlay: {}", e);
            return;
        }

        // The Wayland renderer handles keyboard capture via wl_keyboard
        // and overlay rendering internally. Just keep the runtime alive.
        tokio::signal::ctrl_c().await.ok();
        let _ = renderer.stop().await;
    });
}

// ── Settings GUI mode ──────────────────────────────────────────

fn run_settings_gui(initial_config: FileConfig) {
    info!("Starting EchoInput settings GUI");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 500.0])
            .with_min_inner_size([350.0, 400.0])
            .with_title("EchoInput Settings"),
        ..Default::default()
    };

    eframe::run_native(
        "EchoInput Settings",
        options,
        Box::new(move |_cc| Ok(Box::new(SettingsApp::new(initial_config)))),
    )
    .unwrap();
}

struct SettingsApp {
    config: FileConfig,
    position_index: usize,
    scale_index: usize,
    theme_index: usize,
    save_status: String,
}

const POSITIONS: &[&str] = &[
    "BottomCenter",
    "TopLeft",
    "TopRight",
    "TopCenter",
    "BottomLeft",
    "BottomRight",
    "Center",
];

const SCALES: &[&str] = &["Small", "Medium", "Large", "ExtraLarge"];
const THEMES: &[&str] = &["Dark", "Light", "System"];

impl SettingsApp {
    fn new(config: FileConfig) -> Self {
        let position_index = config
            .position
            .as_deref()
            .and_then(|p| POSITIONS.iter().position(|&s| s == p))
            .unwrap_or(0);
        let scale_index = config
            .scale
            .as_deref()
            .and_then(|s| SCALES.iter().position(|&x| x == s))
            .unwrap_or(1);
        let theme_index = config
            .theme
            .as_deref()
            .and_then(|t| THEMES.iter().position(|&x| x == t))
            .unwrap_or(0);

        Self {
            config,
            position_index,
            scale_index,
            theme_index,
            save_status: String::new(),
        }
    }

    fn sync_to_config(&mut self) {
        self.config.position = Some(POSITIONS[self.position_index].into());
        self.config.scale = Some(SCALES[self.scale_index].into());
        self.config.theme = Some(THEMES[self.theme_index].into());
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("EchoInput Settings");
            ui.separator();

            ui.label("Position:");
            egui::ComboBox::from_id_salt("position")
                .selected_text(POSITIONS[self.position_index])
                .show_ui(ui, |ui| {
                    for (i, &pos) in POSITIONS.iter().enumerate() {
                        ui.selectable_value(&mut self.position_index, i, pos);
                    }
                });

            ui.add_space(8.0);

            ui.label("Scale:");
            egui::ComboBox::from_id_salt("scale")
                .selected_text(SCALES[self.scale_index])
                .show_ui(ui, |ui| {
                    for (i, &scale) in SCALES.iter().enumerate() {
                        ui.selectable_value(&mut self.scale_index, i, scale);
                    }
                });

            ui.add_space(8.0);

            let mut opacity = self.config.opacity.unwrap_or(0.9) as f64;
            ui.label(format!("Opacity: {:.0}%", opacity * 100.0));
            ui.add(egui::Slider::from_get_set(0.1..=1.0, |v| {
                if let Some(new_val) = v {
                    opacity = new_val;
                }
                opacity
            })
            .suffix("%")
            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            self.config.opacity = Some(opacity as f32);

            ui.add_space(8.0);

            let mut duration_ms = self.config.display_duration_ms.unwrap_or(1500) as f32;
            ui.label(format!("Display Duration: {}ms", duration_ms as u64));
            ui.add(egui::Slider::new(&mut duration_ms, 500.0..=5000.0).suffix("ms"));
            self.config.display_duration_ms = Some(duration_ms as u64);

            ui.add_space(8.0);

            let mut hist = self.config.history_length.unwrap_or(3) as f32;
            ui.label(format!("History Length: {}", hist as usize));
            ui.add(egui::Slider::new(&mut hist, 1.0..=10.0).step_by(1.0));
            self.config.history_length = Some(hist as usize);

            ui.add_space(8.0);

            ui.label("Theme:");
            egui::ComboBox::from_id_salt("theme")
                .selected_text(THEMES[self.theme_index])
                .show_ui(ui, |ui| {
                    for (i, &theme) in THEMES.iter().enumerate() {
                        ui.selectable_value(&mut self.theme_index, i, theme);
                    }
                });

            ui.add_space(8.0);

            ui.label("Monitor (leave empty for default):");
            let mut monitor = self.config.monitor.clone().unwrap_or_default();
            ui.text_edit_singleline(&mut monitor);
            self.config.monitor = if monitor.is_empty() {
                None
            } else {
                Some(monitor)
            };

            ui.add_space(16.0);
            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.sync_to_config();
                    match self.config.save() {
                        Ok(()) => {
                            self.save_status = "Settings saved!".into();
                        }
                        Err(e) => {
                            self.save_status = format!("Error: {}", e);
                        }
                    }
                }

                if ui.button("Save & Close").clicked() {
                    self.sync_to_config();
                    if self.config.save().is_ok() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            });

            if !self.save_status.is_empty() {
                ui.label(&self.save_status);
            }

            ui.add_space(8.0);

            ui.collapsing("Config file location", |ui| {
                if let Some(path) = FileConfig::config_path() {
                    ui.label(path.display().to_string());
                } else {
                    ui.label("Could not determine config path");
                }
            });
        });
    }
}
