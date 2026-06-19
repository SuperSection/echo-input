use crate::animation::Animation;
use crate::error::WaylandError;
use input_core::events::{InputEvent, KeyState, KeyboardEvent, ModifierState, ShortcutCombo};
use input_core::ipc::MessageBus;
use input_core::keys::VirtualKey;
use input_core::overlay::{DisplayEvent, OverlayConfig, OverlayPosition};
use input_core::processor::DefaultEventProcessor;
use input_core::traits::{EventProcessor, ProcessorConfig};
use std::os::unix::io::{AsFd, AsRawFd};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_registry, wl_seat, wl_shm,
    wl_shm_pool, wl_surface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};

const KEYCAP_PADDING_X: f64 = 14.0;
const KEYCAP_PADDING_Y: f64 = 8.0;
const KEYCAP_GAP: f64 = 6.0;
const ROW_GAP: f64 = 6.0;
const SURFACE_MARGIN: f64 = 12.0;
const CORNER_RADIUS: f64 = 8.0;
const FONT_SIZE: f64 = 24.0;
const KEYCAP_BG: (f64, f64, f64) = (0.15, 0.15, 0.15);
const KEYCAP_BORDER: (f64, f64, f64) = (0.3, 0.3, 0.3);
const TEXT_COLOR: (f64, f64, f64) = (0.95, 0.95, 0.95);
const SEP_COLOR: (f64, f64, f64) = (0.6, 0.6, 0.6);
const MAX_HISTORY_ROWS: usize = 3;

enum RendererCommand {
    Update(DisplayEvent),
    Stop,
}

#[derive(Clone)]
struct OutputInfo {
    name: String,
    scale: i32,
    width: i32,
    height: i32,
    proxy_id: u32,
    global_id: u32,
}

struct WaylandGlobals {
    compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1,
}

struct ShmBuffer {
    _file: std::fs::File,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    mmap_ptr: *mut u8,
    mmap_len: usize,
    width: i32,
    height: i32,
}

unsafe impl Send for ShmBuffer {}
unsafe impl Sync for ShmBuffer {}

impl ShmBuffer {
    fn create(
        globals: &WaylandGlobals,
        width: i32,
        height: i32,
        qh: &QueueHandle<AppState>,
    ) -> Result<Self, WaylandError> {
        let stride = width * 4;
        let size = (stride * height) as usize;

        let dir = std::env::temp_dir();
        let file_name = format!("echoinput-shm-{}-{}", std::process::id(), rand_id());
        let file_path = dir.join(&file_name);

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file_path)
            .map_err(|e| WaylandError::ShmAllocation(e.to_string()))?;

        file.set_len(size as u64)
            .map_err(|e| WaylandError::ShmAllocation(e.to_string()))?;

        let mmap_ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };

        if mmap_ptr == libc::MAP_FAILED {
            return Err(WaylandError::ShmAllocation("mmap failed".into()));
        }

        let pool = globals.shm.create_pool(file.as_fd(), size as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            stride,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );

        let _ = std::fs::remove_file(&file_path);

        Ok(Self {
            _file: file,
            pool,
            buffer,
            mmap_ptr: mmap_ptr as *mut u8,
            mmap_len: size,
            width,
            height,
        })
    }

    fn write_pixels(&self, data: &[u8]) {
        let len = data.len().min(self.mmap_len);
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.mmap_ptr, len);
        }
    }
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.mmap_ptr as *mut libc::c_void, self.mmap_len);
        }
        self.buffer.destroy();
        self.pool.destroy();
    }
}

fn rand_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

