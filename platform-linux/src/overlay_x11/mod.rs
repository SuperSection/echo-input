use input_core::events::ShortcutCombo;
use input_core::ipc::MessageBus;
use input_core::overlay::{
    DisplayEvent, KeycapStyle, OverlayConfig, OverlayPosition, TextCaps, TextVariant,
};
use platform::overlay::{OverlayRenderer, OverlayRendererFactory};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

const KEYCAP_GAP: f64 = 8.0;
const ROW_GAP: f64 = 8.0;
const MAX_HISTORY_ROWS: usize = 5;
const MAX_SEQUENCE_LENGTH: usize = 7;

fn parse_hex_color(hex: &str) -> (f64, f64, f64, f64) {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f64 / 255.0;
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f64 / 255.0;
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f64 / 255.0;
            (r, g, b, 1.0)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f64 / 255.0;
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f64 / 255.0;
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f64 / 255.0;
            let a = u8::from_str_radix(&hex[6..8], 16).unwrap_or(255) as f64 / 255.0;
            (r, g, b, a)
        }
        _ => (1.0, 1.0, 1.0, 1.0),
    }
}

enum RendererCommand {
    Update(DisplayEvent),
    Stop,
}

pub struct X11Renderer {
    bus: Option<MessageBus>,
    cmd_tx: Option<mpsc::UnboundedSender<RendererCommand>>,
    handle: Option<tokio::task::JoinHandle<()>>,
    shutdown: Option<Arc<AtomicBool>>,
}

impl X11Renderer {
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
impl OverlayRenderer for X11Renderer {
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
            if let Err(e) = run_x11_event_loop(bus, config, cmd_rx, shutdown) {
                error!("X11 event loop error: {}", e);
            }
        });

        self.handle = Some(handle);
        info!("X11 overlay started");
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(RendererCommand::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        info!("X11 overlay stopped");
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
        "X11Renderer"
    }
}

pub struct X11RendererFactory;

impl X11RendererFactory {
    pub fn new() -> Self {
        Self
    }
}

impl Default for X11RendererFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayRendererFactory for X11RendererFactory {
    fn create(&self, bus: MessageBus) -> Box<dyn OverlayRenderer> {
        Box::new(X11Renderer::new(bus))
    }
    fn platform_name(&self) -> &str {
        "x11"
    }
}

// ── X11 event loop ─────────────────────────────────────────────

