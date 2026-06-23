use input_core::events::ShortcutCombo;
use input_core::ipc::MessageBus;
use input_core::overlay::{DisplayEvent, KeycapStyle, OverlayConfig, TextCaps, TextVariant};
use platform::overlay::{OverlayRenderer, OverlayRendererFactory};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

#[allow(dead_code)]
enum RendererCommand {
    Update(DisplayEvent),
    Stop,
}

pub struct MacRenderer {
    bus: Option<MessageBus>,
    cmd_tx: Option<mpsc::UnboundedSender<RendererCommand>>,
    handle: Option<tokio::task::JoinHandle<()>>,
    shutdown: Option<Arc<AtomicBool>>,
}

impl MacRenderer {
    pub fn new(bus: MessageBus) -> Self {
        Self {
            bus: Some(bus),
            cmd_tx: None,
            handle: None,
            shutdown: None,
        }
    }

    pub fn with_shutdown(bus: MessageBus, shutdown: Arc<AtomicBool>) -> Self {
        Self {
            bus: Some(bus),
            cmd_tx: None,
            handle: None,
            shutdown: Some(shutdown),
        }
    }
}

#[async_trait::async_trait]
impl OverlayRenderer for MacRenderer {
    async fn start(&mut self, config: OverlayConfig) -> anyhow::Result<()> {
        let bus = self
            .bus
            .take()
            .ok_or_else(|| anyhow::anyhow!("No MessageBus"))?;
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        self.cmd_tx = Some(cmd_tx);
        let shutdown = self
            .shutdown
            .take()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        let handle = tokio::task::spawn_blocking(move || {
            #[cfg(target_os = "macos")]
            {
                if let Err(e) = run_macos_overlay(bus, config, cmd_rx, shutdown) {
                    error!("macOS overlay error: {}", e);
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                tracing::warn!("macOS overlay not available on this platform");
                let _ = (bus, config, cmd_rx, shutdown);
            }
        });

        self.handle = Some(handle);
        info!("macOS overlay started");
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(RendererCommand::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        info!("macOS overlay stopped");
        Ok(())
    }

    fn update(&self, event: DisplayEvent) -> anyhow::Result<()> {
        if let Some(tx) = &self.cmd_tx {
            tx.send(RendererCommand::Update(event))
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        }
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.handle.is_some()
    }
    fn name(&self) -> &str {
        "MacRenderer"
    }
}

pub struct MacRendererFactory;

impl MacRendererFactory {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacRendererFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayRendererFactory for MacRendererFactory {
    fn create(&self, bus: MessageBus) -> Box<dyn OverlayRenderer> {
        Box::new(MacRenderer::new(bus))
    }
    fn platform_name(&self) -> &str {
        "macos"
    }
}

// ── Hex color parsing ──────────────────────────────────────────

#[allow(dead_code)]
fn parse_hex_color_rgb(hex: &str) -> (f64, f64, f64) {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        6 | 8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f64 / 255.0;
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f64 / 255.0;
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f64 / 255.0;
            (r, g, b)
        }
        _ => (1.0, 1.0, 1.0),
    }
}

// ── macOS overlay implementation ───────────────────────────────

#[cfg(target_os = "macos")]
fn run_macos_overlay(
    _bus: MessageBus,
    initial_config: OverlayConfig,
    mut cmd_rx: mpsc::UnboundedReceiver<RendererCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use objc::sel;
    use objc::sel_impl;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    unsafe {
        // Get NSApplication and screen info
        let ns_app: *mut Object =
            msg_send![Class::get("NSApplication").unwrap(), sharedApplication];
        let screen: *mut Object = msg_send![ns_app, mainWindow];
        let screen_frame: NSRect = msg_send![screen, frame];
        let screen_w = screen_frame.size.width;
        let screen_h = screen_frame.size.height;

        // Create NSPanel (borderless, utility, always-on-top)
        let panel_class = Class::get("NSPanel").unwrap();
        let panel: *mut Object = msg_send![panel_class, alloc];

        let style_mask: NSUInteger = 0; // NSBorderlessWindowMask
        let backing: NSUInteger = 2; // NSBackingStoreBuffered
        let defer: BOOL = 0; // NO

        let content_rect = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: screen_w,
                height: screen_h,
            },
        };

        let panel: *mut Object = msg_send![panel,
            initWithContentRect: content_rect
            styleMask: style_mask
            backing: backing
            defer: defer
        ];

