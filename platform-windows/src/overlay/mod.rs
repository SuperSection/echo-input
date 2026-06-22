use input_core::events::ShortcutCombo;
use input_core::ipc::MessageBus;
use input_core::overlay::{
    DisplayEvent, OverlayConfig, TextCaps, TextVariant,
};
use platform::overlay::{OverlayRenderer, OverlayRendererFactory};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

enum RendererCommand {
    Update(DisplayEvent),
    Stop,
}

pub struct WindowsRenderer {
    bus: Option<MessageBus>,
    cmd_tx: Option<mpsc::UnboundedSender<RendererCommand>>,
    handle: Option<tokio::task::JoinHandle<()>>,
    shutdown: Option<Arc<AtomicBool>>,
}

impl WindowsRenderer {
    pub fn new(bus: MessageBus) -> Self {
        Self { bus: Some(bus), cmd_tx: None, handle: None, shutdown: None }
    }

    pub fn with_shutdown(bus: MessageBus, shutdown: Arc<AtomicBool>) -> Self {
        Self { bus: Some(bus), cmd_tx: None, handle: None, shutdown: Some(shutdown) }
    }
}

#[async_trait::async_trait]
impl OverlayRenderer for WindowsRenderer {
    async fn start(&mut self, config: OverlayConfig) -> anyhow::Result<()> {
        let bus = self.bus.take().ok_or_else(|| anyhow::anyhow!("No MessageBus"))?;
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        self.cmd_tx = Some(cmd_tx);
        let shutdown = self.shutdown.take().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        let handle = tokio::task::spawn_blocking(move || {
            #[cfg(target_os = "windows")]
            {
                if let Err(e) = run_windows_overlay(bus, config, cmd_rx, shutdown) {
                    error!("Windows overlay error: {}", e);
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                warn!("Windows overlay not available on this platform");
                let _ = (bus, config, cmd_rx, shutdown);
            }
        });

        self.handle = Some(handle);
        info!("Windows overlay started");
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(RendererCommand::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        info!("Windows overlay stopped");
        Ok(())
    }

    fn update(&self, event: DisplayEvent) -> anyhow::Result<()> {
        if let Some(tx) = &self.cmd_tx {
            tx.send(RendererCommand::Update(event))
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        }
        Ok(())
    }

    fn is_running(&self) -> bool { self.handle.is_some() }
    fn name(&self) -> &str { "WindowsRenderer" }
}

pub struct WindowsRendererFactory;

impl WindowsRendererFactory {
    pub fn new() -> Self { Self }
}

impl Default for WindowsRendererFactory {
    fn default() -> Self { Self::new() }
}

impl OverlayRendererFactory for WindowsRendererFactory {
    fn create(&self, bus: MessageBus) -> Box<dyn OverlayRenderer> {
        Box::new(WindowsRenderer::new(bus))
    }
    fn platform_name(&self) -> &str { "windows" }
}

// ── Hex color parsing ──────────────────────────────────────────

fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        6 | 8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            (r, g, b)
        }
        _ => (255, 255, 255),
    }
}

// ── Windows overlay implementation ─────────────────────────────

#[cfg(target_os = "windows")]
fn run_windows_overlay(
    _bus: MessageBus,
    initial_config: OverlayConfig,
    mut cmd_rx: mpsc::UnboundedReceiver<RendererCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe {
        // Set DPI awareness
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);

        // Register window class
        let class_name = windows::core::w!("EchoInputOverlay");
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = Some(overlay_wndproc);
        wc.hInstance = GetModuleHandleW(None).into();
        wc.lpszClassName = class_name;
        wc.hbrBackground = GetStockObject(NULL_BRUSH);

        RegisterClassExW(&wc);

        // Get screen dimensions
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        // Create the overlay window
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            windows::core::w!("EchoInput Overlay"),
            WS_POPUP,
            0, 0, screen_w, screen_h,
            None,
            None,
            GetModuleHandleW(None),
            None,
        )?;

        // Make window transparent
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

        ShowWindow(hwnd, SW_SHOWNA);
        UpdateWindow(hwnd);

        let mut config = initial_config;
        let mut current_combos: Vec<ShortcutCombo> = Vec::new();
        let mut running = true;

        let mut msg: MSG = std::mem::zeroed();

        info!("Windows overlay running — press keys to see visualization");

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    running = false;
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            if !running {
                break;
            }

            // Handle commands
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    RendererCommand::Update(event) => match event {
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
                                        render_frame(hwnd, &current_combos, &config);
                                        continue;
                                    }
                                }
                            }
                            current_combos.clear();
                            current_combos.push(combo);
                            render_frame(hwnd, &current_combos, &config);
                        }
                        DisplayEvent::History(combos) => {
                            current_combos = combos;
                            render_frame(hwnd, &current_combos, &config);
                        }
                        DisplayEvent::Clear => {
                            current_combos.clear();
                            hide_window(hwnd);
                        }
                        DisplayEvent::UpdateConfig(new_config) => {
                            config = new_config;
                        }
                    },
                    RendererCommand::Stop => {
                        running = false;
                        break;
                    }
                }
            }

            if current_combos.is_empty() {
                hide_window(hwnd);
            }

            std::thread::sleep(Duration::from_millis(8));
        }

        DestroyWindow(hwnd);
    }

    Ok(())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    w_param: windows::Win32::Foundation::WPARAM,
    l_param: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::*;
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}