pub struct WaylandRenderer {
    bus: Option<MessageBus>,
    cmd_tx: Option<mpsc::UnboundedSender<RendererCommand>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl WaylandRenderer {
    pub fn new(bus: MessageBus) -> Self {
        Self {
            bus: Some(bus),
            cmd_tx: None,
            handle: None,
        }
    }
}

#[async_trait::async_trait]
impl input_core::traits::OverlayRenderer for WaylandRenderer {
    async fn start(&mut self, config: OverlayConfig) -> anyhow::Result<()> {
        let bus = self.bus.take().ok_or_else(|| {
            WaylandError::Connection("No MessageBus provided".into())
        })?;
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        self.cmd_tx = Some(cmd_tx);

        let handle = tokio::task::spawn_blocking(move || {
            if let Err(e) = run_wayland_event_loop(bus, config, cmd_rx) {
                error!("Wayland event loop error: {}", e);
            }
        });

        self.handle = Some(handle);
        info!("Overlay started");
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(RendererCommand::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        info!("Overlay stopped");
        Ok(())
    }

    fn update(&self, event: DisplayEvent) -> anyhow::Result<()> {
        if let Some(tx) = &self.cmd_tx {
            tx.send(RendererCommand::Update(event))
                .map_err(|e| WaylandError::ChannelSend(e.to_string()))?;
        }
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    fn name(&self) -> &str {
        "WaylandRenderer"
    }
}

struct AppState {
    outputs: Vec<OutputInfo>,
    output_proxies: Vec<wl_output::WlOutput>,
    configured: bool,
    keyboard_events: Arc<Mutex<Vec<InputEvent>>>,
    xkb_context: Option<xkbcommon::xkb::Context>,
    xkb_keymap: Option<xkbcommon::xkb::Keymap>,
    xkb_state: Option<xkbcommon::xkb::State>,
}

impl AppState {
    fn new() -> Self {
        Self {
            outputs: Vec::new(),
            output_proxies: Vec::new(),
            configured: false,
            keyboard_events: Arc::new(Mutex::new(Vec::new())),
            xkb_context: Some(xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS)),
            xkb_keymap: None,
            xkb_state: None,
        }
    }

    fn find_output(&self, monitor: Option<&str>) -> Option<(usize, &OutputInfo)> {
        match monitor {
            Some(name) => self.outputs.iter().enumerate().find(|(_, o)| o.name == name),
            None => self.outputs.first().map(|o| (0, o)),
        }
    }

    fn find_output_proxy(&self, monitor: Option<&str>) -> Option<&wl_output::WlOutput> {
        let (idx, _) = self.find_output(monitor)?;
        self.output_proxies.get(idx)
    }

    fn find_output_index_by_proxy_id(&self, proxy_id: u32) -> Option<usize> {
        self.outputs.iter().position(|o| o.proxy_id == proxy_id)
    }

    fn keysym_to_key(&self, keysym: xkbcommon::xkb::Keysym) -> VirtualKey {
        use xkbcommon::xkb::keysym_get_name;
        let name = keysym_get_name(keysym);
        match name.as_str() {
            "a" => VirtualKey::A, "b" => VirtualKey::B, "c" => VirtualKey::C,
            "d" => VirtualKey::D, "e" => VirtualKey::E, "f" => VirtualKey::F,
            "g" => VirtualKey::G, "h" => VirtualKey::H, "i" => VirtualKey::I,
            "j" => VirtualKey::J, "k" => VirtualKey::K, "l" => VirtualKey::L,
            "m" => VirtualKey::M, "n" => VirtualKey::N, "o" => VirtualKey::O,
            "p" => VirtualKey::P, "q" => VirtualKey::Q, "r" => VirtualKey::R,
            "s" => VirtualKey::S, "t" => VirtualKey::T, "u" => VirtualKey::U,
            "v" => VirtualKey::V, "w" => VirtualKey::W, "x" => VirtualKey::X,
            "y" => VirtualKey::Y, "z" => VirtualKey::Z,
            "A" => VirtualKey::A, "B" => VirtualKey::B, "C" => VirtualKey::C,
            "D" => VirtualKey::D, "E" => VirtualKey::E, "F" => VirtualKey::F,
            "G" => VirtualKey::G, "H" => VirtualKey::H, "I" => VirtualKey::I,
            "J" => VirtualKey::J, "K" => VirtualKey::K, "L" => VirtualKey::L,
            "M" => VirtualKey::M, "N" => VirtualKey::N, "O" => VirtualKey::O,
            "P" => VirtualKey::P, "Q" => VirtualKey::Q, "R" => VirtualKey::R,
            "S" => VirtualKey::S, "T" => VirtualKey::T, "U" => VirtualKey::U,
            "V" => VirtualKey::V, "W" => VirtualKey::W, "X" => VirtualKey::X,
            "Y" => VirtualKey::Y, "Z" => VirtualKey::Z,
            "0" => VirtualKey::Key0, "1" => VirtualKey::Key1, "2" => VirtualKey::Key2,
            "3" => VirtualKey::Key3, "4" => VirtualKey::Key4, "5" => VirtualKey::Key5,
            "6" => VirtualKey::Key6, "7" => VirtualKey::Key7, "8" => VirtualKey::Key8,
            "9" => VirtualKey::Key9,
            "F1" => VirtualKey::F1, "F2" => VirtualKey::F2, "F3" => VirtualKey::F3,
            "F4" => VirtualKey::F4, "F5" => VirtualKey::F5, "F6" => VirtualKey::F6,
            "F7" => VirtualKey::F7, "F8" => VirtualKey::F8, "F9" => VirtualKey::F9,
            "F10" => VirtualKey::F10, "F11" => VirtualKey::F11, "F12" => VirtualKey::F12,
            "Control_L" => VirtualKey::ControlLeft, "Control_R" => VirtualKey::ControlRight,
            "Shift_L" => VirtualKey::ShiftLeft, "Shift_R" => VirtualKey::ShiftRight,
            "Alt_L" => VirtualKey::AltLeft, "Alt_R" => VirtualKey::AltRight,
            "Super_L" => VirtualKey::SuperLeft, "Super_R" => VirtualKey::SuperRight,
            "Tab" => VirtualKey::Tab, "Escape" => VirtualKey::Escape,
            "space" => VirtualKey::Space, "Return" => VirtualKey::Enter,
            "BackSpace" => VirtualKey::Backspace, "Delete" => VirtualKey::Delete,
            "Insert" => VirtualKey::Insert, "Home" => VirtualKey::Home,
            "End" => VirtualKey::End, "Page_Up" => VirtualKey::PageUp,
            "Page_Down" => VirtualKey::PageDown,
            "Up" => VirtualKey::Up, "Down" => VirtualKey::Down,
            "Left" => VirtualKey::Left, "Right" => VirtualKey::Right,
            "minus" => VirtualKey::Minus, "equal" => VirtualKey::Equal,
            "bracketleft" => VirtualKey::LeftBracket, "bracketright" => VirtualKey::RightBracket,
            "backslash" => VirtualKey::Backslash, "semicolon" => VirtualKey::Semicolon,
            "apostrophe" => VirtualKey::Quote, "comma" => VirtualKey::Comma,
            "period" => VirtualKey::Period, "slash" => VirtualKey::Slash,
            "grave" => VirtualKey::Backtick, "Caps_Lock" => VirtualKey::CapsLock,
            "Scroll_Lock" => VirtualKey::ScrollLock, "Pause" => VirtualKey::Pause,
            "Print" => VirtualKey::PrintScreen,
            _ => VirtualKey::Unknown(0),
        }
    }
}

fn run_wayland_event_loop(
    _bus: MessageBus,
    initial_config: OverlayConfig,
    mut cmd_rx: mpsc::UnboundedReceiver<RendererCommand>,
) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env()
        .map_err(|e| WaylandError::Connection(e.to_string()))?;