        // Configure panel
        let _: () = msg_send![panel, setLevel: 25i64]; // NSScreenSaverWindowLevel + 1
        let _: () = msg_send![panel, setOpaque: 0u8]; // NO
        let _: () = msg_send![panel, setHasShadow: 0u8]; // NO

        // Set background to clear
        let clear_color: *mut Object = msg_send![Class::get("NSColor").unwrap(), clearColor];
        let _: () = msg_send![panel, setBackgroundColor: clear_color];

        // Make click-through
        let _: () = msg_send![panel, setIgnoresMouseEvents: 1u8]; // YES

        // Collection behavior: join all spaces
        let behavior: NSUInteger = (1 << 0) | (1 << 3); // NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary
        let _: () = msg_send![panel, setCollectionBehavior: behavior];

        // Show panel
        let _: () = msg_send![panel, orderFront: 0usize];

        // Add a content view for rendering
        let view_class = Class::get("NSView").unwrap();
        let view: *mut Object = msg_send![view_class, alloc];
        let view: *mut Object = msg_send![view, initWithFrame: content_rect];
        let _: () = msg_send![panel, setContentView: view];

        let mut config = initial_config;
        let mut current_combos: Vec<ShortcutCombo> = Vec::new();
        let mut running = true;

        info!("macOS overlay running — press keys to see visualization");

        while running && !shutdown.load(Ordering::Relaxed) {
            // Handle commands
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    RendererCommand::Update(display_event) => match display_event {
                        DisplayEvent::Shortcut(combo) => {
                            if combo.modifiers.is_empty() {
                                if let Some(front) = current_combos.first() {
                                    if front.modifiers.is_empty() {
                                        let mut keys = front.key_sequence.clone();
                                        if keys.is_empty() {
                                            if let Some(prev_key) = front.key {
                                                keys.push(prev_key);
                                            }
                                        }
                                        if let Some(new_key) = combo.key {
                                            keys.push(new_key);
                                        }
                                        while keys.len() > 7 {
                                            keys.remove(0);
                                        }
                                        current_combos[0] = ShortcutCombo::sequence(keys);
                                        render_macos_view(
                                            view,
                                            &current_combos,
                                            &config,
                                            screen_w,
                                            screen_h,
                                        );
                                        continue;
                                    }
                                }
                            }
                            current_combos.clear();
                            current_combos.push(combo);
                            render_macos_view(view, &current_combos, &config, screen_w, screen_h);
                        }
                        DisplayEvent::History(combos) => {
                            current_combos = combos;
                            render_macos_view(view, &current_combos, &config, screen_w, screen_h);
                        }
                        DisplayEvent::Clear => {
                            current_combos.clear();
                            let _: () = msg_send![view, setNeedsDisplay: 1u8];
                        }
                        DisplayEvent::UpdateConfig(new_config) => {
                            config = *new_config;
                        }
                    },
                    RendererCommand::Stop => {
                        running = false;
                        break;
                    }
                }
            }

            if current_combos.is_empty() {
                // Nothing to display
            }

            // Process NSEvents (drain the event queue)
            let _: () = msg_send![ns_app, nextEventMatchingMask: NSUInteger::MAX
                untilDate: 0 // nil date = don't block
                inMode: NSString::get("NSEventTrackingRunLoopMode")
                dequeue: 1u8];

            std::thread::sleep(Duration::from_millis(8));
        }

        let _: () = msg_send![panel, close];
    }

    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