#[cfg(target_os = "windows")]
unsafe fn render_frame(
    hwnd: windows::Win32::Foundation::HWND,
    combos: &[ShortcutCombo],
    config: &OverlayConfig,
) {
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);

    let hdc_screen = GetDC(None);
    let hdc_mem = CreateCompatibleDC(hdc_screen);

    let mut bmi: BITMAPINFOHEADER = std::mem::zeroed();
    bmi.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.biWidth = screen_w;
    bmi.biHeight = -screen_h; // top-down
    bmi.biPlanes = 1;
    bmi.biBitCount = 32;
    bmi.biCompression = BI_RGB;

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let hbitmap = CreateDIBSection(hdc_screen, &bmi, DIB_RGB_COLORS, &mut bits, 0, 0);
    let old_bitmap = SelectObject(hdc_mem, hbitmap);

    // Clear to transparent
    let clear_brush = CreateSolidBrush(COLORREF(0x00000000));
    let empty_rect = RECT { left: 0, top: 0, right: screen_w, bottom: screen_h };
    FillRect(hdc_mem, &empty_rect, clear_brush);
    DeleteObject(clear_brush);

    // Render keycaps
    render_keycaps_gdi(hdc_mem, combos, config, screen_w, screen_h);

    // Present with per-pixel alpha
    let mut ppt_dst = POINT { x: 0, y: 0 };
    let mut size = SIZE { cx: screen_w, cy: screen_h };
    let mut ppt_src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA,
    };

    let _ = UpdateLayeredWindow(hwnd, hdc_screen, None, Some(&size), hdc_mem, Some(&ppt_src), 0, Some(&blend), ULW_ALPHA);

    // Cleanup
    SelectObject(hdc_mem, old_bitmap);
    DeleteObject(hbitmap);
    DeleteDC(hdc_mem);
    ReleaseDC(None, hdc_screen);
}