    let (globals, mut event_queue) = registry_queue_init::<AppState>(&conn)
        .map_err(|e| WaylandError::Connection(format!("registry_queue_init: {}", e)))?;

    let qh = event_queue.handle();

    let all_globals = globals.contents().clone_list();

    let compositor: wl_compositor::WlCompositor = globals
        .bind(&qh, 4..=5, ())
        .map_err(|e| WaylandError::MissingProtocol(format!("wl_compositor: {}", e)))?;

    let shm: wl_shm::WlShm = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| WaylandError::MissingProtocol(format!("wl_shm: {}", e)))?;

    let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| WaylandError::MissingProtocol(format!("zwlr_layer_shell_v1: {}", e)))?;

    let wayland_globals = WaylandGlobals {
        compositor,
        shm,
        layer_shell,
    };

    let mut state = AppState::new();
    let keyboard_events = state.keyboard_events.clone();

    let registry = globals.registry();
    for g in &all_globals {
        match g.interface.as_str() {
            "wl_output" => {
                let version = g.version.min(4);
                let proxy: wl_output::WlOutput = registry.bind(g.name, version, &qh, ());
                let proxy_id = proxy.id().protocol_id();
                state.outputs.push(OutputInfo {
                    name: String::new(),
                    scale: 1,
                    width: 0,
                    height: 0,
                    proxy_id,
                    global_id: g.name,
                });
                state.output_proxies.push(proxy);
            }
            "wl_seat" => {
                let _seat: wl_seat::WlSeat = registry.bind(g.name, g.version.min(7), &qh, ());
                debug!("Bound wl_seat global");
            }
            _ => {}
        }
    }

    // First roundtrip: discovers outputs, binds seat, creates wl_keyboard
    event_queue
        .roundtrip(&mut state)
        .map_err(|e| WaylandError::Connection(format!("roundtrip failed: {}", e)))?;

    debug!(xkb_loaded = state.xkb_keymap.is_some(), "After first roundtrip");

    // Second roundtrip: picks up the wl_keyboard keymap event
    event_queue
        .roundtrip(&mut state)
        .map_err(|e| WaylandError::Connection(format!("second roundtrip failed: {}", e)))?;

    debug!(xkb_loaded = state.xkb_keymap.is_some(), "After second roundtrip");

    for info in &state.outputs {
        info!(
            name = %info.name,
            w = info.width,
            h = info.height,
            scale = info.scale,
            "Output"
        );
    }

    if state.outputs.is_empty() {
        warn!("No outputs discovered");
    }

    let mut config = initial_config.clone();
    let mut animation = Animation::new(&config);
    let mut shm_buf: Option<ShmBuffer> = None;
    let mut layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1> = None;
    let mut surface: Option<wl_surface::WlSurface> = None;
    let mut current_combos: Vec<ShortcutCombo> = Vec::new();
    let mut running = true;

    let mut processor = DefaultEventProcessor::new(ProcessorConfig {
        group_shortcuts: true,
        history_length: 3,
        dedup_window: Duration::from_millis(50),
    });

    if !state.outputs.is_empty() {
        match create_layer_surface(&wayland_globals, &config, &state, &qh) {
            Ok((s, ls, _scale)) => {
                surface = Some(s);
                layer_surface = Some(ls);
            }
            Err(e) => {
                warn!("Failed to create layer surface: {}", e);
            }
        }
    } else {
        warn!("No outputs available");
    }

    let raw_fd = conn.backend().poll_fd().as_raw_fd();

    info!("EchoInput running — press keys to see overlay");

    loop {
        {
            let mut pollfd = libc::pollfd {
                fd: raw_fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let _ = unsafe { libc::poll(&mut pollfd, 1, 5) };
            if pollfd.revents & libc::POLLIN != 0 {
                if let Some(guard) = event_queue.prepare_read() {
                    let _ = guard.read();
                }
            }
        }
        let _ = event_queue.dispatch_pending(&mut state);

        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                RendererCommand::Update(event) => match &event {
                    DisplayEvent::Shortcut(combo) => {
                        current_combos.clear();
                        current_combos.push(combo.clone());
                        animation.show(config.opacity);
                    }
                    DisplayEvent::History(combos) => {
                        current_combos = combos.clone();
                        animation.show(config.opacity);
                    }
                    DisplayEvent::Clear => {
                        current_combos.clear();
                        animation = Animation::new(&config);
                    }
                    DisplayEvent::UpdateConfig(new_config) => {
                        config = new_config.clone();
                        animation.update_config(&config);
                    }
                },
                RendererCommand::Stop => running = false,
            }
        }

        if !running {
            break;
        }

        {
            let mut events = keyboard_events.lock().unwrap();
            for event in events.drain(..) {
                let processed = processor.process(event);
                for pe in processed {
                    match pe {
                        input_core::events::ProcessedEvent::Shortcut(combo) => {
                            debug!(combo = %combo.display, "Shortcut detected");
                            current_combos.clear();
                            current_combos.push(combo);
                            animation.show(config.opacity);
                        }
                        input_core::events::ProcessedEvent::RawKey(kbd) => {
                            let combo = ShortcutCombo::new(
                                ModifierState::default(),
                                Some(kbd.key),
                            );
                            debug!(combo = %combo.display, "Key detected");
                            current_combos.clear();
                            current_combos.push(combo);
                            animation.show(config.opacity);
                        }
                        input_core::events::ProcessedEvent::ModifierChange(_) => {}
                    }
                }
            }
        }

        if !state.configured {
            animation.tick();
        } else if animation.is_visible() {
            let needs_redraw = animation.tick();
            if needs_redraw {
                let opacity = animation.current_opacity();
                if animation.is_visible() {
                    if let (Some(ref s), Some(ref ls)) = (&surface, &layer_surface) {
                        let (buf_w, buf_h, _keycap_count) =
                            compute_surface_size(&current_combos);

                        if buf_w == 0 || buf_h == 0 {
                            s.attach(None, 0, 0);
                            s.commit();
                        } else {
                            let needs_realloc = match &shm_buf {
                                Some(b) => buf_w > b.width || buf_h > b.height,
                                None => true,
                            };
                            if needs_realloc {
                                if let Some(old_buf) = shm_buf.take() {
                                    s.attach(None, 0, 0);
                                    s.commit();
                                    drop(old_buf);
                                }
                                match ShmBuffer::create(&wayland_globals, buf_w, buf_h, &qh) {
                                    Ok(new_buf) => {
                                        shm_buf = Some(new_buf);
                                        ls.set_size(buf_w as u32, buf_h as u32);
                                        s.commit();
                                    }
                                    Err(e) => {
                                        error!("Buffer allocation failed: {:?}", e);
                                    }
                                }
                            }

                            if let Some(ref buf) = shm_buf {
                                render_keycaps(buf, &current_combos, opacity);
                                s.attach(Some(&buf.buffer), 0, 0);
                                s.damage_buffer(0, 0, buf.width, buf.height);
                                s.commit();
                            }
                        }
                    }
                } else {
                    if let Some(ref s) = surface {
                        s.attach(None, 0, 0);
                        s.commit();
                    }
                }
            }
        }

        if animation.is_visible() {
            std::thread::sleep(Duration::from_millis(16));
        } else {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    if let Some(s) = surface {
        s.destroy();
    }
    if let Some(ls) = layer_surface {
        ls.destroy();
    }
    debug!("Event loop ended");
    Ok(())
}

