use input_core::config::FileConfig;
use input_core::events::{ModifierState, ProcessedEvent, ShortcutCombo};
use input_core::ipc::MessageBus;
use input_core::overlay::{DisplayEvent, OverlayConfig};
use input_core::presets::ThemePreset;
use input_core::processor::DefaultEventProcessor;
use input_core::traits::{EventProcessor, ProcessorConfig};
use platform::{KeyboardCaptureProvider, OverlayRenderer};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info, warn};

// ── Theme ──────────────────────────────────────────────────────

#[derive(Clone)]
struct Theme {
    bg: eframe::egui::Color32,
    bg_card: eframe::egui::Color32,
    bg_hover: eframe::egui::Color32,
    bg_input: eframe::egui::Color32,
    accent: eframe::egui::Color32,
    text: eframe::egui::Color32,
    text_dim: eframe::egui::Color32,
    text_muted: eframe::egui::Color32,
    border: eframe::egui::Color32,
    separator: eframe::egui::Color32,
    tab_active: eframe::egui::Color32,
    tab_inactive: eframe::egui::Color32,
    success: eframe::egui::Color32,
}

impl Theme {
    fn dark() -> Self {
        Self {
            bg: eframe::egui::Color32::from_rgb(32, 33, 36),
            bg_card: eframe::egui::Color32::from_rgb(45, 46, 50),
            bg_hover: eframe::egui::Color32::from_rgb(55, 56, 62),
            bg_input: eframe::egui::Color32::from_rgb(38, 39, 43),
            accent: eframe::egui::Color32::from_rgb(88, 166, 255),
            text: eframe::egui::Color32::from_rgb(232, 232, 232),
            text_dim: eframe::egui::Color32::from_rgb(180, 180, 185),
            text_muted: eframe::egui::Color32::from_rgb(120, 120, 128),
            border: eframe::egui::Color32::from_rgb(55, 56, 62),
            separator: eframe::egui::Color32::from_rgb(50, 51, 56),
            tab_active: eframe::egui::Color32::from_rgb(88, 166, 255),
            tab_inactive: eframe::egui::Color32::from_rgb(140, 140, 148),
            success: eframe::egui::Color32::from_rgb(76, 175, 80),
        }
    }
}

fn apply_theme(ctx: &eframe::egui::Context, theme: &Theme) {
    let mut style = (*ctx.style()).clone();
    let visuals = &mut style.visuals;
    visuals.dark_mode = true;
    visuals.override_text_color = Some(theme.text);
    visuals.widgets.noninteractive.bg_fill = theme.bg_card;
    visuals.widgets.noninteractive.fg_stroke = eframe::egui::Stroke::new(1.0, theme.text_dim);
    visuals.widgets.noninteractive.bg_stroke = eframe::egui::Stroke::new(0.5, theme.border);
    visuals.widgets.noninteractive.corner_radius = eframe::egui::CornerRadius::same(6);
    visuals.widgets.inactive.bg_fill = theme.bg_input;
    visuals.widgets.inactive.fg_stroke = eframe::egui::Stroke::new(1.0, theme.text);
    visuals.widgets.inactive.bg_stroke = eframe::egui::Stroke::new(0.5, theme.border);
    visuals.widgets.inactive.corner_radius = eframe::egui::CornerRadius::same(6);
    visuals.widgets.hovered.bg_fill = theme.bg_hover;
    visuals.widgets.hovered.fg_stroke = eframe::egui::Stroke::new(1.0, theme.text);
    visuals.widgets.hovered.bg_stroke = eframe::egui::Stroke::new(1.0, theme.accent);
    visuals.widgets.hovered.corner_radius = eframe::egui::CornerRadius::same(6);
    visuals.widgets.active.bg_fill = theme.accent;
    visuals.widgets.active.fg_stroke = eframe::egui::Stroke::new(1.0, eframe::egui::Color32::WHITE);
    visuals.widgets.active.corner_radius = eframe::egui::CornerRadius::same(6);
    visuals.selection.bg_fill = theme.accent.linear_multiply(0.3);
    visuals.selection.stroke = eframe::egui::Stroke::new(1.0, theme.accent);
    visuals.extreme_bg_color = theme.bg;
    visuals.faint_bg_color = theme.bg_card;
    visuals.striped = false;
    visuals.slider_trailing_fill = true;
    style.spacing.item_spacing = eframe::egui::vec2(8.0, 6.0);
    style.spacing.indent = 18.0;
    style.spacing.button_padding = eframe::egui::vec2(12.0, 6.0);
    style.spacing.window_margin = eframe::egui::Margin::same(16);
    ctx.set_style(style);
}