fn run_x11_event_loop(
    _bus: MessageBus,
    initial_config: OverlayConfig,
    mut cmd_rx: mpsc::UnboundedReceiver<RendererCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    use x11rb::rust_connection::RustConnection;

    let (conn, screen_num) = RustConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num as usize];
    let root = screen.root;

    let window = conn.generate_id()?;
    let depth = screen.root_depth;
    let visual = screen.root_visual;

    // Create override_redirect window (bypasses window manager)
    let values = CreateWindowAux::new()
        .override_redirect(1)
        .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY)
        .background_pixel(0);

    conn.create_window(
        depth,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        visual,
        &values,
    )?;

    // Set _NET_WM_STATE_ABOVE
    let wm_state = conn.intern_atom(false, b"_NET_WM_STATE")?;
    let wm_state_above = conn.intern_atom(false, b"_NET_WM_STATE_ABOVE")?;
    let wm_state_skip_taskbar = conn.intern_atom(false, b"_NET_WM_STATE_SKIP_TASKBAR")?;
    let atom_atom = conn.intern_atom(false, b"ATOM")?;

    let atoms = [
        wm_state_above.reply()?.atom,
        wm_state_skip_taskbar.reply()?.atom,
    ];
    let atoms_bytes: Vec<u8> = atoms.iter().flat_map(|a| a.to_ne_bytes()).collect();
    conn.change_property(
        PropMode::REPLACE,
        window,
        wm_state.reply()?.atom,
        atom_atom.reply()?.atom,
        32,
        atoms.len() as u32,
        &atoms_bytes,
    )?;

    conn.map_window(window)?;
    conn.flush()?;

    let mut config = initial_config;
    let mut current_combos: Vec<ShortcutCombo> = Vec::new();
    let mut running = true;
    let mut screen_width = screen.width_in_pixels as i32;
    let mut screen_height = screen.height_in_pixels as i32;

    info!("X11 overlay running — press keys to see visualization");

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Handle X events
        while let Some(event) = conn.poll_for_event()? {
            match event {
                x11rb::protocol::Event::ConfigureNotify(e) => {
                    screen_width = e.width as i32;
                    screen_height = e.height as i32;
                }
                _ => {}
            }
        }

        // Handle commands
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                RendererCommand::Update(event) => match event {
                    DisplayEvent::Shortcut(combo) => {
                        if combo.modifiers.is_empty() && combo.resolved_text.is_some() {
                            if let Some(front) = current_combos.first() {
                                if front.modifiers.is_empty() {
                                    let mut keys = front.key_sequence.clone();
                                    let mut chars = front.resolved_chars.clone();
                                    if keys.is_empty() {
                                        if let Some(prev_key) = front.key {
                                            keys.push(prev_key);
                                            if let Some(ref rt) = front.resolved_text {
                                                chars.push(rt.clone());
                                            }
                                        }
                                    }
                                    if let Some(new_key) = combo.key {
                                        keys.push(new_key);
                                    }
                                    if let Some(ref new_char) = combo.resolved_text {
                                        chars.push(new_char.clone());
                                    }
                                    while keys.len() > MAX_SEQUENCE_LENGTH {
                                        keys.remove(0);
                                        if !chars.is_empty() {
                                            chars.remove(0);
                                        }
                                    }
                                    let merged = if !chars.is_empty() {
                                        ShortcutCombo::resolved_sequence(keys, chars)
                                    } else {
                                        ShortcutCombo::sequence(keys)
                                    };
                                    current_combos[0] = merged;
                                    let _ = render_x11(
                                        &conn,
                                        window,
                                        &current_combos,
                                        &config,
                                        screen_width,
                                        screen_height,
                                    );
                                    continue;
                                }
                            }
                        }
                        current_combos.clear();
                        current_combos.push(combo);
                        let _ = render_x11(
                            &conn,
                            window,
                            &current_combos,
                            &config,
                            screen_width,
                            screen_height,
                        );
                    }
                    DisplayEvent::History(combos) => {
                        current_combos = combos;
                        let _ = render_x11(
                            &conn,
                            window,
                            &current_combos,
                            &config,
                            screen_width,
                            screen_height,
                        );
                    }
                    DisplayEvent::Clear => {
                        current_combos.clear();
                        let _ = clear_x11(&conn, window);
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

        if !running {
            break;
        }

        if current_combos.is_empty() {
            let _ = clear_x11(&conn, window);
        }

        conn.flush()?;
        std::thread::sleep(Duration::from_millis(8));
    }

    conn.destroy_window(window)?;
    conn.flush()?;
    debug!("X11 event loop ended");
    Ok(())
}

// ── X11 rendering ──────────────────────────────────────────────