fn create_layer_surface(
    globals: &WaylandGlobals,
    config: &OverlayConfig,
    state: &AppState,
    qh: &QueueHandle<AppState>,
) -> Result<
    (
        wl_surface::WlSurface,
        zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        i32,
    ),
    WaylandError,
> {
    let surface = globals.compositor.create_surface(qh, ());

    let layer = zwlr_layer_shell_v1::Layer::Overlay;

    let (output_proxy, scale) = match state.find_output_proxy(config.monitor.as_deref()) {
        Some(proxy) => {
            let scale = state
                .find_output(config.monitor.as_deref())
                .map(|(_, o)| o.scale)
                .unwrap_or(1);
            (Some(proxy.clone()), scale)
        }
        None => (None, 1),
    };

    let layer_surface = globals.layer_shell.get_layer_surface(
        &surface,
        output_proxy.as_ref(),
        layer,
        "echoinput-overlay".to_string(),
        qh,
        (),
    );

    let anchor_bits = position_to_anchor_bits(config.position);
    layer_surface.set_anchor(zwlr_layer_surface_v1::Anchor::from_bits_truncate(
        anchor_bits,
    ));
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_keyboard_interactivity(
        zwlr_layer_surface_v1::KeyboardInteractivity::None,
    );

    surface.commit();

    Ok((surface, layer_surface, scale))
}

