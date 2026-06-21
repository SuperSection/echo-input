use eframe::egui;
use input_core::config::FileConfig;
use input_core::events::{ModifierState, ProcessedEvent, ShortcutCombo};
use input_core::ipc::MessageBus;
use input_core::overlay::{DisplayEvent, OverlayConfig};
use input_core::presets::ThemePreset;
use input_core::processor::DefaultEventProcessor;
use input_core::traits::{EventProcessor, KeyboardCaptureProvider, OverlayRenderer, ProcessorConfig};
use overlay_wayland::WaylandRenderer;
use platform_linux::evdev_capture::EvdevCapture;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info, warn};

// ── Theme Colors ───────────────────────────────────────────────

#[derive(Clone)]
struct Theme {
    bg: egui::Color32,
    bg_card: egui::Color32,
    bg_hover: egui::Color32,
    bg_input: egui::Color32,
    accent: egui::Color32,
    text: egui::Color32,
    text_dim: egui::Color32,
    text_muted: egui::Color32,
    border: egui::Color32,
    separator: egui::Color32,
    tab_active: egui::Color32,
    tab_inactive: egui::Color32,
    success: egui::Color32,
}

impl Theme {
    fn dark() -> Self {
        Self {
            bg: egui::Color32::from_rgb(32, 33, 36),
            bg_card: egui::Color32::from_rgb(45, 46, 50),
            bg_hover: egui::Color32::from_rgb(55, 56, 62),
            bg_input: egui::Color32::from_rgb(38, 39, 43),
            accent: egui::Color32::from_rgb(88, 166, 255),
            text: egui::Color32::from_rgb(232, 232, 232),
            text_dim: egui::Color32::from_rgb(180, 180, 185),
            text_muted: egui::Color32::from_rgb(120, 120, 128),
            border: egui::Color32::from_rgb(55, 56, 62),
            separator: egui::Color32::from_rgb(50, 51, 56),
            tab_active: egui::Color32::from_rgb(88, 166, 255),
            tab_inactive: egui::Color32::from_rgb(140, 140, 148),
            success: egui::Color32::from_rgb(76, 175, 80),
        }
    }
}

fn apply_theme(ctx: &egui::Context, theme: &Theme) {
    let mut style = (*ctx.style()).clone();

    // Visuals
    let visuals = &mut style.visuals;
    visuals.dark_mode = true;
    visuals.override_text_color = Some(theme.text);
    visuals.widgets.noninteractive.bg_fill = theme.bg_card;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, theme.text_dim);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, theme.border);
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);

    visuals.widgets.inactive.bg_fill = theme.bg_input;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, theme.text);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, theme.border);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);

    visuals.widgets.hovered.bg_fill = theme.bg_hover;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, theme.text);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, theme.accent);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);

    visuals.widgets.active.bg_fill = theme.accent;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);

    visuals.selection.bg_fill = theme.accent.linear_multiply(0.3);
    visuals.selection.stroke = egui::Stroke::new(1.0, theme.accent);

    visuals.extreme_bg_color = theme.bg;
    visuals.faint_bg_color = theme.bg_card;
    visuals.striped = false;

    // Sliders
    visuals.slider_trailing_fill = true;

    // Spacing
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.indent = 18.0;
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(16);

    ctx.set_style(style);
}

fn parse_log_level() -> String {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--trace") {
        return "trace".into();
    }
    if args.iter().any(|a| a == "--debug") {
        return "debug".into();
    }
    std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into())
}