unsafe fn render_macos_view(
    view: *mut objc::runtime::Object,
    combos: &[ShortcutCombo],
    config: &OverlayConfig,
    screen_w: f64,
    screen_h: f64,
) {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use objc::sel;
    use objc::sel_impl;

    // Render to Cairo ImageSurface
    let surf_w = screen_w as i32;
    let surf_h = screen_h as i32;

    if let Ok(surface) = cairo::ImageSurface::create(cairo::Format::ARgb32, surf_w, surf_h) {
        {
            if let Ok(cr) = cairo::Context::new(&surface) {
                render_keycaps_cairo(&cr, combos, config, surf_w as f64, surf_h as f64);
            }
        }
        surface.flush();

        if let Ok(data) = surface.data() {
            let width = surface.width() as usize;
            let height = surface.height() as usize;
            let bytes_per_row = surface.stride() as usize;

            // Create NSBitmapImageRep from Cairo data
            let bitmap_class = Class::get("NSBitmapImageRep").unwrap();
            let bitmap: *mut Object = msg_send![bitmap_class, alloc];

            // Convert ARGB to RGBA for NSBitmapImageRep
            let mut rgba_data = Vec::with_capacity(data.len());
            for chunk in data.chunks_exact(4) {
                let a = chunk[3];
                let r = chunk[2];
                let g = chunk[1];
                let b = chunk[0];
                rgba_data.push(r);
                rgba_data.push(g);
                rgba_data.push(b);
                rgba_data.push(a);
            }

            let bitmap: *mut Object = msg_send![bitmap,
                initWithBitmapDataPlanes: &rgba_data.as_ptr()
                pixelsWide: width
                pixelsHigh: height
                bitsPerSample: 8
                samplesPerPixel: 4
                hasAlpha: 1u8
                isPlanar: 0u8
                colorSpaceName: NSString::get("NSCalibratedRGBColorSpace")
                bytesPerRow: width * 4
                bitsPerPixel: 32
            ];

            // Create NSImage and set as layer contents
            let image_class = Class::get("NSImage").unwrap();
            let image: *mut Object = msg_send![image_class, alloc];
            let size = NSSize {
                width: screen_w,
                height: screen_h,
            };
            let image: *mut Object = msg_send![image, initWithSize: size];
            let _: () = msg_send![image, addRepresentation: bitmap];

            let layer: *mut Object = msg_send![view, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setContents: image];
            }

            let _: () = msg_send![bitmap, release];
            let _: () = msg_send![image, release];
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
unsafe fn render_macos_view(
    _view: isize,
    _combos: &[ShortcutCombo],
    _config: &OverlayConfig,
    _screen_w: f64,
    _screen_h: f64,
) {
}

// ── Cairo rendering (shared with X11 and macOS) ────────────────

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(dead_code)]
fn render_keycaps_cairo(
    cr: &cairo::Context,
    combos: &[ShortcutCombo],
    config: &OverlayConfig,
    _surf_w: f64,
    surf_h: f64,
) {
    use input_core::overlay::OverlayPosition;

    let font_size = config.text.size.unwrap_or(config.scale.font_size()) as f64;
    let padding_x = config.scale.padding() as f64 * 1.5;
    let padding_y = config.scale.padding() as f64;
    let corner_radius = font_size * config.border.radius as f64;

    let keycap_primary = parse_hex_color_rgb(&config.colors.keycap_primary);
    let keycap_secondary = parse_hex_color_rgb(&config.colors.keycap_secondary);
    let modifier_primary = parse_hex_color_rgb(&config.colors.modifier_primary);
    let modifier_secondary = parse_hex_color_rgb(&config.colors.modifier_secondary);
    let text_color = parse_hex_color_rgb(&config.text.color);
    let modifier_text_color = parse_hex_color_rgb(&config.text.modifier_color);
    let border_color = parse_hex_color_rgb(&config.border.color);
    let modifier_border_color = parse_hex_color_rgb(&config.border.modifier_color);

    // Clear
    cr.set_operator(cairo::Operator::Clear);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    let _ = cr.paint();
    cr.set_operator(cairo::Operator::Over);

    let visible: Vec<&ShortcutCombo> = combos.iter().take(5).collect();
    if visible.is_empty() {
        return;
    }

    cr.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    cr.set_font_size(font_size);

    let keycap_h = font_size + padding_y * 2.0;
    let content_h =
        visible.len() as f64 * keycap_h + (visible.len().saturating_sub(1)) as f64 * 8.0;

    let mut y = match config.position {
        OverlayPosition::TopLeft | OverlayPosition::TopRight | OverlayPosition::TopCenter => {
            config.margin_y as f64
        }
        _ => (surf_h - content_h - config.margin_y as f64).max(0.0),
    };

    for (row_idx, combo) in visible.iter().enumerate() {
        let parts = combo_to_key_parts(combo, &config.text.variant);
        if parts.is_empty() {
            continue;
        }
        let is_seq = combo.is_sequence();
        let row_opacity = (1.0 - row_idx as f32 * 0.15).max(0.3);

        let mut x = match config.position {
            OverlayPosition::TopLeft | OverlayPosition::BottomLeft => config.margin_x as f64,
            _ => config.margin_x as f64, // Simplified positioning
        };

        for (i, label) in parts.iter().enumerate() {
            let kw = measure_text_width_cairo(cr, label, font_size) + padding_x * 2.0;
            let is_mod = is_modifier_label(label);

            let (bg_r, bg_g, bg_b, bg2_r, bg2_g, bg2_b, brd_r, brd_g, brd_b, txt_r, txt_g, txt_b) =
                if is_mod && config.colors.highlight_modifiers {
                    (
                        modifier_primary.0,
                        modifier_primary.1,
                        modifier_primary.2,
                        modifier_secondary.0,
                        modifier_secondary.1,
                        modifier_secondary.2,
                        modifier_border_color.0,
                        modifier_border_color.1,
                        modifier_border_color.2,
                        modifier_text_color.0,
                        modifier_text_color.1,
                        modifier_text_color.2,
                    )
                } else {
                    (
                        keycap_primary.0,
                        keycap_primary.1,
                        keycap_primary.2,
                        keycap_secondary.0,
                        keycap_secondary.1,
                        keycap_secondary.2,
                        border_color.0,
                        border_color.1,
                        border_color.2,
                        text_color.0,
                        text_color.1,
                        text_color.2,
                    )
                };

            // Shadow
            if config.keycap_style != KeycapStyle::Minimal {
                cr.new_path();
                draw_rounded_rect(cr, x + 2.0, y + 3.0, kw, keycap_h, corner_radius);
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.4 * row_opacity as f64);
                let _ = cr.fill();
            }

            // Background
            cr.new_path();
            draw_rounded_rect(cr, x, y, kw, keycap_h, corner_radius);

            match config.keycap_style {
                KeycapStyle::Minimal => {
                    let _ = cr.set_source_rgba(bg_r, bg_g, bg_b, row_opacity as f64 * 0.9);
                    let _ = cr.fill();
                }
                KeycapStyle::LowProfile => {
                    let _ = cr.set_source_rgba(
                        bg_r * 0.8,
                        bg_g * 0.8,
                        bg_b * 0.8,
                        row_opacity as f64 * 0.85,
                    );
                    let _ = cr.fill();
                }
                _ => {
                    if config.colors.use_gradient {
                        let pattern = cairo::LinearGradient::new(0.0, y, 0.0, y + keycap_h);
                        pattern.add_color_stop_rgba(
                            0.0,
                            bg2_r,
                            bg2_g,
                            bg2_b,
                            row_opacity as f64 * 0.95,
                        );
                        pattern.add_color_stop_rgba(
                            1.0,
                            bg_r,
                            bg_g,
                            bg_b,
                            row_opacity as f64 * 0.9,
                        );
                        let _ = cr.set_source(&pattern);
                    } else {
                        let _ = cr.set_source_rgba(bg_r, bg_g, bg_b, row_opacity as f64 * 0.9);
                    }
                    let _ = cr.fill_preserve();
                }
            }

            // Border
            if config.keycap_style != KeycapStyle::Minimal && config.border.enabled {
                cr.set_source_rgba(brd_r, brd_g, brd_b, row_opacity as f64 * 0.6);
                cr.set_line_width(config.border.width as f64);
                let _ = cr.stroke();
            }

            // Highlight
            if matches!(config.keycap_style, KeycapStyle::Laptop | KeycapStyle::PBT) {
                cr.new_path();
                draw_rounded_rect(
                    cr,
                    x + 1.0,
                    y + 1.0,
                    kw - 2.0,
                    keycap_h * 0.4,
                    corner_radius - 1.0,
                );
                let highlight = cairo::LinearGradient::new(0.0, y, 0.0, y + keycap_h * 0.4);
                highlight.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.08 * row_opacity as f64);
                highlight.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
                let _ = cr.set_source(&highlight);
                let _ = cr.fill();
            }

            // Text
            let display_label = apply_text_caps(label, &config.text.caps);
            if let Ok(extents) = cr.text_extents(&display_label) {
                let visual_w = extents.x_bearing() + extents.width();
                let text_x = x + (kw - visual_w) / 2.0 - extents.x_bearing();
                let text_y = y + (keycap_h - extents.height()) / 2.0 - extents.y_bearing();

                if config.keycap_style != KeycapStyle::Minimal {
                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.5 * row_opacity as f64);
                    cr.move_to(text_x + 1.0, text_y + 1.0);
                    let _ = cr.show_text(&display_label);
                }

                cr.set_source_rgba(txt_r, txt_g, txt_b, row_opacity as f64);
                cr.move_to(text_x, text_y);
                let _ = cr.show_text(&display_label);
            }

            x += kw;

            if i < parts.len() - 1 {
                if is_seq {
                    x += 8.0;
                } else {
                    let sep_w = measure_text_width_cairo(cr, "+", font_size);
                    let total_gap = sep_w + 16.0;
                    cr.set_source_rgba(0.55, 0.55, 0.6, row_opacity as f64 * 0.8);
                    let sep_center_x = x + total_gap / 2.0;
                    if let Ok(sep_ext) = cr.text_extents("+") {
                        let sep_x = sep_center_x - sep_ext.width() / 2.0;
                        let sep_y = y + (keycap_h - sep_ext.height()) / 2.0 - sep_ext.y_bearing();
                        cr.move_to(sep_x, sep_y);
                        let _ = cr.show_text("+");
                    }
                    x += total_gap;
                }
            }
        }

        y += keycap_h + 8.0;
    }

    let _ = cr.show_page();
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(dead_code)]
fn measure_text_width_cairo(cr: &cairo::Context, label: &str, font_size: f64) -> f64 {
    if let Ok(extents) = cr.text_extents(label) {
        extents.x_bearing() + extents.width()
    } else {
        label.len() as f64 * font_size * 0.6
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(dead_code)]
fn draw_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

// ── Shared helpers ──────────────────────────────────────────────

#[allow(dead_code)]
fn combo_to_key_parts(combo: &ShortcutCombo, variant: &TextVariant) -> Vec<String> {
    if combo.is_sequence() {
        return combo
            .key_sequence
            .iter()
            .map(|k| apply_text_variant(&k.label(), variant))
            .collect();
    }
    let mut parts = Vec::new();
    if combo.modifiers.ctrl {
        parts.push(apply_modifier_label("Ctrl", variant));
    }
    if combo.modifiers.alt {
        parts.push(apply_modifier_label("Alt", variant));
    }
    if combo.modifiers.shift {
        parts.push(apply_modifier_label("Shift", variant));
    }
    if combo.modifiers.super_key {
        parts.push(apply_modifier_label("Super", variant));
    }
    if let Some(key) = &combo.key {
        let key_label = ShortcutCombo::get_key_display(&combo.modifiers, key);
        parts.push(apply_text_variant(&key_label, variant));
    }
    parts
}

#[allow(dead_code)]
fn apply_text_variant(label: &str, variant: &TextVariant) -> String {
    match variant {
        TextVariant::Full => label.to_string(),
        TextVariant::Short => shorten_label(label),
        TextVariant::Icon => {
            if label.len() <= 2 {
                label.to_string()
            } else {
                shorten_label(label)
            }
        }
    }
}

#[allow(dead_code)]
fn apply_modifier_label(label: &str, variant: &TextVariant) -> String {
    match variant {
        TextVariant::Full => match label {
            "Ctrl" => "Control".to_string(),
            _ => label.to_string(),
        },
        TextVariant::Short | TextVariant::Icon => label.to_string(),
    }
}

#[allow(dead_code)]
fn apply_text_caps(label: &str, caps: &TextCaps) -> String {
    match caps {
        TextCaps::Uppercase => label.to_uppercase(),
        TextCaps::Lowercase => label.to_lowercase(),
        TextCaps::Capitalize => {
            let mut chars = label.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    let rest: String = chars.collect();
                    format!("{}{}", upper, rest)
                }
            }
        }
    }
}