fn position_to_anchor_bits(pos: OverlayPosition) -> u32 {
    match pos {
        OverlayPosition::TopLeft => 1 | 4,
        OverlayPosition::TopRight => 1 | 8,
        OverlayPosition::TopCenter => 1,
        OverlayPosition::BottomLeft => 2 | 4,
        OverlayPosition::BottomRight => 2 | 8,
        OverlayPosition::BottomCenter => 2,
        OverlayPosition::Center => 2,
    }
}

fn combo_to_key_parts(combo: &ShortcutCombo) -> Vec<String> {
    let mut parts = Vec::new();
    if combo.modifiers.ctrl {
        parts.push("Ctrl".to_string());
    }
    if combo.modifiers.alt {
        parts.push("Alt".to_string());
    }
    if combo.modifiers.shift {
        parts.push("Shift".to_string());
    }
    if combo.modifiers.super_key {
        parts.push("Super".to_string());
    }
    if let Some(key) = &combo.key {
        parts.push(key.label());
    }
    parts
}

fn compute_surface_size(combos: &[ShortcutCombo]) -> (i32, i32, usize) {
    let visible: Vec<&ShortcutCombo> = combos.iter().take(MAX_HISTORY_ROWS).collect();
    if visible.is_empty() {
        return (0, 0, 0);
    }

    let mut max_row_width = 0.0_f64;
    let mut total_keycaps = 0usize;

    for combo in &visible {
        let parts = combo_to_key_parts(combo);
        if parts.is_empty() {
            continue;
        }
        total_keycaps += parts.len();

        let sep_w = measure_text_width("+");
        let mut row_width = 0.0_f64;
        for (i, label) in parts.iter().enumerate() {
            row_width += measure_text_width(label) + KEYCAP_PADDING_X * 2.0;
            if i < parts.len() - 1 {
                row_width += sep_w + KEYCAP_GAP * 2.0;
            }
        }
        max_row_width = max_row_width.max(row_width);
    }

    if total_keycaps == 0 {
        return (0, 0, 0);
    }

    let keycap_h = FONT_SIZE + KEYCAP_PADDING_Y * 2.0;
    let content_h = visible.len() as f64 * keycap_h
        + (visible.len().saturating_sub(1)) as f64 * ROW_GAP;

    let surf_w = (max_row_width + SURFACE_MARGIN * 2.0).ceil() as i32;
    let surf_h = (content_h + SURFACE_MARGIN * 2.0).ceil() as i32;

    (surf_w.max(1), surf_h.max(1), total_keycaps)
}

