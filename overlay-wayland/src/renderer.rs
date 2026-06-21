use crate::animation::Animation;
use crate::error::WaylandError;
use input_core::events::ShortcutCombo;
use input_core::ipc::MessageBus;
use input_core::overlay::{
    DisplayEvent, KeycapStyle, OverlayConfig, OverlayPosition, TextCaps, TextVariant,
};
use std::os::unix::io::{AsFd, AsRawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_region, wl_registry, wl_seat,
    wl_shm, wl_shm_pool, wl_surface,
};

use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};

const KEYCAP_GAP: f64 = 8.0;
const ROW_GAP: f64 = 8.0;
const MAX_HISTORY_ROWS: usize = 5;
const MAX_SEQUENCE_LENGTH: usize = 7;

// ── Hex color parsing ──────────────────────────────────────────

/// Parse a hex color string like "#RRGGBB" or "#RRGGBBAA" to (r, g, b, a) in 0.0-1.0.
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

/// Darken a color by a factor (0.0 = original, 1.0 = black).
fn darken(r: f64, g: f64, b: f64, factor: f64) -> (f64, f64, f64) {
    (
        r * (1.0 - factor),
        g * (1.0 - factor),
        b * (1.0 - factor),
    )
}

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

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.mmap_ptr as *mut libc::c_void, self.mmap_len);
        }
        self.buffer.destroy();
        self.pool.destroy();
    }
}

unsafe impl Send for ShmBuffer {}
unsafe impl Sync for ShmBuffer {}