fn print_help() {
    println!("EchoInput — keyboard visualization overlay");
    println!();
    println!("USAGE:");
    println!("  echoinput                 Run the overlay (default)");
    println!("  echoinput --settings      Open settings GUI");
    println!("  echoinput --help          Show this help");
    println!();
    println!("OPTIONS:");
    println!("  --debug     Enable debug logging");
    println!("  --trace     Enable trace logging (very verbose)");
    println!();
    println!("NOTES:");
    println!("  - Requires read access to /dev/input/event* devices (Linux)");
    println!("  - If overlay doesn't appear, check: ls -la /dev/input/event*");
    println!("  - Fix permissions: sudo usermod -aG input $USER  (then relogin)");
    println!("  - Config saved to: ~/.config/echoinput/config.toml");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(parse_log_level())
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

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
    let shutdown = Arc::new(AtomicBool::new(false));

    rt.block_on(async {
        let mut renderer = WaylandRenderer::with_shutdown(bus.clone(), shutdown.clone());

        if let Err(e) = renderer.start(config.clone()).await {
            error!("Failed to start overlay: {}", e);
            eprintln!("Error: Failed to start overlay: {}", e);
            return;
        }

        let mut capture = EvdevCapture::with_shutdown(shutdown.clone());
        let mut input_rx = capture.subscribe();

        if let Err(e) = capture.start().await {
            error!("Failed to start evdev capture: {}", e);
            eprintln!("Error: Failed to start keyboard capture: {}", e);
            eprintln!("Hint: No keyboard devices found. Check /dev/input/event* permissions.");
            eprintln!("      Try: sudo usermod -aG input $USER  (then relogin)");
            return;
        }

        eprintln!("EchoInput overlay running. Press keys to see visualization.");
        eprintln!("Press Ctrl+C to quit.");

        let mut processor = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            history_length: config.history_length,
            dedup_window: Duration::from_millis(50),
        });

        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        loop {
            tokio::select! {
                result = input_rx.recv() => {
                    match result {
                        Ok(event) => {
                            let processed = processor.process(event);
                            for pe in processed {
                                match pe {
                                    ProcessedEvent::Shortcut(combo) => {
                                        if let Err(e) = renderer.update(DisplayEvent::Shortcut(combo)) {
                                            warn!("Failed to send shortcut to renderer: {}", e);
                                        }
                                    }
                                    ProcessedEvent::RawKey(kbd) => {
                                        let combo = ShortcutCombo::new(
                                            ModifierState::default(),
                                            Some(kbd.key),
                                        );
                                        if let Err(e) = renderer.update(DisplayEvent::Shortcut(combo)) {
                                            warn!("Failed to send key to renderer: {}", e);
                                        }
                                    }
                                    ProcessedEvent::ModifierChange(_) => {}
                                }
                            }
                        }
                        Err(RecvError::Lagged(n)) => {
                            warn!("Input channel lagged, dropped {} events", n);
                        }
                        Err(RecvError::Closed) => {
                            error!("Input channel closed — capture thread may have exited");
                            eprintln!("Error: Input capture channel closed.");
                            break;
                        }
                    }
                }
                _ = &mut ctrl_c => {
                    eprintln!("\nShutting down...");
                    shutdown.store(true, Ordering::Relaxed);
                    break;
                }
            }

            if shutdown.load(Ordering::Relaxed) {
                eprintln!("\nShutting down...");
                break;
            }
        }

        let _ = capture.stop().await;
        let _ = renderer.stop().await;
    });
}

// ── Settings GUI mode ──────────────────────────────────────────

fn run_settings_gui(initial_config: FileConfig) {
    info!("Starting EchoInput settings GUI");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 480.0])
            .with_min_inner_size([420.0, 380.0])
            .with_title("EchoInput Settings"),
        ..Default::default()
    };

    eframe::run_native(
        "EchoInput Settings",
        options,
        Box::new(move |cc| {
            let theme = Theme::dark();
            apply_theme(&cc.egui_ctx, &theme);
            Ok(Box::new(SettingsApp::new(initial_config, theme)))
        }),
    )
    .unwrap();
}

#[derive(PartialEq, Clone, Copy)]
enum SettingsTab {
    General,
    Position,
    Keycap,
    Display,
    About,
}

impl SettingsTab {
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Position => "Position",
            Self::Keycap => "Keycap",
            Self::Display => "Display",
            Self::About => "About",
        }
    }

    fn all() -> &'static [SettingsTab] {
        &[Self::General, Self::Position, Self::Keycap, Self::Display, Self::About]
    }
}