// ── CLI & Main ─────────────────────────────────────────────────

fn parse_log_level() -> String {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--trace") { return "trace".into(); }
    if args.iter().any(|a| a == "--debug") { return "debug".into(); }
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
    println!("PLATFORM-SPECIFIC NOTES:");
    #[cfg(target_os = "linux")]
    {
        println!("  Linux: Requires read access to /dev/input/event* devices");
        println!("  Fix permissions: sudo usermod -aG input $USER  (then relogin)");
    }
    #[cfg(target_os = "windows")]
    {
        println!("  Windows: Global keyboard hook requires the app to be running");
        println!("  The overlay will appear on top of all windows");
    }
    #[cfg(target_os = "macos")]
    {
        println!("  macOS: Requires Accessibility permissions");
        println!("  System Preferences > Privacy & Security > Accessibility > EchoInput");
    }
    println!();
    println!("Config saved to: ~/.config/echoinput/config.toml");
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

// ── Overlay mode ───────────────────────────────────────────────

fn run_overlay(config: OverlayConfig) {
    info!("Starting EchoInput overlay");
    eprintln!("EchoInput overlay running. Press keys to see visualization.");
    eprintln!("Press Ctrl+C to quit.");

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let bus = MessageBus::new(4096);
    let shutdown = Arc::new(AtomicBool::new(false));

    rt.block_on(async {
        // Create platform-specific renderer and capture
        let (mut renderer, mut capture) = create_platform_components(bus.clone(), shutdown.clone());

        if let Err(e) = renderer.start(config.clone()).await {
            error!("Failed to start overlay: {}", e);
            eprintln!("Error: Failed to start overlay: {}", e);
            return;
        }

        if let Err(e) = capture.start().await {
            error!("Failed to start keyboard capture: {}", e);
            eprintln!("Error: Failed to start keyboard capture: {}", e);
            #[cfg(target_os = "linux")]
            {
                eprintln!("Hint: No keyboard devices found. Check /dev/input/event* permissions.");
                eprintln!("      Try: sudo usermod -aG input $USER  (then relogin)");
            }
            #[cfg(target_os = "macos")]
            {
                eprintln!("Hint: Check System Preferences > Privacy & Security > Accessibility");
            }
            return;
        }

        let mut input_rx = capture.subscribe();
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

// ── Platform-specific component creation ────────────────────────

#[cfg(target_os = "linux")]
fn create_platform_components(
    bus: MessageBus,
    shutdown: Arc<AtomicBool>,
) -> (
    Box<dyn OverlayRenderer>,
    Box<dyn KeyboardCaptureProvider>,
) {
    use platform_linux::{overlay_wayland::WaylandRenderer, overlay_x11::X11Renderer, evdev_capture::EvdevCapture};

    let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();

    let renderer: Box<dyn OverlayRenderer> = if is_wayland {
        eprintln!("Detected Wayland display server");
        Box::new(WaylandRenderer::with_shutdown(bus, shutdown.clone()))
    } else {
        eprintln!("Detected X11 display server");
        Box::new(X11Renderer::with_shutdown(bus, shutdown.clone()))
    };

    let capture: Box<dyn KeyboardCaptureProvider> =
        Box::new(EvdevCapture::with_shutdown(shutdown));

    (renderer, capture)
}

#[cfg(target_os = "windows")]
fn create_platform_components(
    bus: MessageBus,
    shutdown: Arc<AtomicBool>,
) -> (
    Box<dyn OverlayRenderer>,
    Box<dyn KeyboardCaptureProvider>,
) {
    use platform_windows::{overlay::WindowsRenderer, WindowsCapture};

    let renderer: Box<dyn OverlayRenderer> =
        Box::new(WindowsRenderer::with_shutdown(bus, shutdown.clone()));
    let capture: Box<dyn KeyboardCaptureProvider> =
        Box::new(WindowsCapture::with_shutdown(shutdown).expect("Failed to create Windows capture"));

    (renderer, capture)
}

#[cfg(target_os = "macos")]
fn create_platform_components(
    bus: MessageBus,
    shutdown: Arc<AtomicBool>,
) -> (
    Box<dyn OverlayRenderer>,
    Box<dyn KeyboardCaptureProvider>,
) {
    use platform_macos::{overlay::MacRenderer, MacosCapture};

    let renderer: Box<dyn OverlayRenderer> =
        Box::new(MacRenderer::with_shutdown(bus, shutdown.clone()));
    let capture: Box<dyn KeyboardCaptureProvider> =
        Box::new(MacosCapture::with_shutdown(shutdown).expect("Failed to create macOS capture"));

    (renderer, capture)
}

// ── Settings GUI ───────────────────────────────────────────────

fn run_settings_gui(initial_config: FileConfig) {
    info!("Starting EchoInput settings GUI");

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
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

const POSITIONS: &[&str] = &["BottomCenter", "TopLeft", "TopRight", "TopCenter", "BottomLeft", "BottomRight", "Center"];
const SCALES: &[&str] = &["Small", "Medium", "Large", "ExtraLarge"];
const THEMES: &[&str] = &["Dark", "Light", "System"];
const KEYCAP_STYLES: &[&str] = &["Minimal", "Laptop", "LowProfile", "PBT"];
const ANIMATION_TYPES: &[&str] = &["None", "Fade", "Zoom", "Float", "Slide"];
const TEXT_CAPS: &[&str] = &["Uppercase", "Capitalize", "Lowercase"];
const TEXT_VARIANTS: &[&str] = &["Full", "Short", "Icon"];

impl SettingsApp {
    fn new(config: FileConfig, theme: Theme) -> Self {
        let position_index = config.position.as_deref().and_then(|p| POSITIONS.iter().position(|&s| s == p)).unwrap_or(0);
        let scale_index = config.scale.as_deref().and_then(|s| SCALES.iter().position(|&x| x == s)).unwrap_or(1);
        let theme_index = config.theme.as_deref().and_then(|t| THEMES.iter().position(|&x| x == t)).unwrap_or(0);
        let keycap_style_index = config.keycap_style.as_deref().and_then(|s| KEYCAP_STYLES.iter().position(|&x| x == s)).unwrap_or(1);
        let animation_type_index = config.animation_type.as_deref().and_then(|a| ANIMATION_TYPES.iter().position(|&x| x == a)).unwrap_or(4);
        let text_caps_index = config.text_caps.as_deref().and_then(|c| TEXT_CAPS.iter().position(|&x| x == c)).unwrap_or(0);
        let text_variant_index = config.text_variant.as_deref().and_then(|v| TEXT_VARIANTS.iter().position(|&x| x == v)).unwrap_or(0);

        Self {
            config, theme, active_tab: SettingsTab::General,
            position_index, scale_index, theme_index, keycap_style_index,
            animation_type_index, text_caps_index, text_variant_index,
            preset_index: 0, save_status: String::new(), save_status_time: None,
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
        self.keycap_style_index = KEYCAP_STYLES.iter().position(|&s| s == format!("{:?}", preset.keycap_style)).unwrap_or(1);
        self.text_caps_index = TEXT_CAPS.iter().position(|&s| s == format!("{:?}", preset.text.caps)).unwrap_or(0);
        self.text_variant_index = TEXT_VARIANTS.iter().position(|&s| s == format!("{:?}", preset.text.variant)).unwrap_or(0);
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

    // ── UI Helpers ──

    fn section_header(ui: &mut eframe::egui::Ui, theme: &Theme, label: &str) {
        ui.add_space(4.0);
        let (rect, _) = ui.allocate_exact_size(eframe::egui::vec2(ui.available_width(), 0.0), eframe::egui::Sense::hover());
        ui.painter().text(rect.min, eframe::egui::Align2::LEFT_CENTER, label, eframe::egui::FontId::proportional(14.0), theme.text_dim);
        ui.add_space(18.0);
    }

    fn card<F: FnOnce(&mut eframe::egui::Ui)>(ui: &mut eframe::egui::Ui, theme: &Theme, content: F) -> eframe::egui::Response {
        let frame = eframe::egui::Frame::NONE
            .fill(theme.bg_card)
            .corner_radius(eframe::egui::CornerRadius::same(8))
            .stroke(eframe::egui::Stroke::new(0.5, theme.border))
            .inner_margin(eframe::egui::Margin::same(12));
        frame.show(ui, |ui| { content(ui); }).response
    }

    fn labeled_slider(ui: &mut eframe::egui::Ui, theme: &Theme, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) {
        ui.horizontal(|ui| {
            ui.label(eframe::egui::RichText::new(label).color(theme.text_dim).size(13.0));
            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                ui.label(eframe::egui::RichText::new(format!("{:.0}{}", value, suffix)).color(theme.accent).size(13.0).strong());
            });
        });
        ui.spacing_mut().slider_width = ui.available_width();
        ui.add(eframe::egui::Slider::new(value, range).suffix(suffix).show_value(false));
    }

    fn dropdown(ui: &mut eframe::egui::Ui, _theme: &Theme, id: &str, label: &str, options: &[&str], selected: &mut usize) {
        ui.horizontal(|ui| {
            ui.label(eframe::egui::RichText::new(label).color(_theme.text_dim).size(13.0));
            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                eframe::egui::ComboBox::from_id_salt(id)
                    .selected_text(eframe::egui::RichText::new(options[*selected]).color(_theme.text).size(13.0))
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for (i, &opt) in options.iter().enumerate() {
                            ui.selectable_value(selected, i, eframe::egui::RichText::new(opt).size(13.0));
                        }
                    });
            });
        });
    }

    fn toggle_row(ui: &mut eframe::egui::Ui, _theme: &Theme, label: &str, value: &mut bool) {
        ui.horizontal(|ui| {
            ui.label(eframe::egui::RichText::new(label).color(_theme.text_dim).size(13.0));
            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                ui.toggle_value(value, "");
            });
        });
    }

    fn color_row(ui: &mut eframe::egui::Ui, _theme: &Theme, label: &str, value: &mut String) {
        ui.horizontal(|ui| {
            ui.label(eframe::egui::RichText::new(label).color(_theme.text_dim).size(13.0));
            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                if let Some(hex) = value.strip_prefix('#') {
                    if hex.len() >= 6 {
                        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                        let (rect, _) = ui.allocate_exact_size(eframe::egui::vec2(14.0, 14.0), eframe::egui::Sense::hover());
                        ui.painter().rect_filled(rect, eframe::egui::CornerRadius::same(3), eframe::egui::Color32::from_rgb(r, g, b));
                        ui.add_space(4.0);
                    }
                }
                let mut color = value.clone();
                let response = ui.add(eframe::egui::TextEdit::singleline(&mut color).desired_width(80.0).font(eframe::egui::FontId::monospace(12.0)));
                if response.changed() { *value = color; }
            });
        });
    }

    fn save_bar(ui: &mut eframe::egui::Ui, theme: &Theme, ctx: &eframe::egui::Context, app: &mut SettingsApp) {
        ui.add_space(8.0);
        let frame = eframe::egui::Frame::NONE
            .fill(theme.bg_card)
            .corner_radius(eframe::egui::CornerRadius::same(8))
            .stroke(eframe::egui::Stroke::new(0.5, theme.border))
            .inner_margin(eframe::egui::Margin::same(12));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                let save_btn = ui.add(eframe::egui::Button::new(eframe::egui::RichText::new("Save").size(13.0).strong()).fill(theme.accent).corner_radius(eframe::egui::CornerRadius::same(6)).min_size(eframe::egui::vec2(80.0, 30.0)));
                if save_btn.clicked() { app.save(); }
                let close_btn = ui.add(eframe::egui::Button::new(eframe::egui::RichText::new("Save & Close").size(13.0)).fill(theme.bg_hover).corner_radius(eframe::egui::CornerRadius::same(6)).min_size(eframe::egui::vec2(100.0, 30.0)));
                if close_btn.clicked() {
                    app.sync_to_config();
                    if app.config.save().is_ok() { ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close); }
                }
                if !app.save_status.is_empty() {
                    let show = match app.save_status_time { Some(t) => t.elapsed() < std::time::Duration::from_secs(3), None => false };
                    if show {
                        let color = if app.save_status == "Saved" { theme.success } else { eframe::egui::Color32::from_rgb(255, 100, 100) };
                        ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                            ui.label(eframe::egui::RichText::new(&app.save_status).color(color).size(12.0));
                        });
                    } else { app.save_status.clear(); }
                }
            });
        });
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        let theme = self.theme.clone();

        // Tab bar
        eframe::egui::TopBottomPanel::top("tab_bar")
            .frame(eframe::egui::Frame::NONE.fill(theme.bg).stroke(eframe::egui::Stroke::new(0.5, theme.separator)).inner_margin(eframe::egui::Margin::symmetric(16, 0)))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for &tab in SettingsTab::all() {
                        let is_active = self.active_tab == tab;
                        let text_color = if is_active { theme.tab_active } else { theme.tab_inactive };
                        let btn = ui.add(eframe::egui::Button::new(eframe::egui::RichText::new(tab.label()).color(text_color).size(13.0).strong()).fill(eframe::egui::Color32::TRANSPARENT).stroke(eframe::egui::Stroke::NONE));
                        if is_active {
                            let rect = btn.rect;
                            ui.painter().line_segment([rect.min + eframe::egui::vec2(0.0, rect.height()), rect.max + eframe::egui::vec2(0.0, 0.0)], eframe::egui::Stroke::new(2.0, theme.tab_active));
                        }
                        if btn.clicked() { self.active_tab = tab; }
                    }
                });
                ui.add_space(2.0);
            });

        // Content
        eframe::egui::CentralPanel::default().frame(eframe::egui::Frame::NONE.fill(theme.bg).inner_margin(eframe::egui::Margin::same(16))).show(ctx, |ui| {
            match self.active_tab {
                SettingsTab::General => {
                    Self::section_header(ui, &theme, "General");
                    Self::card(ui, &theme, |ui| {
                        Self::dropdown(ui, &theme, "position", "Position", POSITIONS, &mut self.position_index);
                        Self::dropdown(ui, &theme, "scale", "Scale", SCALES, &mut self.scale_index);
                        Self::dropdown(ui, &theme, "theme_color", "Theme", THEMES, &mut self.theme_index);
                        let mut dur_ms = self.config.display_duration_ms.unwrap_or(1500) as f32;
                        Self::labeled_slider(ui, &theme, "Display Duration", &mut dur_ms, 500.0..=5000.0, "ms");
                        self.config.display_duration_ms = Some(dur_ms as u64);
                        let mut hist = self.config.history_length.unwrap_or(3) as f32;
                        Self::labeled_slider(ui, &theme, "History Length", &mut hist, 1.0..=10.0, "");
                        self.config.history_length = Some(hist as usize);
                    });
                }
                SettingsTab::Position => {
                    Self::section_header(ui, &theme, "Positioning");
                    Self::card(ui, &theme, |ui| {
                        Self::dropdown(ui, &theme, "pos2", "Position", POSITIONS, &mut self.position_index);
                        Self::labeled_slider(ui, &theme, "Margin X", &mut self.config.margin_x.as_mut().unwrap(), 0.0..=100.0, "px");
                        Self::labeled_slider(ui, &theme, "Margin Y", &mut self.config.margin_y.as_mut().unwrap(), 0.0..=100.0, "px");
                    });
                }
                SettingsTab::Keycap => {
                    Self::section_header(ui, &theme, "Keycap Style");
                    Self::card(ui, &theme, |ui| {
                        Self::dropdown(ui, &theme, "kcs", "Style", KEYCAP_STYLES, &mut self.keycap_style_index);
                        Self::dropdown(ui, &theme, "tc", "Text Case", TEXT_CAPS, &mut self.text_caps_index);
                        Self::dropdown(ui, &theme, "tv", "Text Variant", TEXT_VARIANTS, &mut self.text_variant_index);
                        Self::color_row(ui, &theme, "Keycap Primary", &mut self.config.keycap_primary.as_mut().unwrap());
                        Self::color_row(ui, &theme, "Keycap Secondary", &mut self.config.keycap_secondary.as_mut().unwrap());
                        Self::color_row(ui, &theme, "Text Color", &mut self.config.text_color.as_mut().unwrap());
                        Self::toggle_row(ui, &theme, "Use Gradient", &mut self.config.use_gradient.as_mut().unwrap());
                        Self::toggle_row(ui, &theme, "Highlight Modifiers", &mut self.config.highlight_modifiers.as_mut().unwrap());
                        Self::toggle_row(ui, &theme, "Show Border", &mut self.config.border_enabled.as_mut().unwrap());
                    });
                }
                SettingsTab::Display => {
                    Self::section_header(ui, &theme, "Animation");
                    Self::card(ui, &theme, |ui| {
                        Self::dropdown(ui, &theme, "anim", "Animation", ANIMATION_TYPES, &mut self.animation_type_index);
                        Self::labeled_slider(ui, &theme, "Animation Speed", &mut self.config.animation_speed.as_mut().unwrap(), 0.05..=1.0, "");
                        Self::labeled_slider(ui, &theme, "Opacity", &mut self.config.opacity.as_mut().unwrap(), 0.1..=1.0, "");
                    });
                }
                SettingsTab::About => {
                    Self::section_header(ui, &theme, "About EchoInput");
                    Self::card(ui, &theme, |ui| {
                        ui.label(eframe::egui::RichText::new("EchoInput").size(18.0).strong().color(theme.accent));
                        ui.add_space(4.0);
                        ui.label(eframe::egui::RichText::new("A keyboard visualization overlay for Wayland, X11, Windows, and macOS").color(theme.text_dim).size(13.0));
                        ui.add_space(8.0);
                        let platform = if cfg!(target_os = "linux") {
                            if std::env::var("WAYLAND_DISPLAY").is_ok() { "Linux (Wayland)" } else { "Linux (X11)" }
                        } else if cfg!(target_os = "windows") { "Windows" }
                        else if cfg!(target_os = "macos") { "macOS" }
                        else { "Unknown" };
                        ui.label(eframe::egui::RichText::new(format!("Platform: {}", platform)).color(theme.text_dim).size(12.0));
                        ui.label(eframe::egui::RichText::new("License: MIT OR Apache-2.0").color(theme.text_muted).size(12.0));
                        ui.label(eframe::egui::RichText::new("Repository: github.com/SuperSection/echoinput").color(theme.text_muted).size(12.0));
                    });
                }
            }
        });

        // Save bar
        eframe::egui::TopBottomPanel::bottom("save_bar").frame(eframe::egui::Frame::NONE.fill(theme.bg).stroke(eframe::egui::Stroke::new(0.5, theme.separator)).inner_margin(eframe::egui::Margin::same(16))).show(ctx, |ui| {
            Self::save_bar(ui, &theme, ctx, self);
        });
    }
}