impl ShmBuffer {
    fn create(
        globals: &WaylandGlobals,
        width: i32,
        height: i32,
        buffer_id: usize,
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
            buffer_id,
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
    shutdown: Option<Arc<AtomicBool>>,
}

impl WaylandRenderer {
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
impl input_core::traits::OverlayRenderer for WaylandRenderer {
    async fn start(&mut self, config: OverlayConfig) -> anyhow::Result<()> {
        let bus = self.bus.take().ok_or_else(|| {
            WaylandError::Connection("No MessageBus provided".into())
        })?;
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        self.cmd_tx = Some(cmd_tx);

        let shutdown = self.shutdown.take().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        let handle = tokio::task::spawn_blocking(move || {
            if let Err(e) = run_wayland_event_loop(bus, config, cmd_rx, shutdown) {
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
    configure_width: u32,
    configure_height: u32,
    configure_serial: Option<u32>,
    xkb_context: Option<xkbcommon::xkb::Context>,
    xkb_keymap: Option<xkbcommon::xkb::Keymap>,
    xkb_state: Option<xkbcommon::xkb::State>,
    _keyboard: Option<wl_keyboard::WlKeyboard>,
    buffer_attached: bool,
    buffer_a_ready: bool,
    buffer_b_ready: bool,
    surface_closed: bool,
}

impl AppState {
    fn new() -> Self {
        Self {
            outputs: Vec::new(),
            output_proxies: Vec::new(),
            configured: false,
            configure_width: 0,
            configure_height: 0,
            configure_serial: None,
            xkb_context: Some(xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS)),
            xkb_keymap: None,
            xkb_state: None,
            _keyboard: None,
            buffer_attached: false,
            buffer_a_ready: true,
            buffer_b_ready: true,
            surface_closed: false,
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
}

fn run_wayland_event_loop(
    _bus: MessageBus,
    initial_config: OverlayConfig,
    mut cmd_rx: mpsc::UnboundedReceiver<RendererCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env()
        .map_err(|e| WaylandError::Connection(e.to_string()))?;

    let (globals, mut event_queue) = registry_queue_init::<AppState>(&conn)
        .map_err(|e| WaylandError::Connection(format!("registry_queue_init: {}", e)))?;

    let qh = event_queue.handle();

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

    let all_globals = globals.contents().clone_list();
    for g in &all_globals {
        match g.interface.as_str() {
            "wl_output" => {
                let version = g.version.min(4);
                let proxy: wl_output::WlOutput = globals.registry().bind(g.name, version, &qh, ());
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
                let _seat: wl_seat::WlSeat = globals.registry().bind(g.name, g.version.min(7), &qh, ());
                debug!("Bound wl_seat global");
            }
            _ => {}
        }
    }

    // First roundtrip: discovers outputs, binds seat, creates wl_keyboard
    // Registry::Global events are dispatched here, binding wl_output and wl_seat
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
    let mut buf_a: Option<ShmBuffer> = None;
    let mut buf_b: Option<ShmBuffer> = None;
    let mut use_a = true;
    let mut layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1> = None;
    let mut surface: Option<wl_surface::WlSurface> = None;
    let mut current_combos: Vec<ShortcutCombo> = Vec::new();
    let mut running = true;

    if !state.outputs.is_empty() {
        match create_layer_surface(&wayland_globals, &config, &state, &qh) {
            Ok((s, ls, _scale)) => {
                eprintln!("DEBUG: Layer surface created successfully");
                surface = Some(s);
                layer_surface = Some(ls);
                if let Err(e) = conn.flush() {
                    warn!("Failed to flush after layer surface creation: {}", e);
                }
            }
            Err(e) => {
                warn!("Failed to create layer surface: {}", e);
            }
        }
    } else {
        warn!("No outputs available");
    }

    info!("EchoInput running — press keys to see overlay");

    let mut configure_received = state.configured;
    let mut configure_logged = false;
    let start_time = std::time::Instant::now();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            debug!("Shutdown flag set — Wayland event loop exiting");
            break;
        }

        if let Err(e) = event_queue.dispatch_pending(&mut state) {
            error!("Event queue dispatch_pending error: {:?}", e);
            // Wayland protocol error — connection is broken, exit cleanly
            warn!("Wayland connection lost, exiting event loop");
            break;
        }

        if let Some(guard) = event_queue.prepare_read() {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    error!("Wayland event read failed: {:?}", e);
                    break;
                }
            }
        }

        if let Err(e) = event_queue.dispatch_pending(&mut state) {
            error!("Event queue dispatch_pending error (second pass): {:?}", e);
            warn!("Wayland connection lost on second dispatch, exiting event loop");
            break;
        }

        if state.surface_closed {
            state.surface_closed = false;
            state.configured = false;
            state.configure_width = 0;
            state.configure_height = 0;
            state.configure_serial = None;
            state.buffer_attached = false;
            state.buffer_a_ready = true;
            state.buffer_b_ready = true;
            buf_a = None;
            buf_b = None;
            use_a = true;

            if let Some(s) = surface.take() {
                s.destroy();
            }
            if let Some(ls) = layer_surface.take() {
                ls.destroy();
            }

            match create_layer_surface(&wayland_globals, &config, &state, &qh) {
                Ok((s, ls, _scale)) => {
                    info!("Layer surface recreated successfully");
                    surface = Some(s);
                    layer_surface = Some(ls);
                    if let Err(e) = conn.flush() {
                        warn!("Failed to flush after surface recreation: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to recreate layer surface: {}", e);
                }
            }
        }

        if !state.configured && start_time.elapsed() > std::time::Duration::from_secs(5) && !configure_received {
            configure_received = true;
            warn!("No configure received after 5s — forcing configured state");
            state.configured = true;
            if state.configure_width == 0 {
                state.configure_width = state.outputs.first().map(|o| o.width as u32).unwrap_or(1920);
            }
            if state.configure_height == 0 {
                state.configure_height = state.outputs.first().map(|o| o.height as u32).unwrap_or(1080);
            }
        }

        if !configure_logged && state.configured {
            configure_logged = true;
            configure_received = true;
            info!("Compositor configure received — overlay ready (w={} h={})",
                state.configure_width, state.configure_height);
            if !current_combos.is_empty() {
                animation.show(config.opacity);
            }
        }

        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                RendererCommand::Update(event) => match &event {
                    DisplayEvent::Shortcut(combo) => {
                        debug!("Renderer received shortcut: {}", combo.display);
                        // Merge consecutive plain keystrokes into one row
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
                                    // Cap sequence length — oldest keys vanish
                                    while keys.len() > MAX_SEQUENCE_LENGTH {
                                        keys.remove(0);
                                    }
                                    let merged = ShortcutCombo::sequence(keys);
                                    current_combos[0] = merged;
                                    animation.refresh();
                                    continue;
                                }
                            }
                        }
                        // New modifier combo — clear old rows, show this one
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

        if !state.configured {
            animation.tick();
        } else {
            let needs_redraw = animation.tick();
            let opacity = animation.current_opacity();

            // When animation finishes fading to Idle, clear the combo list
            // so the next keypress starts fresh — no stale rows reappear.
            if needs_redraw && animation.state() == crate::animation::AnimationState::Idle {
                current_combos.clear();
            }

            let has_content = !current_combos.is_empty() && animation.is_visible();

            // After receiving a new Configure event, we MUST re-attach a buffer
            // and commit to complete the Wayland layer-shell handshake.
            let needs_configure_flush = state.configure_serial.is_some();

            if has_content {
                if let (Some(ref s), Some(ref _ls)) = (&surface, &layer_surface) {
                    let (content_w, content_h, _keycap_count) =
                        compute_surface_size(&current_combos, &config);

                    if content_w > 0 && content_h > 0 {
                        let render_w = if state.configure_width > 0 {
                            state.configure_width as i32
                        } else {
                            state.outputs.first().map(|o| o.width).unwrap_or(1920)
                        };
                        let render_h = if state.configure_height > 0 {
                            state.configure_height as i32
                        } else {
                            state.outputs.first().map(|o| o.height).unwrap_or(1080)
                        };

                        let (active_buf, active_id, ready_flag) = if use_a {
                            (&mut buf_a, 0usize, &mut state.buffer_a_ready)
                        } else {
                            (&mut buf_b, 1usize, &mut state.buffer_b_ready)
                        };

                        let needs_realloc = match active_buf {
                            Some(b) => render_w > b.width || render_h > b.height,
                            None => true,
                        };
                        if needs_realloc && *ready_flag {
                            *active_buf = None;
                            match ShmBuffer::create(&wayland_globals, render_w, render_h, active_id, &qh) {
                                Ok(new_buf) => {
                                    *active_buf = Some(new_buf);
                                }
                                Err(e) => {
                                    error!("Buffer allocation failed: {:?}", e);
                                }
                            }
                        }

                        if let Some(ref buf) = *active_buf {
                            if *ready_flag {
                                let offset_x = match config.position {
                                    OverlayPosition::TopLeft | OverlayPosition::BottomLeft => config.margin_x as f64,
                                    OverlayPosition::TopRight | OverlayPosition::BottomRight => {
                                        (buf.width as f64 - content_w as f64 - config.margin_x as f64).max(0.0)
                                    }
                                    _ => ((buf.width - content_w) / 2).max(0) as f64,
                                };
                                render_keycaps(
                                    buf,
                                    &current_combos,
                                    &config,
                                    opacity,
                                    animation.slide_offset(),
                                    animation.scale(),
                                    offset_x,
                                    config.position,
                                );
                                s.attach(Some(&buf.buffer), 0, 0);
                                s.damage_buffer(0, 0, buf.width, buf.height);
                                s.commit();
                                if let Err(e) = conn.flush() {
                                    error!("Wayland flush failed after render: {:?}", e);
                                    warn!("Wayland connection lost, exiting event loop");
                                    break;
                                }
                                state.buffer_attached = true;
                                state.configure_serial = None;
                                *ready_flag = false;
                                use_a = !use_a;
                            }
                        }
                    }
                }
            } else if state.buffer_attached {
                // No content to show (or animation faded to idle).
                // Render a transparent frame to clear stale keycap pixels
                // from the buffer.  We MUST keep the buffer attached —
                // detaching it causes Hyprland to unconfigure the surface
                // and crash on the next attach.
                if let (Some(ref s), Some(ref _ls)) = (&surface, &layer_surface) {
                    let (active_buf, active_id, ready_flag) = if use_a {
                        (&mut buf_a, 0usize, &mut state.buffer_a_ready)
                    } else {
                        (&mut buf_b, 1usize, &mut state.buffer_b_ready)
                    };

                    let render_w = if state.configure_width > 0 {
                        state.configure_width as i32
                    } else {
                        state.outputs.first().map(|o| o.width).unwrap_or(1920)
                    };
                    let render_h = if state.configure_height > 0 {
                        state.configure_height as i32
                    } else {
                        state.outputs.first().map(|o| o.height).unwrap_or(1080)
                    };

                    let needs_realloc = match active_buf {
                        Some(b) => render_w > b.width || render_h > b.height,
                        None => true,
                    };
                    if needs_realloc && *ready_flag {
                        *active_buf = None;
                        match ShmBuffer::create(&wayland_globals, render_w, render_h, active_id, &qh) {
                            Ok(new_buf) => {
                                *active_buf = Some(new_buf);
                            }
                            Err(e) => {
                                error!("Buffer allocation failed for clear frame: {:?}", e);
                            }
                        }
                    }

                    if let Some(ref buf) = *active_buf {
                        if *ready_flag {
                            render_clear_frame(buf);
                            s.attach(Some(&buf.buffer), 0, 0);
                            s.damage_buffer(0, 0, buf.width, buf.height);
                            s.commit();
                            if let Err(e) = conn.flush() {
                                error!("Wayland flush failed after clear: {:?}", e);
                                break;
                            }
                            state.configure_serial = None;
                            *ready_flag = false;
                            use_a = !use_a;
                        }
                    }
                }
            } else if needs_configure_flush {
                state.configure_serial = None;
            }
        }

        if animation.is_visible() {
            std::thread::sleep(Duration::from_millis(8));
        } else {
            std::thread::sleep(Duration::from_millis(16));
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

    let anchor = position_to_anchor(config);
    let bits = anchor.bits();
    eprintln!("DEBUG: anchor bits={} binary={:04b}", bits, bits);

    layer_surface.set_anchor(anchor);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_keyboard_interactivity(
        zwlr_layer_surface_v1::KeyboardInteractivity::None,
    );

    let empty_region = globals.compositor.create_region(qh, ());
    surface.set_input_region(Some(&empty_region));
    empty_region.destroy();

    let output_width = state.outputs.first().map(|o| o.width).filter(|&w| w > 0).unwrap_or(1920);
    let output_height = state.outputs.first().map(|o| o.height).filter(|&h| h > 0).unwrap_or(1080);
    layer_surface.set_size(output_width as u32, output_height as u32);

    surface.commit();

    Ok((surface, layer_surface, scale))
}

fn position_to_anchor(config: &OverlayConfig) -> zwlr_layer_surface_v1::Anchor {
    use zwlr_layer_surface_v1::Anchor;
    match config.position {
        OverlayPosition::TopLeft => Anchor::Top | Anchor::Left,
        OverlayPosition::TopRight => Anchor::Top | Anchor::Right,
        OverlayPosition::TopCenter => Anchor::Top | Anchor::Left | Anchor::Right,
        OverlayPosition::BottomLeft => Anchor::Bottom | Anchor::Left,
        OverlayPosition::BottomRight => Anchor::Bottom | Anchor::Right,
        OverlayPosition::BottomCenter => Anchor::Bottom | Anchor::Left | Anchor::Right,
        OverlayPosition::Center => Anchor::Top | Anchor::Left | Anchor::Right,
    }
}

fn combo_to_key_parts(combo: &ShortcutCombo, variant: &TextVariant) -> Vec<String> {
    let mut parts = Vec::new();

    // For key sequences, return all keys from the sequence
    if combo.is_sequence() {
        return combo
            .key_sequence
            .iter()
            .map(|k| apply_text_variant(&k.label(), variant))
            .collect();
    }

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
        parts.push(apply_text_variant(&key.label(), variant));
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

/// Compute keycap dimensions from config.
fn keycap_dimensions(config: &OverlayConfig) -> (f64, f64, f64, f64) {
    let font_size = config.text.size.unwrap_or(config.scale.font_size()) as f64;
    let padding_x = config.scale.padding() as f64 * 1.5;
    let padding_y = config.scale.padding() as f64;
    let corner_radius = font_size * config.border.radius as f64;
    (font_size, padding_x, padding_y, corner_radius)
}

fn compute_surface_size(combos: &[ShortcutCombo], config: &OverlayConfig) -> (i32, i32, usize) {
    let (font_size, padding_x, padding_y, _corner_radius) = keycap_dimensions(config);
    let visible: Vec<&ShortcutCombo> = combos.iter().take(MAX_HISTORY_ROWS).collect();
    if visible.is_empty() {
        return (0, 0, 0);
    }

    let mut max_row_width = 0.0_f64;
    let mut total_keycaps = 0usize;

    for combo in &visible {
        let parts = combo_to_key_parts(combo, &config.text.variant);
        if parts.is_empty() {
            continue;
        }
        total_keycaps += parts.len();

        let sep_w = measure_text_width("+", font_size);
        let is_seq = combo.is_sequence();
        let mut row_width = 0.0_f64;
        for (i, label) in parts.iter().enumerate() {
            row_width += measure_text_width(label, font_size) + padding_x * 2.0;
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

    if total_keycaps == 0 {
        return (0, 0, 0);
    }

    let keycap_h = font_size + padding_y * 2.0;
    let content_h = visible.len() as f64 * keycap_h
        + (visible.len().saturating_sub(1)) as f64 * ROW_GAP;

    let margin_x = config.margin_x as f64;
    let margin_y = config.margin_y as f64;
    let surf_w = (max_row_width + margin_x * 2.0).ceil() as i32;
    let surf_h = (content_h + margin_y * 2.0).ceil() as i32;

    (surf_w.max(1), surf_h.max(1), total_keycaps)
}

fn measure_text_width(label: &str, font_size: f64) -> f64 {
    let surface = match cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1) {
        Ok(s) => s,
        Err(_) => return label.len() as f64 * font_size * 0.6,
    };
    let cr = match cairo::Context::new(&surface) {
        Ok(c) => c,
        Err(_) => return label.len() as f64 * font_size * 0.6,
    };
    cr.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    cr.set_font_size(font_size);
    if let Ok(extents) = cr.text_extents(label) {
        extents.x_bearing() + extents.width()
    } else {
        label.len() as f64 * font_size * 0.6
    }
}

fn render_clear_frame(shm: &ShmBuffer) {
    let mut image_surface =
        match cairo::ImageSurface::create(cairo::Format::ARgb32, shm.width, shm.height) {
            Ok(s) => s,
            Err(e) => {
                error!("Cairo surface create failed for clear frame: {:?}", e);
                return;
            }
        };
    {
        let cr = match cairo::Context::new(&image_surface) {
            Ok(cr) => cr,
            Err(e) => {
                error!("Cairo context create failed for clear frame: {:?}", e);
                return;
            }
        };
        let _ = cr.set_operator(cairo::Operator::Clear);
        let _ = cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();
        let _ = cr.show_page();
    }
    image_surface.flush();
    let data = match image_surface.data() {
        Ok(d) => d,
        Err(e) => {
            error!("Failed to read Cairo surface data for clear frame: {:?}", e);
            return;
        }
    };
    shm.write_pixels(&data);
}

fn render_keycaps(shm: &ShmBuffer, combos: &[ShortcutCombo], config: &OverlayConfig, opacity: f32, slide_offset: f32, scale: f32, offset_x: f64, position: OverlayPosition) {
    let width = shm.width;
    let height = shm.height;

    let (font_size, padding_x, padding_y, corner_radius) = keycap_dimensions(config);

    // Parse config colors
    let keycap_primary = parse_hex_color(&config.colors.keycap_primary);
    let keycap_secondary = parse_hex_color(&config.colors.keycap_secondary);
    let modifier_primary = parse_hex_color(&config.colors.modifier_primary);
    let modifier_secondary = parse_hex_color(&config.colors.modifier_secondary);
    let text_color = parse_hex_color(&config.text.color);
    let modifier_text_color = parse_hex_color(&config.text.modifier_color);
    let border_color = parse_hex_color(&config.border.color);
    let modifier_border_color = parse_hex_color(&config.border.modifier_color);
    let background_color = parse_hex_color(&config.background.color);

    let mut image_surface =
        match cairo::ImageSurface::create(cairo::Format::ARgb32, width, height) {
            Ok(s) => s,
            Err(e) => {
                error!("Cairo surface create failed: {:?}", e);
                return;
            }
        };

    let visible: Vec<&ShortcutCombo> = combos.iter().take(MAX_HISTORY_ROWS).collect();

    {
        let cr = match cairo::Context::new(&image_surface) {
            Ok(cr) => cr,
            Err(e) => {
                error!("Cairo context create failed: {:?}", e);
                return;
            }
        };

        // Clear to transparent
        let _ = cr.set_operator(cairo::Operator::Clear);
        let _ = cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();

        let _ = cr.set_operator(cairo::Operator::Over);

        // Apply scale transform from center of content
        let center_x = width as f64 / 2.0;
        let center_y = height as f64 / 2.0;
        cr.translate(center_x, center_y);
        cr.scale(scale as f64, scale as f64);
        cr.translate(-center_x, -center_y);

        // Apply slide offset
        let slide_y = slide_offset as f64;

        cr.select_font_face(
            "sans-serif",
            cairo::FontSlant::Normal,
            cairo::FontWeight::Bold,
        );
        cr.set_font_size(font_size);

        // Compute content height for vertical positioning
        let keycap_h = font_size + padding_y * 2.0;
        let content_h = visible.len() as f64 * keycap_h
            + (visible.len().saturating_sub(1)) as f64 * ROW_GAP;

        let margin_y = config.margin_y as f64;

        // Compute initial Y based on position
        let mut y = match position {
            OverlayPosition::BottomLeft | OverlayPosition::BottomRight | OverlayPosition::BottomCenter => {
                (height as f64 - content_h - margin_y + slide_y).max(0.0)
            }
            OverlayPosition::Center => {
                ((height as f64 - content_h) / 2.0 + slide_y).max(0.0)
            }
            OverlayPosition::TopLeft | OverlayPosition::TopRight | OverlayPosition::TopCenter => {
                margin_y + slide_y
            }
        };

        for (row_idx, combo) in visible.iter().enumerate() {
            let parts = combo_to_key_parts(combo, &config.text.variant);
            if parts.is_empty() {
                continue;
            }
            let is_seq = combo.is_sequence();

            let mut x = offset_x;
            let row_opacity = opacity * (1.0 - row_idx as f32 * 0.15).max(0.3);

            for (i, label) in parts.iter().enumerate() {
                let kw = measure_text_width(label, font_size) + padding_x * 2.0;
                let is_modifier = is_modifier_label(label);

                // Determine colors based on whether this is a modifier
                let (bg_r, bg_g, bg_b, bg2_r, bg2_g, bg2_b, brd_r, brd_g, brd_b, txt_r, txt_g, txt_b) = if is_modifier && config.colors.highlight_modifiers {
                    (
                        modifier_primary.0, modifier_primary.1, modifier_primary.2,
                        modifier_secondary.0, modifier_secondary.1, modifier_secondary.2,
                        modifier_border_color.0, modifier_border_color.1, modifier_border_color.2,
                        modifier_text_color.0, modifier_text_color.1, modifier_text_color.2,
                    )
                } else {
                    (
                        keycap_primary.0, keycap_primary.1, keycap_primary.2,
                        keycap_secondary.0, keycap_secondary.1, keycap_secondary.2,
                        border_color.0, border_color.1, border_color.2,
                        text_color.0, text_color.1, text_color.2,
                    )
                };

                // Draw shadow (unless minimal style)
                if config.keycap_style != KeycapStyle::Minimal {
                    let _ = cr.new_path();
                    draw_rounded_rect(&cr, x + 2.0, y + 3.0, kw, keycap_h, corner_radius);
                    let _ = cr.set_source_rgba(0.0, 0.0, 0.0, 0.4 * row_opacity as f64);
                    let _ = cr.fill();
                }

                // Draw keycap background
                let _ = cr.new_path();
                draw_rounded_rect(&cr, x, y, kw, keycap_h, corner_radius);

                match config.keycap_style {
                    KeycapStyle::Minimal => {
                        // Flat solid color, no gradient
                        let _ = cr.set_source_rgba(
                            bg_r, bg_g, bg_b,
                            row_opacity as f64 * 0.9,
                        );
                        let _ = cr.fill();
                    }
                    KeycapStyle::LowProfile => {
                        // Darker, flatter
                        let (dark_r, dark_g, dark_b) = darken(bg_r, bg_g, bg_b, 0.2);
                        let _ = cr.set_source_rgba(
                            dark_r, dark_g, dark_b,
                            row_opacity as f64 * 0.85,
                        );
                        let _ = cr.fill();
                    }
                    _ => {
                        // Gradient background (Laptop, PBT)
                        if config.colors.use_gradient {
                            let pattern = cairo::LinearGradient::new(0.0, y, 0.0, y + keycap_h);
                            pattern.add_color_stop_rgba(0.0, bg2_r, bg2_g, bg2_b, row_opacity as f64 * 0.95);
                            pattern.add_color_stop_rgba(1.0, bg_r, bg_g, bg_b, row_opacity as f64 * 0.9);
                            let _ = cr.set_source(&pattern);
                        } else {
                            let _ = cr.set_source_rgba(
                                bg_r, bg_g, bg_b,
                                row_opacity as f64 * 0.9,
                            );
                        }
                        let _ = cr.fill_preserve();
                    }
                }

                // Draw border (unless minimal style)
                if config.keycap_style != KeycapStyle::Minimal && config.border.enabled {
                    let _ = cr.set_source_rgba(
                        brd_r, brd_g, brd_b,
                        row_opacity as f64 * 0.6,
                    );
                    cr.set_line_width(config.border.width as f64);
                    let _ = cr.stroke();
                }

                // Draw top highlight (subtle shine) - not for minimal/lowprofile
                if matches!(config.keycap_style, KeycapStyle::Laptop | KeycapStyle::PBT) {
                    let _ = cr.new_path();
                    draw_rounded_rect(&cr, x + 1.0, y + 1.0, kw - 2.0, keycap_h * 0.4, corner_radius - 1.0);
                    let highlight = cairo::LinearGradient::new(0.0, y, 0.0, y + keycap_h * 0.4);
                    highlight.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.08 * row_opacity as f64);
                    highlight.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
                    let _ = cr.set_source(&highlight);
                    let _ = cr.fill();
                }

                // Draw background fill (if enabled)
                if config.background.enabled {
                    let _ = cr.new_path();
                    draw_rounded_rect(&cr, x, y, kw, keycap_h, corner_radius);
                    let _ = cr.set_source_rgba(
                        background_color.0, background_color.1, background_color.2,
                        background_color.3 * row_opacity as f64,
                    );
                    let _ = cr.fill();
                }

                // Apply text capitalization
                let display_label = match config.text.caps {
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
                };

                // Draw text label
                if let Ok(extents) = cr.text_extents(&display_label) {
                    let visual_w = extents.x_bearing() + extents.width();
                    let text_x = x + (kw - visual_w) / 2.0 - extents.x_bearing();
                    let text_y =
                        y + (keycap_h - extents.height()) / 2.0 - extents.y_bearing();

                    // Text shadow (not for minimal style)
                    if config.keycap_style != KeycapStyle::Minimal {
                        let _ = cr.set_source_rgba(0.0, 0.0, 0.0, 0.5 * row_opacity as f64);
                        cr.move_to(text_x + 1.0, text_y + 1.0);
                        let _ = cr.show_text(&display_label);
                    }

                    // Main text
                    let _ = cr.set_source_rgba(txt_r, txt_g, txt_b, row_opacity as f64);
                    cr.move_to(text_x, text_y);
                    let _ = cr.show_text(&display_label);
                }

                x += kw;

                // Draw separator between keys
                if i < parts.len() - 1 {
                    if is_seq {
                        x += KEYCAP_GAP;
                    } else {
                        if let Ok(sep_ext) = cr.text_extents("+") {
                            let sep_visual_w = sep_ext.x_bearing() + sep_ext.width();
                            let sep_x = x + KEYCAP_GAP - sep_visual_w / 2.0;
                            let sep_y = y + (keycap_h - sep_ext.height()) / 2.0
                                - sep_ext.y_bearing();
                            let _ = cr.set_source_rgba(
                                0.55, 0.55, 0.6,
                                row_opacity as f64 * 0.8,
                            );
                            cr.move_to(sep_x, sep_y);
                            let _ = cr.show_text("+");
                        }
                        x += measure_text_width("+", font_size) + KEYCAP_GAP;
                    }
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

fn is_modifier_label(label: &str) -> bool {
    matches!(
        label,
        "Ctrl" | "Alt" | "Shift" | "Super" | "Meta"
    )
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
delegate_noop!(AppState: ignore wl_surface::WlSurface);
delegate_noop!(AppState: ignore wl_region::WlRegion);

impl Dispatch<wl_buffer::WlBuffer, usize> for AppState {
    fn event(
        state: &mut Self,
        _buffer: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        id: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_buffer::Event::Release => {
                if *id == 0 {
                    state.buffer_a_ready = true;
                } else {
                    state.buffer_b_ready = true;
                }
                trace!(buffer_id = *id, "wl_buffer released");
            }
            _ => {}
        }
    }
}

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
        state: &mut Self,
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
                state._keyboard = Some(seat.get_keyboard(qh, ()));
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
                // Key events from wl_keyboard are not used — input comes from evdev.
                // This handler exists only for protocol compliance.
                let _ = (key, key_state);
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
                warn!("Layer surface closed by compositor — will recreate");
                state.surface_closed = true;
            }
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                proxy.ack_configure(serial);
                state.configured = true;
                state.configure_width = width;
                state.configure_height = height;
                state.configure_serial = Some(serial);
                debug!(serial, width, height, "Layer surface configured");
            }
            _ => {}
        }
    }
}