fn measure_text_width(label: &str) -> f64 {
    let surface = match cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1) {
        Ok(s) => s,
        Err(_) => return label.len() as f64 * FONT_SIZE * 0.6,
    };
    let cr = match cairo::Context::new(&surface) {
        Ok(c) => c,
        Err(_) => return label.len() as f64 * FONT_SIZE * 0.6,
    };
    cr.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    cr.set_font_size(FONT_SIZE);
    if let Ok(extents) = cr.text_extents(label) {
        extents.x_bearing() + extents.width()
    } else {
        label.len() as f64 * FONT_SIZE * 0.6
    }
}

fn render_keycaps(shm: &ShmBuffer, combos: &[ShortcutCombo], opacity: f32) {
    let width = shm.width;
    let height = shm.height;

    let mut image_surface =
        match cairo::ImageSurface::create(cairo::Format::ARgb32, width, height) {
            Ok(s) => s,
            Err(e) => {
                error!("Cairo surface create failed: {:?}", e);
                return;
            }
        };

    let visible: Vec<&ShortcutCombo> = combos.iter().take(MAX_HISTORY_ROWS).collect();
    let keycap_h = FONT_SIZE + KEYCAP_PADDING_Y * 2.0;

    {
        let cr = match cairo::Context::new(&image_surface) {
            Ok(cr) => cr,
            Err(e) => {
                error!("Cairo context create failed: {:?}", e);
                return;
            }
        };

        let _ = cr.set_operator(cairo::Operator::Clear);
        let _ = cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();

        let _ = cr.set_operator(cairo::Operator::Over);

        cr.select_font_face(
            "sans-serif",
            cairo::FontSlant::Normal,
            cairo::FontWeight::Bold,
        );
        cr.set_font_size(FONT_SIZE);

        let mut y = SURFACE_MARGIN;

        for combo in &visible {
            let parts = combo_to_key_parts(combo);
            if parts.is_empty() {
                continue;
            }

            let mut x = SURFACE_MARGIN;

            for (i, label) in parts.iter().enumerate() {
                let kw = measure_text_width(label) + KEYCAP_PADDING_X * 2.0;

                let _ = cr.new_path();
                draw_rounded_rect(&cr, x, y, kw, keycap_h, CORNER_RADIUS);
                let _ = cr.set_source_rgba(
                    KEYCAP_BG.0,
                    KEYCAP_BG.1,
                    KEYCAP_BG.2,
                    opacity as f64,
                );
                let _ = cr.fill_preserve();

                let _ = cr.set_source_rgba(
                    KEYCAP_BORDER.0,
                    KEYCAP_BORDER.1,
                    KEYCAP_BORDER.2,
                    opacity as f64 * 0.5,
                );
                cr.set_line_width(1.0);
                let _ = cr.stroke();

                if let Ok(extents) = cr.text_extents(label) {
                    let visual_w = extents.x_bearing() + extents.width();
                    let text_x = x + (kw - visual_w) / 2.0 - extents.x_bearing();
                    let text_y =
                        y + (keycap_h - extents.height()) / 2.0 - extents.y_bearing();
                    let _ = cr.set_source_rgba(
                        TEXT_COLOR.0,
                        TEXT_COLOR.1,
                        TEXT_COLOR.2,
                        opacity as f64,
                    );
                    cr.move_to(text_x, text_y);
                    let _ = cr.show_text(label);
                }

                x += kw;

                if i < parts.len() - 1 {
                    if let Ok(sep_ext) = cr.text_extents("+") {
                        let sep_visual_w = sep_ext.x_bearing() + sep_ext.width();
                        let sep_x = x + KEYCAP_GAP - sep_visual_w / 2.0;
                        let sep_y = y + (keycap_h - sep_ext.height()) / 2.0
                            - sep_ext.y_bearing();
                        let _ = cr.set_source_rgba(
                            SEP_COLOR.0,
                            SEP_COLOR.1,
                            SEP_COLOR.2,
                            opacity as f64 * 0.7,
                        );
                        cr.move_to(sep_x, sep_y);
                        let _ = cr.show_text("+");
                    }
                    x += measure_text_width("+") + KEYCAP_GAP;
                }
            }

            y += keycap_h + ROW_GAP;
        }

        let _ = cr.show_page();
    }

    image_surface.flush();

    {
        let data = match image_surface.data() {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to read Cairo surface data: {:?}", e);
                return;
            }
        };
        shm.write_pixels(&data);
    }
}