struct SettingsApp {
    config: FileConfig,
    theme: Theme,
    active_tab: SettingsTab,
    position_index: usize,
    scale_index: usize,
    theme_index: usize,
    keycap_style_index: usize,
    animation_type_index: usize,
    text_caps_index: usize,
    text_variant_index: usize,
    preset_index: usize,
    save_status: String,
    save_status_time: Option<std::time::Instant>,
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
const KEYCAP_STYLES: &[&str] = &["Minimal", "Laptop", "LowProfile", "PBT"];
const ANIMATION_TYPES: &[&str] = &["None", "Fade", "Zoom", "Float", "Slide"];
const TEXT_CAPS: &[&str] = &["Uppercase", "Capitalize", "Lowercase"];
const TEXT_VARIANTS: &[&str] = &["Full", "Short", "Icon"];

impl SettingsApp {
    fn new(config: FileConfig, theme: Theme) -> Self {
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
        let keycap_style_index = config
            .keycap_style
            .as_deref()
            .and_then(|s| KEYCAP_STYLES.iter().position(|&x| x == s))
            .unwrap_or(1);
        let animation_type_index = config
            .animation_type
            .as_deref()
            .and_then(|a| ANIMATION_TYPES.iter().position(|&x| x == a))
            .unwrap_or(4);
        let text_caps_index = config
            .text_caps
            .as_deref()
            .and_then(|c| TEXT_CAPS.iter().position(|&x| x == c))
            .unwrap_or(0);
        let text_variant_index = config
            .text_variant
            .as_deref()
            .and_then(|v| TEXT_VARIANTS.iter().position(|&x| x == v))
            .unwrap_or(0);

        Self {
            config,
            theme,
            active_tab: SettingsTab::General,
            position_index,
            scale_index,
            theme_index,
            keycap_style_index,
            animation_type_index,
            text_caps_index,
            text_variant_index,
            preset_index: 0,
            save_status: String::new(),
            save_status_time: None,
        }
    }

    fn sync_to_config(&mut self) {
        self.config.position = Some(POSITIONS[self.position_index].into());
        self.config.scale = Some(SCALES[self.scale_index].into());
        self.config.theme = Some(THEMES[self.theme_index].into());
        self.config.keycap_style = Some(KEYCAP_STYLES[self.keycap_style_index].into());
        self.config.animation_type = Some(ANIMATION_TYPES[self.animation_type_index].into());
        self.config.text_caps = Some(TEXT_CAPS[self.text_caps_index].into());
        self.config.text_variant = Some(TEXT_VARIANTS[self.text_variant_index].into());
    }

    fn apply_preset(&mut self, preset: &ThemePreset) {
        self.config.keycap_primary = Some(preset.colors.keycap_primary.clone());
        self.config.keycap_secondary = Some(preset.colors.keycap_secondary.clone());
        self.config.use_gradient = Some(preset.colors.use_gradient);
        self.config.highlight_modifiers = Some(preset.colors.highlight_modifiers);
        self.config.modifier_primary = Some(preset.colors.modifier_primary.clone());
        self.config.modifier_secondary = Some(preset.colors.modifier_secondary.clone());
        self.config.text_size = preset.text.size;
        self.config.text_color = Some(preset.text.color.clone());
        self.config.text_modifier_color = Some(preset.text.modifier_color.clone());
        self.config.text_caps = Some(format!("{:?}", preset.text.caps));
        self.config.text_variant = Some(format!("{:?}", preset.text.variant));
        self.config.border_enabled = Some(preset.border.enabled);
        self.config.border_color = Some(preset.border.color.clone());
        self.config.border_width = Some(preset.border.width);
        self.config.border_radius = Some(preset.border.radius);
        self.config.border_modifier_color = Some(preset.border.modifier_color.clone());

        self.keycap_style_index = KEYCAP_STYLES
            .iter()
            .position(|&s| s == format!("{:?}", preset.keycap_style))
            .unwrap_or(1);
        self.text_caps_index = TEXT_CAPS
            .iter()
            .position(|&s| s == format!("{:?}", preset.text.caps))
            .unwrap_or(0);
        self.text_variant_index = TEXT_VARIANTS
            .iter()
            .position(|&s| s == format!("{:?}", preset.text.variant))
            .unwrap_or(0);
    }

    fn save(&mut self) {
        self.sync_to_config();
        match self.config.save() {
            Ok(()) => {
                self.save_status = "Saved".into();
                self.save_status_time = Some(std::time::Instant::now());
            }
            Err(e) => {
                self.save_status = format!("Error: {}", e);
                self.save_status_time = Some(std::time::Instant::now());
            }
        }
    }