#[allow(dead_code)]
fn shorten_label(label: &str) -> String {
    match label {
        "Control" => "Ctrl".to_string(),
        "Escape" => "Esc".to_string(),
        "Delete" => "Del".to_string(),
        "Insert" => "Ins".to_string(),
        "PageUp" => "PgUp".to_string(),
        "PageDown" => "PgDn".to_string(),
        _ => label.to_string(),
    }
}

#[allow(dead_code)]
fn is_modifier_label(label: &str) -> bool {
    matches!(
        label,
        "Ctrl" | "Alt" | "Shift" | "Super" | "Meta" | "Control"
    )
}

// macOS FFI types (used when compiling on macOS)
#[cfg(target_os = "macos")]
type NSUInteger = u64;
#[cfg(target_os = "macos")]
type BOOL = u8;

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(C)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(C)]
struct NSSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(C)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

#[cfg(target_os = "macos")]
struct NSString;

#[cfg(target_os = "macos")]
impl NSString {
    unsafe fn get(s: &str) -> *mut objc::runtime::Object {
        use objc::msg_send;
        use objc::runtime::Class;
        use objc::sel;
        use objc::sel_impl;
        let ns_string_class = Class::get("NSString").unwrap();
        let utf8_str = std::ffi::CString::new(s).unwrap();
        msg_send![ns_string_class, stringWithUTF8String: utf8_str.as_ptr()]
    }
}