fn render_x11(
    conn: &x11rb::rust_connection::RustConnection,
    window: u32,
    combos: &[ShortcutCombo],
    config: &OverlayConfig,
    screen_width: i32,
    screen_height: i32,
) -> anyhow::Result<()> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;

    let (font_size, padding_x, padding_y, corner_radius) = keycap_dimensions(config);

    let visible: Vec<&ShortcutCombo> = combos.iter().take(MAX_HISTORY_ROWS).collect();
    if visible.is_empty() {
        return clear_x11(conn, window);
    }

    // Compute content size using temporary Cairo surface
    let (content_w, content_h) =
        compute_content_size(&visible, config, font_size, padding_x, padding_y);

    let surf_w = (content_w + config.margin_x as f64 * 2.0).ceil() as u32;
    let surf_h = (content_h + config.margin_y as f64 * 2.0).ceil() as u32;

    if surf_w == 0 || surf_h == 0 {
        return clear_x11(conn, window);
    }

    // Position
    let x = match config.position {
        OverlayPosition::TopLeft | OverlayPosition::BottomLeft => config.margin_x as i32,
        OverlayPosition::TopRight | OverlayPosition::BottomRight => {
            (screen_width as f64 - content_w - config.margin_x as f64).max(0.0) as i32
        }
        _ => ((screen_width as f64 - content_w) / 2.0).max(0.0) as i32,
    };

    let y = match config.position {
        OverlayPosition::TopLeft | OverlayPosition::TopRight | OverlayPosition::TopCenter => {
            config.margin_y as i32
        }
        OverlayPosition::BottomLeft
        | OverlayPosition::BottomRight
        | OverlayPosition::BottomCenter => {
            (screen_height as f64 - content_h - config.margin_y as f64).max(0.0) as i32
        }
        _ => ((screen_height as f64 - content_h) / 2.0).max(0.0) as i32,
    };

    conn.configure_window(
        window,
        &ConfigureWindowAux::new()
            .x(x)
            .y(y)
            .width(surf_w)
            .height(surf_h),
    )?;

    // Render to Cairo ImageSurface
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, surf_w as i32, surf_h as i32)?;
    {
        let cr = cairo::Context::new(&surface)?;

        // Clear
        let _ = cr.set_operator(cairo::Operator::Clear);
        let _ = cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();

        let _ = cr.set_operator(cairo::Operator::Over);

        cr.select_font_face(
            "sans-serif",
            cairo::FontSlant::Normal,
            cairo::FontWeight::Bold,
        );
        cr.set_font_size(font_size);

        let keycap_h = font_size + padding_y * 2.0;
        let mut y_offset = config.margin_y as f64;

        for (row_idx, combo) in visible.iter().enumerate() {
            let parts = combo_to_key_parts(combo, &config.text.variant);
            if parts.is_empty() {
                continue;
            }
            let is_seq = combo.is_sequence();
            let mut x_offset = config.margin_x as f64;
            let row_opacity = (1.0 - row_idx as f32 * 0.15).max(0.3);

            for (i, label) in parts.iter().enumerate() {
                let kw = measure_text_width(&cr, label, font_size) + padding_x * 2.0;
                let is_modifier = is_modifier_label(label);

                let keycap_primary = parse_hex_color(&config.colors.keycap_primary);
                let keycap_secondary = parse_hex_color(&config.colors.keycap_secondary);
                let modifier_primary = parse_hex_color(&config.colors.modifier_primary);
                let modifier_secondary = parse_hex_color(&config.colors.modifier_secondary);
                let text_color = parse_hex_color(&config.text.color);
                let modifier_text_color = parse_hex_color(&config.text.modifier_color);
                let border_color = parse_hex_color(&config.border.color);
                let modifier_border_color = parse_hex_color(&config.border.modifier_color);

                let (
                    bg_r,
                    bg_g,
                    bg_b,
                    bg2_r,
                    bg2_g,
                    bg2_b,
                    brd_r,
                    brd_g,
                    brd_b,
                    txt_r,
                    txt_g,
                    txt_b,
                ) = if is_modifier && config.colors.highlight_modifiers {
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
                    draw_rounded_rect(
                        &cr,
                        x_offset + 2.0,
                        y_offset + 3.0,
                        kw,
                        keycap_h,
                        corner_radius,
                    );
                    let _ = cr.set_source_rgba(0.0, 0.0, 0.0, 0.4 * row_opacity as f64);
                    let _ = cr.fill();
                }

                // Keycap background
                cr.new_path();
                draw_rounded_rect(&cr, x_offset, y_offset, kw, keycap_h, corner_radius);

                match config.keycap_style {
                    KeycapStyle::Minimal => {
                        let _ = cr.set_source_rgba(bg_r, bg_g, bg_b, row_opacity as f64 * 0.9);
                        let _ = cr.fill();
                    }
                    KeycapStyle::LowProfile => {
                        let darken_factor = 0.2;
                        let _ = cr.set_source_rgba(
                            bg_r * (1.0 - darken_factor),
                            bg_g * (1.0 - darken_factor),
                            bg_b * (1.0 - darken_factor),
                            row_opacity as f64 * 0.85,
                        );
                        let _ = cr.fill();
                    }
                    _ => {
                        if config.colors.use_gradient {
                            let pattern =
                                cairo::LinearGradient::new(0.0, y_offset, 0.0, y_offset + keycap_h);
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
                    let _ = cr.set_source_rgba(brd_r, brd_g, brd_b, row_opacity as f64 * 0.6);
                    cr.set_line_width(config.border.width as f64);
                    let _ = cr.stroke();
                }

                // Highlight (laptop/PBT style)
                if matches!(config.keycap_style, KeycapStyle::Laptop | KeycapStyle::PBT) {
                    cr.new_path();
                    draw_rounded_rect(
                        &cr,
                        x_offset + 1.0,
                        y_offset + 1.0,
                        kw - 2.0,
                        keycap_h * 0.4,
                        corner_radius - 1.0,
                    );
                    let highlight =
                        cairo::LinearGradient::new(0.0, y_offset, 0.0, y_offset + keycap_h * 0.4);
                    highlight.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.08 * row_opacity as f64);
                    highlight.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
                    let _ = cr.set_source(&highlight);
                    let _ = cr.fill();
                }

                // Text
                let display_label = apply_text_caps(label, &config.text.caps);
                if let Ok(extents) = cr.text_extents(&display_label) {
                    let visual_w = extents.x_bearing() + extents.width();
                    let text_x = x_offset + (kw - visual_w) / 2.0 - extents.x_bearing();
                    let text_y =
                        y_offset + (keycap_h - extents.height()) / 2.0 - extents.y_bearing();

                    // Text shadow
                    if config.keycap_style != KeycapStyle::Minimal {
                        let _ = cr.set_source_rgba(0.0, 0.0, 0.0, 0.5 * row_opacity as f64);
                        cr.move_to(text_x + 1.0, text_y + 1.0);
                        let _ = cr.show_text(&display_label);
                    }

                    let _ = cr.set_source_rgba(txt_r, txt_g, txt_b, row_opacity as f64);
                    cr.move_to(text_x, text_y);
                    let _ = cr.show_text(&display_label);
                }

                x_offset += kw;

                // Separator
                if i < parts.len() - 1 {
                    if is_seq {
                        x_offset += KEYCAP_GAP;
                    } else {
                        let sep_w = measure_text_width(&cr, "+", font_size);
                        let total_gap = sep_w + KEYCAP_GAP * 2.0;
                        let sep_center_x = x_offset + total_gap / 2.0;
                        if let Ok(sep_ext) = cr.text_extents("+") {
                            let sep_visual_w = sep_ext.x_bearing() + sep_ext.width();
                            let sep_x = sep_center_x - sep_visual_w / 2.0;
                            let sep_y = y_offset + (keycap_h - sep_ext.height()) / 2.0
                                - sep_ext.y_bearing();
                            let _ = cr.set_source_rgba(0.55, 0.55, 0.6, row_opacity as f64 * 0.8);
                            cr.move_to(sep_x, sep_y);
                            let _ = cr.show_text("+");
                        }
                        x_offset += total_gap;
                    }
                }
            }

            y_offset += keycap_h + ROW_GAP;
        }

        let _ = cr.show_page();
    }

    surface.flush();

    // Copy Cairo surface to X11 window
    let width = surface.width() as u32;
    let height = surface.height() as u32;
    let data = surface.data()?;

    let gc = conn.generate_id()?;
    let depth = conn.setup().roots[0].root_depth;
    conn.create_gc(gc, window, &CreateGCAux::new())?;

    // Convert ARGB32 Cairo data to X11 format (native byte order is BGRA on little-endian)
    let mut x11_data = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(4) {
        let b = chunk[0];
        let g = chunk[1];
        let r = chunk[2];
        let a = chunk[3];
        x11_data.push(b);
        x11_data.push(g);
        x11_data.push(r);
        x11_data.push(a);
    }

    conn.put_image(
        ImageFormat::Z_PIXMAP,
        window,
        gc,
        width as u16,
        height as u16,
        0,
        0,
        0,
        depth,
        &x11_data,
    )?;
    conn.flush()?;

    Ok(())
}