fn draw_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

// ── Wayland dispatch implementations ────────────────────────────

delegate_noop!(AppState: ignore wl_compositor::WlCompositor);
delegate_noop!(AppState: ignore wl_shm::WlShm);
delegate_noop!(AppState: ignore wl_shm_pool::WlShmPool);
delegate_noop!(AppState: ignore wl_buffer::WlBuffer);
delegate_noop!(AppState: ignore wl_surface::WlSurface);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                match interface.as_str() {
                    "wl_output" => {
                        let proxy: wl_output::WlOutput =
                            _proxy.bind(name, version.min(4), qh, ());
                        let proxy_id = proxy.id().protocol_id();
                        state.outputs.push(OutputInfo {
                            name: String::new(),
                            scale: 1,
                            width: 0,
                            height: 0,
                            proxy_id,
                            global_id: name,
                        });
                        state.output_proxies.push(proxy);
                    }
                    "wl_seat" => {
                        let _seat: wl_seat::WlSeat =
                            _proxy.bind(name, version.min(7), qh, ());
                        debug!("Bound wl_seat");
                    }
                    _ => {}
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(idx) = state.outputs.iter().position(|o| o.global_id == name) {
                    state.outputs.remove(idx);
                    state.output_proxies.remove(idx);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for AppState {
    fn event(
        _state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                let _keyboard = seat.get_keyboard(qh, ());
                debug!("Got wl_keyboard from seat");
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for AppState {
    fn event(
        state: &mut Self,
        _keyboard: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap {
                format,
                fd,
                size,
            } => {
                if format == WEnum::Value(wl_keyboard::KeymapFormat::XkbV1) {
                    debug!(size, "Got keymap, loading via mmap");

                    if let Some(ref ctx) = state.xkb_context {
                        match unsafe {
                            xkbcommon::xkb::Keymap::new_from_fd(
                                ctx,
                                fd,
                                size as usize,
                                xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1,
                                xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
                            )
                        } {
                            Ok(Some(keymap)) => {
                                state.xkb_state = Some(xkbcommon::xkb::State::new(&keymap));
                                state.xkb_keymap = Some(keymap);
                                info!("XKB keymap loaded successfully");
                            }
                            Ok(None) => {
                                warn!("XKB keymap returned null");
                            }
                            Err(e) => {
                                warn!("Failed to load XKB keymap from fd: {}", e);
                            }
                        }
                    }
                }
            }
            wl_keyboard::Event::Enter { .. } => {
                debug!("Keyboard enter");
            }
            wl_keyboard::Event::Leave { .. } => {
                debug!("Keyboard leave");
            }
            wl_keyboard::Event::Key {
                serial: _,
                time: _,
                key,
                state: key_state,
            } => {
                let pressed = match key_state {
                    WEnum::Value(wl_keyboard::KeyState::Pressed) => true,
                    WEnum::Value(wl_keyboard::KeyState::Released) => false,
                    _ => return,
                };

                let keycode = xkbcommon::xkb::Keycode::new(key + 8);

                let virtual_key = if let Some(ref xkb_state) = state.xkb_state {
                    let mut xkb = xkb_state.clone();
                    xkb.update_key(
                        keycode,
                        if pressed {
                            xkbcommon::xkb::KeyDirection::Down
                        } else {
                            xkbcommon::xkb::KeyDirection::Up
                        },
                    );
                    let keysym = xkb.key_get_one_sym(keycode);
                    state.keysym_to_key(keysym)
                } else {
                    use crate::keymap_compat::scancode_to_key;
                    scancode_to_key(key + 8)
                };

                let key_event = InputEvent::Keyboard(KeyboardEvent {
                    key: virtual_key,
                    state: if pressed {
                        KeyState::Pressed
                    } else {
                        KeyState::Released
                    },
                    timestamp: std::time::SystemTime::now(),
                    native_code: key + 8,
                });

                if let Ok(mut events) = state.keyboard_events.lock() {
                    events.push(key_event);
                }
            }
            wl_keyboard::Event::Modifiers {
                serial: _,
                mods_depressed,
                mods_latched,
                mods_locked,
                group: _,
            } => {
                // Update XKB state with modifier changes
                if let Some(ref mut xkb_state) = state.xkb_state {
                    xkb_state.update_mask(
                        mods_depressed,
                        mods_latched,
                        mods_locked,
                        0,
                        0,
                        0,
                    );
                }
            }
            wl_keyboard::Event::RepeatInfo { rate: _, delay: _ } => {
                // We handle repeat ourselves via the animation system
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for AppState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let source_id = output.id().protocol_id();
        let idx = match state.find_output_index_by_proxy_id(source_id) {
            Some(i) => i,
            None => return,
        };

        let info = &mut state.outputs[idx];
        match event {
            wl_output::Event::Geometry { make, .. } => {
                if !make.is_empty() {
                    info.name = make.clone();
                } else {
                    info.name = format!("output-{}", info.proxy_id);
                }
            }
            wl_output::Event::Scale { factor } => {
                info.scale = factor;
            }
            wl_output::Event::Mode {
                width,
                height,
                flags,
                ..
            } => {
                if let WEnum::Value(f) = flags {
                    if f.bits() & 1 != 0 {
                        info.width = width;
                        info.height = height;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _: zwlr_layer_shell_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Closed => {
                warn!("Layer surface closed by compositor")
            }
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width: _,
                height: _,
            } => {
                proxy.ack_configure(serial);
                state.configured = true;
            }
            _ => {}
        }
    }
}