#[cfg(target_os = "windows")]
unsafe fn render_keycaps_gdi(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    combos: &[ShortcutCombo],
    config: &OverlayConfig,
    screen_w: i32,
    screen_h: i32,
) {
    use windows::Win32::Graphics::Gdi::*;

    let font_size = config.text.size.unwrap_or(config.scale.font_size()) as i32;
    let padding_x = config.scale.padding() as i32;
    let padding_y = config.scale.padding() as i32;
    let keycap_h = font_size + padding_y * 2;
    let corner_radius = (font_size as f64 * config.border.radius as f64) as i32;

    let (keycap_bg, _) = parse_hex_color(&config.colors.keycap_primary);
    let (keycap_bg2, _) = parse_hex_color(&config.colors.keycap_secondary);
    let (mod_bg, _) = parse_hex_color(&config.colors.modifier_primary);
    let (txt_r, txt_g, txt_b) = parse_hex_color(&config.text.color);
    let (mod_txt_r, mod_txt_g, mod_txt_b) = parse_hex_color(&config.text.modifier_color);

    // Create font
    let font_name = windows::core::w!("Segoe UI");
    let hfont = CreateFontW(
        font_size, 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0,
        DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32,
        DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
        font_name,
    );
    let old_font = SelectObject(hdc, hfont);

    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, RGB(txt_r, txt_g, txt_b));

    let visible: Vec<&ShortcutCombo> = combos.iter().take(5).collect();
    let mut y = match config.position {
        OverlayPosition::TopLeft | OverlayPosition::TopRight | OverlayPosition::TopCenter => {
            config.margin_y as i32
        }
        _ => {
            let content_h = visible.len() as i32 * keycap_h + (visible.len().saturating_sub(1)) as i32 * 8;
            (screen_h - content_h - config.margin_y as i32).max(0)
        }
    };

    for (row_idx, combo) in visible.iter().enumerate() {
        let parts = combo_to_key_parts(combo, &config.text.variant);
        if parts.is_empty() { continue; }

        let is_seq = combo.is_sequence();
        let mut x = match config.position {
            OverlayPosition::TopLeft | OverlayPosition::BottomLeft => config.margin_x as i32,
            OverlayPosition::TopRight | OverlayPosition::BottomRight => {
                // Approximate width
                (screen_w - 200 - config.margin_x as i32).max(0)
            }
            _ => ((screen_w - 200) / 2).max(0),
        };

        let row_alpha = (255.0 * (1.0 - row_idx as f32 * 0.15).max(0.3)) as u8;

        for (i, label) in parts.iter().enumerate() {
            let label_w = measure_text_width_gdi(hdc, label) + padding_x * 2;
            let is_mod = is_modifier_label(label);

            let bg = if is_mod && config.colors.highlight_modifiers { mod_bg } else { keycap_bg };
            let bg_color = COLORREF((bg as u32) | ((bg as u32) << 8) | ((bg as u32) << 16));

            // Draw keycap background
            let brush = CreateSolidBrush(bg_color);
            let pen = if config.border.enabled {
                let (br, bg2, bb) = parse_hex_color(&config.border.color);
                CreatePen(PS_SOLID, config.border.width as i32, COLORREF((br as u32) | ((bg2 as u32) << 8) | ((bb as u32) << 16)))
            } else {
                CreatePen(PS_SOLID, 0, COLORREF(0))
            };
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, pen);

            RoundRect(hdc, x, y, x + label_w, y + keycap_h, corner_radius, corner_radius);

            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(brush);
            DeleteObject(pen);

            // Draw text
            if is_mod && config.colors.highlight_modifiers {
                SetTextColor(hdc, RGB(mod_txt_r, mod_txt_g, mod_txt_b));
            } else {
                SetTextColor(hdc, RGB(txt_r, txt_g, txt_b));
            }

            let display_label = apply_text_caps(label, &config.text.caps);
            let text_x = x + padding_x;
            let text_y = y + padding_y;
            TextOutW(hdc, text_x, text_y, &display_label.encode_utf16().collect::<Vec<u16>>());

            x += label_w;

            // Separator
            if i < parts.len() - 1 {
                if is_seq {
                    x += 8;
                } else {
                    let sep_w = measure_text_width_gdi(hdc, "+");
                    let sep_x = x + 4;
                    let sep_y = y + padding_y;
                    SetTextColor(hdc, RGB(140, 140, 153));
                    TextOutW(hdc, sep_x, sep_y, &"+".encode_utf16().collect::<Vec<u16>>());
                    SetTextColor(hdc, RGB(txt_r, txt_g, txt_b));
                    x += sep_w + 16;
                }
            }
        }

        y += keycap_h + 8;
    }

    SelectObject(hdc, old_font);
    DeleteObject(hfont);
}

#[cfg(target_os = "windows")]
unsafe fn measure_text_width_gdi(hdc: windows::Win32::Graphics::Gdi::HDC, label: &str) -> i32 {
    use windows::Win32::Graphics::Gdi::*;
    let mut size: SIZE = std::mem::zeroed();
    let utf16: Vec<u16> = label.encode_utf16().collect();
    GetTextExtentPoint32W(hdc, &utf16, &mut size);
    size.cx
}

#[cfg(target_os = "windows")]
unsafe fn hide_window(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    ShowWindow(hwnd, SW_HIDE);
}

#[cfg(not(target_os = "windows"))]
unsafe fn hide_window(_hwnd: isize) {}

// ── Cross-platform helpers ──────────────────────────────────────

fn combo_to_key_parts(combo: &ShortcutCombo, variant: &TextVariant) -> Vec<String> {
    if combo.is_sequence() {
        return combo.key_sequence.iter().map(|k| apply_text_variant(&k.label(), variant)).collect();
    }
    let mut parts = Vec::new();
    if combo.modifiers.ctrl { parts.push(apply_modifier_label("Ctrl", variant)); }
    if combo.modifiers.alt { parts.push(apply_modifier_label("Alt", variant)); }
    if combo.modifiers.shift { parts.push(apply_modifier_label("Shift", variant)); }
    if combo.modifiers.super_key { parts.push(apply_modifier_label("Super", variant)); }
    if let Some(key) = &combo.key { parts.push(apply_text_variant(&key.label(), variant)); }
    parts
}

fn apply_text_variant(label: &str, variant: &TextVariant) -> String {
    match variant {
        TextVariant::Full => label.to_string(),
        TextVariant::Short => shorten_label(label),
        TextVariant::Icon => if label.len() <= 2 { label.to_string() } else { shorten_label(label) },
    }
}

fn apply_modifier_label(label: &str, variant: &TextVariant) -> String {
    match variant {
        TextVariant::Full => match label { "Ctrl" => "Control".to_string(), _ => label.to_string() },
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
        _ => label.to_string(),
    }
}

fn is_modifier_label(label: &str) -> bool {
    matches!(label, "Ctrl" | "Alt" | "Shift" | "Super" | "Meta" | "Control")
}