    // ── UI Helpers ─────────────────────────────────────────────

    fn section_header(ui: &mut egui::Ui, theme: &Theme, label: &str) {
        ui.add_space(4.0);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 0.0),
            egui::Sense::hover(),
        );
        ui.painter().text(
            rect.min,
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(14.0),
            theme.text_dim,
        );
        ui.add_space(18.0);
    }

    fn card<F: FnOnce(&mut egui::Ui)>(
        ui: &mut egui::Ui,
        theme: &Theme,
        content: F,
    ) -> egui::Response {
        let margin = egui::Margin::same(12);
        let frame = egui::Frame::NONE
            .fill(theme.bg_card)
            .corner_radius(egui::CornerRadius::same(8))
            .stroke(egui::Stroke::new(0.5, theme.border))
            .inner_margin(margin);
        frame.show(ui, |ui| {
            content(ui);
        })
        .response
    }

    fn labeled_slider(
        ui: &mut egui::Ui,
        theme: &Theme,
        label: &str,
        value: &mut f32,
        range: std::ops::RangeInclusive<f32>,
        suffix: &str,
    ) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(theme.text_dim).size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.0}{}", value, suffix))
                        .color(theme.accent)
                        .size(13.0)
                        .strong(),
                );
            });
        });
        ui.spacing_mut().slider_width = ui.available_width();
        ui.add(
            egui::Slider::new(value, range)
                .suffix(suffix)
                .show_value(false),
        );
    }

    fn labeled_slider_f64(
        ui: &mut egui::Ui,
        theme: &Theme,
        label: &str,
        value: &mut f64,
        range: std::ops::RangeInclusive<f64>,
        suffix: &str,
    ) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(theme.text_dim).size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.0}{}", value, suffix))
                        .color(theme.accent)
                        .size(13.0)
                        .strong(),
                );
            });
        });
        ui.spacing_mut().slider_width = ui.available_width();
        ui.add(
            egui::Slider::new(value, range)
                .suffix(suffix)
                .show_value(false),
        );
    }

    fn dropdown(
        ui: &mut egui::Ui,
        theme: &Theme,
        id: &str,
        label: &str,
        options: &[&str],
        selected: &mut usize,
    ) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(theme.text_dim).size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt(id)
                    .selected_text(egui::RichText::new(options[*selected]).color(theme.text).size(13.0))
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for (i, &opt) in options.iter().enumerate() {
                            ui.selectable_value(selected, i, egui::RichText::new(opt).size(13.0));
                        }
                    });
            });
        });
    }

    fn color_row(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &mut String) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(theme.text_dim).size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Color preview swatch
                if let Some(hex) = value.strip_prefix('#') {
                    if hex.len() >= 6 {
                        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(3),
                            egui::Color32::from_rgb(r, g, b),
                        );
                        ui.add_space(4.0);
                    }
                }
                let mut color = value.clone();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut color)
                        .desired_width(80.0)
                        .font(egui::FontId::monospace(12.0)),
                );
                if response.changed() {
                    *value = color;
                }
            });
        });
    }

    fn toggle_row(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &mut bool) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(theme.text_dim).size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = ui.toggle_value(value, "");
                if response.changed() {
                    // value is already updated by toggle
                }
            });
        });
    }

    fn save_bar(ui: &mut egui::Ui, theme: &Theme, ctx: &egui::Context, app: &mut SettingsApp) {
        ui.add_space(8.0);
        let frame = egui::Frame::NONE
            .fill(theme.bg_card)
            .corner_radius(egui::CornerRadius::same(8))
            .stroke(egui::Stroke::new(0.5, theme.border))
            .inner_margin(egui::Margin::same(12));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                let save_btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("Save").size(13.0).strong(),
                    )
                    .fill(theme.accent)
                    .corner_radius(egui::CornerRadius::same(6))
                    .min_size(egui::vec2(80.0, 30.0)),
                );
                if save_btn.clicked() {
                    app.save();
                }

                let close_btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("Save & Close").size(13.0),
                    )
                    .fill(theme.bg_hover)
                    .corner_radius(egui::CornerRadius::same(6))
                    .min_size(egui::vec2(100.0, 30.0)),
                );
                if close_btn.clicked() {
                    app.sync_to_config();
                    if app.config.save().is_ok() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }

                // Show save status with auto-fade
                if !app.save_status.is_empty() {
                    let show = match app.save_status_time {
                        Some(t) => t.elapsed() < std::time::Duration::from_secs(3),
                        None => false,
                    };
                    if show {
                        let color = if app.save_status == "Saved" {
                            theme.success
                        } else {
                            egui::Color32::from_rgb(255, 100, 100)
                        };
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(&app.save_status)
                                    .color(color)
                                    .size(12.0),
                            );
                        });
                    } else {
                        app.save_status.clear();
                    }
                }
            });
        });
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = self.theme.clone();

        // ── Top Tab Bar ──
        egui::TopBottomPanel::top("tab_bar")
            .frame(
                egui::Frame::NONE
                    .fill(theme.bg)
                    .stroke(egui::Stroke::new(0.5, theme.separator))
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for &tab in SettingsTab::all() {
                        let is_active = self.active_tab == tab;
                        let text_color = if is_active {
                            theme.tab_active
                        } else {
                            theme.tab_inactive
                        };
                        let btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new(tab.label())
                                    .color(text_color)
                                    .size(13.0)
                                    .strong(),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE),
                        );

                        // Underline for active tab
                        if is_active {
                            let rect = btn.rect;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(rect.left(), rect.bottom()),
                                    egui::pos2(rect.right(), rect.bottom()),
                                ],
                                egui::Stroke::new(2.0, theme.tab_active),
                            );
                        }

                        if btn.clicked() {
                            self.active_tab = tab;
                        }
                    }
                });
                ui.add_space(4.0);
            });

        // ── Content Panel ──
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme.bg)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                match self.active_tab {
                    SettingsTab::General => self.render_general_tab(ui, ctx),
                    SettingsTab::Position => self.render_position_tab(ui, ctx),
                    SettingsTab::Keycap => self.render_keycap_tab(ui, ctx),
                    SettingsTab::Display => self.render_display_tab(ui, ctx),
                    SettingsTab::About => self.render_about_tab(ui),
                }
            });
    }
}