fn clear_x11(conn: &x11rb::rust_connection::RustConnection, window: u32) -> anyhow::Result<()> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    conn.configure_window(
        window,
        &ConfigureWindowAux::new().width(1).height(1).x(-100).y(-100),
    )?;
    conn.flush()?;
    Ok(())
}

// ── Shared helpers ──────────────────────────────────────────────

fn keycap_dimensions(config: &OverlayConfig) -> (f64, f64, f64, f64) {
    let font_size = config.text.size.unwrap_or(config.scale.font_size()) as f64;
    let padding_x = config.scale.padding() as f64 * 1.5;
    let padding_y = config.scale.padding() as f64;
    let corner_radius = font_size * config.border.radius as f64;
    (font_size, padding_x, padding_y, corner_radius)
}

fn compute_content_size(
    combos: &[&ShortcutCombo],
    config: &OverlayConfig,
    font_size: f64,
    padding_x: f64,
    padding_y: f64,
) -> (f64, f64) {
    if combos.is_empty() {
        return (0.0, 0.0);
    }

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).unwrap();
    let cr = cairo::Context::new(&surface).unwrap();
    cr.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    cr.set_font_size(font_size);

    let mut max_row_width = 0.0_f64;

    for combo in combos {
        let parts = combo_to_key_parts(combo, &config.text.variant);
        if parts.is_empty() {
            continue;
        }
        let sep_w = measure_text_width(&cr, "+", font_size);
        let is_seq = combo.is_sequence();
        let mut row_width = 0.0_f64;
        for (i, label) in parts.iter().enumerate() {
            row_width += measure_text_width(&cr, label, font_size) + padding_x * 2.0;
            if i < parts.len() - 1 {
                if is_seq {
                    row_width += KEYCAP_GAP;
                } else {
                    row_width += sep_w + KEYCAP_GAP * 2.0;
                }
            }
        }
        max_row_width = max_row_width.max(row_width);
    }

    let keycap_h = font_size + padding_y * 2.0;
    let content_h =
        combos.len() as f64 * keycap_h + (combos.len().saturating_sub(1)) as f64 * ROW_GAP;

    (max_row_width, content_h)
}