impl SettingsApp {
    fn render_general_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let theme = self.theme.clone();

        ui.add_space(4.0);
        Self::card(ui, &theme, |ui| {
            Self::dropdown(ui, &theme, "theme", "Theme", THEMES, &mut self.theme_index);
            ui.add_space(4.0);

            ui.label(egui::RichText::new("Monitor").color(theme.text_dim).size(13.0));
            ui.add_space(2.0);
            let mut monitor = self.config.monitor.clone().unwrap_or_default();
            let response = ui.add(
                egui::TextEdit::singleline(&mut monitor)
                    .hint_text("Default")
                    .desired_width(ui.available_width())
                    .font(egui::FontId::proportional(13.0)),
            );
            if response.changed() {
                self.config.monitor = if monitor.is_empty() {
                    None
                } else {
                    Some(monitor)
                };
            }
        });

        Self::save_bar(ui, &theme, ctx, self);
    }

    fn render_position_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let theme = self.theme.clone();

        ui.add_space(4.0);
        Self::card(ui, &theme, |ui| {
            Self::dropdown(ui, &theme, "position", "Position", POSITIONS, &mut self.position_index);
            ui.add_space(4.0);
            Self::dropdown(ui, &theme, "scale", "Scale", SCALES, &mut self.scale_index);

            ui.add_space(4.0);
            let mut margin_x = self.config.margin_x.unwrap_or(16.0);
            Self::labeled_slider(ui, &theme, "Margin X", &mut margin_x, 0.0..=100.0, "px");
            self.config.margin_x = Some(margin_x);

            let mut margin_y = self.config.margin_y.unwrap_or(16.0);
            Self::labeled_slider(ui, &theme, "Margin Y", &mut margin_y, 0.0..=100.0, "px");
            self.config.margin_y = Some(margin_y);
        });

        ui.add_space(8.0);
        Self::card(ui, &theme, |ui| {
            Self::dropdown(
                ui,
                &theme,
                "animation_type",
                "Animation",
                ANIMATION_TYPES,
                &mut self.animation_type_index,
            );

            ui.add_space(4.0);
            let mut anim_speed = self.config.animation_speed.unwrap_or(0.5);
            Self::labeled_slider(ui, &theme, "Speed", &mut anim_speed, 0.05..=1.0, "");
            self.config.animation_speed = Some(anim_speed);

            let mut duration_ms = self.config.display_duration_ms.unwrap_or(1500) as f32;
            Self::labeled_slider(ui, &theme, "Duration", &mut duration_ms, 500.0..=5000.0, "ms");
            self.config.display_duration_ms = Some(duration_ms as u64);
        });

        Self::save_bar(ui, &theme, ctx, self);
    }

    fn render_keycap_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let theme = self.theme.clone();

        // ── Preset ──
        Self::card(ui, &theme, |ui| {
            let presets = ThemePreset::all();
            let preset_names: Vec<String> = presets.iter().map(|p| p.name.clone()).collect();

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Preset")
                        .color(theme.text_dim)
                        .size(13.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let apply_btn = ui.add(
                        egui::Button::new(
                            egui::RichText::new("Apply").size(12.0),
                        )
                        .fill(theme.accent)
                        .corner_radius(egui::CornerRadius::same(4)),
                    );
                    if apply_btn.clicked() {
                        if let Some(preset) = presets.get(self.preset_index) {
                            self.apply_preset(preset);
                        }
                    }

                    egui::ComboBox::from_id_salt("preset")
                        .selected_text(
                            egui::RichText::new(
                                preset_names.get(self.preset_index).cloned().unwrap_or_default(),
                            )
                            .size(13.0),
                        )
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for (i, name) in preset_names.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.preset_index,
                                    i,
                                    egui::RichText::new(name.as_str()).size(13.0),
                                );
                            }
                        });
                });
            });
        });

        ui.add_space(4.0);

        // ── Style ──
        Self::card(ui, &theme, |ui| {
            Self::dropdown(
                ui,
                &theme,
                "keycap_style",
                "Style",
                KEYCAP_STYLES,
                &mut self.keycap_style_index,
            );
        });

        ui.add_space(4.0);

        // ── Colors ──
        Self::card(ui, &theme, |ui| {
            Self::section_header(ui, &theme, "Colors");

            Self::color_row(ui, &theme, "Primary", &mut self.config.keycap_primary.clone().unwrap_or_default());
            Self::color_row(ui, &theme, "Secondary", &mut self.config.keycap_secondary.clone().unwrap_or_default());

            let mut use_gradient = self.config.use_gradient.unwrap_or(true);
            Self::toggle_row(ui, &theme, "Gradient", &mut use_gradient);
            self.config.use_gradient = Some(use_gradient);

            ui.add_space(4.0);

            let mut highlight_mods = self.config.highlight_modifiers.unwrap_or(true);
            Self::toggle_row(ui, &theme, "Highlight Modifiers", &mut highlight_mods);
            self.config.highlight_modifiers = Some(highlight_mods);

            if highlight_mods {
                Self::color_row(ui, &theme, "Modifier Primary", &mut self.config.modifier_primary.clone().unwrap_or_default());
                Self::color_row(ui, &theme, "Modifier Secondary", &mut self.config.modifier_secondary.clone().unwrap_or_default());
            }
        });

        ui.add_space(4.0);

        // ── Text ──
        Self::card(ui, &theme, |ui| {
            Self::section_header(ui, &theme, "Text");

            let mut text_size = self.config.text_size.unwrap_or(0.0);
            Self::labeled_slider(ui, &theme, "Font Size", &mut text_size, 0.0..=64.0, "px");
            self.config.text_size = if text_size <= 0.0 { None } else { Some(text_size) };

            Self::color_row(ui, &theme, "Color", &mut self.config.text_color.clone().unwrap_or_default());

            Self::dropdown(
                ui,
                &theme,
                "text_caps",
                "Capitalization",
                TEXT_CAPS,
                &mut self.text_caps_index,
            );

            Self::dropdown(
                ui,
                &theme,
                "text_variant",
                "Variant",
                TEXT_VARIANTS,
                &mut self.text_variant_index,
            );

            let highlight_mods = self.config.highlight_modifiers.unwrap_or(true);
            if highlight_mods {
                Self::color_row(ui, &theme, "Modifier Color", &mut self.config.text_modifier_color.clone().unwrap_or_default());
            }
        });

        ui.add_space(4.0);

        // ── Border ──
        Self::card(ui, &theme, |ui| {
            Self::section_header(ui, &theme, "Border");

            let mut border_enabled = self.config.border_enabled.unwrap_or(true);
            Self::toggle_row(ui, &theme, "Enabled", &mut border_enabled);
            self.config.border_enabled = Some(border_enabled);

            if border_enabled {
                Self::color_row(ui, &theme, "Color", &mut self.config.border_color.clone().unwrap_or_default());

                let mut border_width = self.config.border_width.unwrap_or(1.0);
                Self::labeled_slider(ui, &theme, "Width", &mut border_width, 0.5..=4.0, "px");
                self.config.border_width = Some(border_width);

                let mut border_radius = self.config.border_radius.unwrap_or(0.25);
                Self::labeled_slider(ui, &theme, "Radius", &mut border_radius, 0.0..=1.0, "%");
                self.config.border_radius = Some(border_radius);

                let highlight_mods = self.config.highlight_modifiers.unwrap_or(true);
                if highlight_mods {
                    Self::color_row(ui, &theme, "Modifier Color", &mut self.config.border_modifier_color.clone().unwrap_or_default());
                }
            }
        });

        ui.add_space(4.0);

        // ── Background ──
        Self::card(ui, &theme, |ui| {
            Self::section_header(ui, &theme, "Background");

            let mut bg_enabled = self.config.background_enabled.unwrap_or(false);
            Self::toggle_row(ui, &theme, "Fill", &mut bg_enabled);
            self.config.background_enabled = Some(bg_enabled);

            if bg_enabled {
                Self::color_row(ui, &theme, "Color", &mut self.config.background_color.clone().unwrap_or_default());
            }
        });

        Self::save_bar(ui, &theme, ctx, self);
    }

    fn render_display_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let theme = self.theme.clone();

        ui.add_space(4.0);
        Self::card(ui, &theme, |ui| {
            let mut opacity = self.config.opacity.unwrap_or(0.9) as f64;
            Self::labeled_slider_f64(ui, &theme, "Opacity", &mut opacity, 0.1..=1.0, "%");
            self.config.opacity = Some(opacity as f32);

            ui.add_space(4.0);

            let mut hist = self.config.history_length.unwrap_or(3) as f32;
            Self::labeled_slider(ui, &theme, "History Length", &mut hist, 1.0..=10.0, "");
            self.config.history_length = Some(hist as usize);
        });

        Self::save_bar(ui, &theme, ctx, self);
    }

    fn render_about_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();

        ui.add_space(12.0);

        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("EchoInput")
                        .size(24.0)
                        .color(theme.accent)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("v0.1.0")
                        .size(13.0)
                        .color(theme.text_muted),
                );
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("A privacy-first keyboard visualization overlay")
                        .size(13.0)
                        .color(theme.text_dim),
                );
                ui.add_space(24.0);

                Self::card(ui, &theme, |ui| {
                    ui.label(
                        egui::RichText::new("Config file")
                            .size(12.0)
                            .color(theme.text_muted),
                    );
                    ui.add_space(2.0);
                    if let Some(path) = FileConfig::config_path() {
                        ui.label(
                            egui::RichText::new(path.display().to_string())
                                .size(12.0)
                                .color(theme.text)
                                .monospace(),
                        );
                    }
                });

                ui.add_space(12.0);

                if ui.add(
                    egui::Button::new(
                        egui::RichText::new("Open Config Directory").size(13.0),
                    )
                    .fill(theme.bg_hover)
                    .corner_radius(egui::CornerRadius::same(6)),
                ).clicked() {
                    if let Some(path) = FileConfig::config_path() {
                        if let Some(parent) = path.parent() {
                            // Cross-platform directory open
                            #[cfg(target_os = "linux")]
                            { let _ = std::process::Command::new("xdg-open").arg(parent).spawn(); }
                            #[cfg(target_os = "macos")]
                            { let _ = std::process::Command::new("open").arg(parent).spawn(); }
                            #[cfg(target_os = "windows")]
                            { let _ = std::process::Command::new("explorer").arg(parent).spawn(); }
                        }
                    }
                }
            });
        });
    }
}