fn measure_text_width(cr: &cairo::Context, label: &str, font_size: f64) -> f64 {
    if let Ok(extents) = cr.text_extents(label) {
        extents.x_bearing() + extents.width()
    } else {
        label.len() as f64 * font_size * 0.6
    }
}

fn combo_to_key_parts(combo: &ShortcutCombo, variant: &TextVariant) -> Vec<String> {
    // Character events: show resolved text as a single keycap
    if let Some(ref text) = combo.resolved_text {
        return vec![text.clone()];
    }

    if combo.is_sequence() {
        if !combo.resolved_chars.is_empty() {
            return combo.resolved_chars.clone();
        }
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

fn apply_modifier_label(label: &str, variant: &TextVariant) -> String {
    match variant {
        TextVariant::Full => match label {
            "Ctrl" => "Control".to_string(),
            _ => label.to_string(),
        },
        TextVariant::Short | TextVariant::Icon => label.to_string(),
    }
}

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

fn shorten_label(label: &str) -> String {
    match label {
        "Control" => "Ctrl".to_string(),
        "Escape" => "Esc".to_string(),
        "Delete" => "Del".to_string(),
        "Insert" => "Ins".to_string(),
        "PageUp" => "PgUp".to_string(),
        "PageDown" => "PgDn".to_string(),
        "CapsLock" => "Caps".to_string(),
        "NumLock" => "Num".to_string(),
        "ScrollLock" => "Scrl".to_string(),
        "PrintScreen" => "PrtSc".to_string(),
        _ => label.to_string(),
    }
}

fn is_modifier_label(label: &str) -> bool {
    matches!(
        label,
        "Ctrl" | "Alt" | "Shift" | "Super" | "Meta" | "Control"
    )
}

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
