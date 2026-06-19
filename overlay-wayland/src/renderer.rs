use crate::animation::Animation;
use crate::error::WaylandError;
use input_core::events::ShortcutCombo;
use input_core::ipc::{MessageBus, OverlayCommand};
use input_core::overlay::{DisplayEvent, OverlayConfig, OverlayPosition};
use input_core::traits::OverlayRenderer;
use std::os::unix::io::{AsFd, AsRawFd};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_output, wl_registry, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};

const BASE_WIDTH: i32 = 400;
const BASE_HEIGHT: i32 = 120;
const CORNER_RADIUS: f64 = 12.0;
const PADDING: f64 = 16.0;
const FONT_SIZE: f64 = 20.0;
const BG_COLOR: (f64, f64, f64) = (0.13, 0.13, 0.13);
const TEXT_COLOR: (f64, f64, f64) = (0.95, 0.95, 0.95);

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
        let buffer = pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, qh, ());

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
impl OverlayRenderer for WaylandRenderer {
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
        info!("Wayland renderer started");
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(RendererCommand::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        info!("Wayland renderer stopped");
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
    needs_surface_commit: bool,
}

impl AppState {
    fn new() -> Self {
        Self {
            outputs: Vec::new(),
            output_proxies: Vec::new(),
            needs_surface_commit: false,
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
    bus: MessageBus,
    initial_config: OverlayConfig,
    mut cmd_rx: mpsc::UnboundedReceiver<RendererCommand>,
) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env()
        .map_err(|e| WaylandError::Connection(e.to_string()))?;
    info!("Wayland connection established");

    let (globals, mut event_queue) = registry_queue_init::<AppState>(&conn)
        .map_err(|e| WaylandError::Connection(format!("registry_queue_init: {}", e)))?;

    let qh = event_queue.handle();

    // Log all registry globals at startup
    let all_globals = globals.contents().clone_list();
    info!("Registry globals ({}):", all_globals.len());
    for g in &all_globals {
        info!(name = %g.interface, version = g.version, "  global");
    }

    // Bind singleton globals
    info!("Binding singleton globals...");
    let compositor: wl_compositor::WlCompositor = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| WaylandError::MissingProtocol(format!("wl_compositor: {}", e)))?;
    info!("  wl_compositor bound");

    let shm: wl_shm::WlShm = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| WaylandError::MissingProtocol(format!("wl_shm: {}", e)))?;
    info!("  wl_shm bound");

    let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| WaylandError::MissingProtocol(format!("zwlr_layer_shell_v1: {}", e)))?;
    info!("  zwlr_layer_shell_v1 bound");

    let wayland_globals = WaylandGlobals { compositor, shm, layer_shell };

    // Bind wl_output globals manually via the registry.
    // registry_queue_init consumed the initial Global events internally,
    // so the Dispatch impl was NOT called for them. We iterate GlobalListContents
    // and bind each wl_output by its global ID.
    let mut state = AppState::new();
    let registry = globals.registry();
    let mut output_count = 0;
    for g in &all_globals {
        if g.interface == "wl_output" {
            output_count += 1;
            let version = g.version.min(4);
            info!(global_id = g.name, version, "Binding wl_output");
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
            info!(global_id = g.name, proxy_id, "wl_output bound");
        }
    }
    info!(count = output_count, "wl_output globals found");

    // Roundtrip to receive wl_output geometry/mode/scale events
    info!("Post-bind roundtrip for output metadata...");
    event_queue.roundtrip(&mut state).map_err(|e| {
        WaylandError::Connection(format!("output roundtrip failed: {}", e))
    })?;

    info!("Output discovered:");
    for info in &state.outputs {
        info!(name = %info.name, width = info.width, height = info.height, scale = info.scale, "");
    }
    info!(count = state.outputs.len(), "Outputs discovered");

    if state.outputs.is_empty() {
        warn!("No outputs discovered - overlay will not be visible");
    }

    let mut shortcut_rx = bus.subscribe_shortcut();
    let mut command_rx = bus.subscribe_command();
    let mut settings_rx = bus.subscribe_settings();
    let mut config = initial_config.clone();
    let mut animation = Animation::new(&config);
    let mut shm_buf: Option<ShmBuffer> = None;
    let mut layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1> = None;
    let mut surface: Option<wl_surface::WlSurface> = None;
    let mut current_combos: Vec<ShortcutCombo> = Vec::new();
    let mut running = true;

    // Create initial surface
    if !state.outputs.is_empty() {
        info!("Creating initial layer surface...");
        match create_layer_surface(&wayland_globals, &config, &state, &qh) {
            Ok((s, ls, scale)) => {
                let w = BASE_WIDTH * scale;
                let h = BASE_HEIGHT * scale;
                surface = Some(s);
                layer_surface = Some(ls);
                shm_buf = Some(ShmBuffer::create(&wayland_globals, w, h, &qh)?);
                info!("Layer surface created");
                info!(width = w, height = h, scale, "Layer surface configured");
            }
            Err(e) => {
                warn!("Failed to create layer surface: {}", e);
                warn!("Continuing without overlay - will retry on Restart command");
            }
        }
    } else {
        warn!("No outputs available, cannot create layer surface");
    }

    info!("Renderer ready");

    loop {
        // Read pending wayland events (non-blocking)
        if let Some(guard) = event_queue.prepare_read() {
            let _ = guard.read();
        }
        let _ = event_queue.dispatch_pending(&mut state);

        if state.needs_surface_commit {
            if let Some(ref s) = surface {
                s.commit();
            }
            state.needs_surface_commit = false;
        }

        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                RendererCommand::Update(event) => {
                    match &event {
                        DisplayEvent::Shortcut(combo) => {
                            info!("Renderer received event");
                            info!("Shortcut received: {}", combo.display);
                            current_combos.clear();
                            current_combos.push(combo.clone());
                            animation.show(config.opacity);
                            info!("Overlay updated");
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
                    }
                }
                RendererCommand::Stop => running = false,
            }
        }

        if !running {
            break;
        }

        while let Ok(event) = shortcut_rx.try_recv() {
            info!("Renderer received event");
            info!("Shortcut received: {}", event.combo.display);
            current_combos.clear();
            current_combos.push(event.combo);
            animation.show(config.opacity);
            info!("Overlay updated");
        }

        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                OverlayCommand::Start => running = true,
                OverlayCommand::Stop => running = false,
                OverlayCommand::Restart => {
                    current_combos.clear();
                    animation = Animation::new(&config);
                    if let Some(s) = surface.take() { s.destroy(); }
                    if let Some(ls) = layer_surface.take() { ls.destroy(); }
                    shm_buf = None;
                    if let Ok((s, ls, scale)) = create_layer_surface(&wayland_globals, &config, &state, &qh) {
                        let w = BASE_WIDTH * scale;
                        let h = BASE_HEIGHT * scale;
                        surface = Some(s);
                        layer_surface = Some(ls);
                        shm_buf = Some(ShmBuffer::create(&wayland_globals, w, h, &qh)?);
                    }
                }
                OverlayCommand::Clear => {
                    current_combos.clear();
                    animation = Animation::new(&config);
                }
                OverlayCommand::UpdateConfig(new_config) => {
                    config = new_config;
                    animation.update_config(&config);
                }
            }
        }

        while let Ok(update) = settings_rx.try_recv() {
            update.apply(&mut config);
            animation.update_config(&config);
        }

        if animation.is_visible() {
            let needs_redraw = animation.tick();
            if needs_redraw {
                let opacity = animation.current_opacity();
                if animation.is_visible() {
                    if let (Some(ref buf), Some(ref s)) = (&shm_buf, &surface) {
                        render_frame(buf, &current_combos, opacity);
                        s.attach(Some(&buf.buffer), 0, 0);
                        s.damage_buffer(0, 0, buf.width, buf.height);
                        s.commit();
                        info!("Surface committed");
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

    if let Some(s) = surface { s.destroy(); }
    if let Some(ls) = layer_surface { ls.destroy(); }
    debug!("Wayland event loop ended");
    Ok(())
}

fn create_layer_surface(
    globals: &WaylandGlobals,
    config: &OverlayConfig,
    state: &AppState,
    qh: &QueueHandle<AppState>,
) -> Result<(wl_surface::WlSurface, zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, i32), WaylandError> {
    info!("Creating layer surface");

    let surface = globals.compositor.create_surface(qh, ());
    info!("  wl_surface created");

    let layer = zwlr_layer_shell_v1::Layer::Overlay;

    // Select output: use config.monitor if set, otherwise first available output
    let (output_proxy, scale) = match state.find_output_proxy(config.monitor.as_deref()) {
        Some(proxy) => {
            let scale = state
                .find_output(config.monitor.as_deref())
                .map(|(_, o)| o.scale)
                .unwrap_or(1);
            info!(scale, "  Using output for layer surface");
            (Some(proxy.clone()), scale)
        }
        None => {
            if let Some(name) = &config.monitor {
                warn!(monitor = %name, "  Monitor not found, using default");
            }
            info!("  No specific output, using compositor default");
            (None, 1)
        }
    };

    info!("  Calling get_layer_surface...");
    let layer_surface = globals.layer_shell.get_layer_surface(
        &surface,
        output_proxy.as_ref(),
        layer,
        "echoinput-overlay".to_string(),
        qh,
        (),
    );
    info!("  Layer surface obtained");

    let anchor_bits = position_to_anchor_bits(config.position);
    info!(anchor_bits, "  set_anchor called");
    layer_surface.set_anchor(zwlr_layer_surface_v1::Anchor::from_bits_truncate(anchor_bits));
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_keyboard_interactivity(
        zwlr_layer_surface_v1::KeyboardInteractivity::None,
    );

    let w = (BASE_WIDTH * scale) as u32;
    let h = (BASE_HEIGHT * scale) as u32;
    layer_surface.set_size(w, h);
    info!(width = w, height = h, scale, "  set_size done");
    surface.commit();
    info!("  Surface committed");

    Ok((surface, layer_surface, scale))
}

fn position_to_anchor_bits(pos: OverlayPosition) -> u32 {
    let mut bits: u32 = 0;
    match pos {
        OverlayPosition::TopLeft => { bits |= 1 | 4; }
        OverlayPosition::TopRight => { bits |= 1 | 8; }
        OverlayPosition::TopCenter => { bits |= 1; }
        OverlayPosition::BottomLeft => { bits |= 2 | 4; }
        OverlayPosition::BottomRight => { bits |= 2 | 8; }
        OverlayPosition::BottomCenter => { bits |= 2; }
        OverlayPosition::Center => {}
    }
    bits
}

fn render_frame(shm: &ShmBuffer, combos: &[ShortcutCombo], opacity: f32) {
    let width = shm.width;
    let height = shm.height;

    let mut image_surface = match cairo::ImageSurface::create(cairo::Format::ARgb32, width, height) {
        Ok(s) => s,
        Err(e) => {
            error!("Cairo surface create failed: {:?}", e);
            return;
        }
    };

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

    // Draw rounded rectangle background
    draw_rounded_rect(&cr, width as f64, height as f64, CORNER_RADIUS);
    let _ = cr.set_source_rgba(BG_COLOR.0, BG_COLOR.1, BG_COLOR.2, opacity as f64);
    let _ = cr.fill_preserve();

    // Subtle border
    let _ = cr.set_source_rgba(0.3, 0.3, 0.3, opacity as f64 * 0.5);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // Draw text
    let _ = cr.set_source_rgba(TEXT_COLOR.0, TEXT_COLOR.1, TEXT_COLOR.2, opacity as f64);
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(FONT_SIZE);

    let mut y = PADDING + FONT_SIZE;
    for combo in combos.iter().take(3) {
        if let Ok(extents) = cr.text_extents(&combo.display) {
            let x = (width as f64 - extents.width()) / 2.0;
            cr.move_to(x, y);
            let _ = cr.show_text(&combo.display);
        }
        y += FONT_SIZE + 8.0;
    }

    let _ = cr.show_page();

    image_surface.flush();

    let data_result = image_surface.data();
    if let Ok(data) = data_result {
        shm.write_pixels(&data);
    }
}

fn draw_rounded_rect(cr: &cairo::Context, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(w - r, r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(w - r, h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(r, h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(r, r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

// ── Wayland dispatch implementations ────────────────────────────

// Ignore events from these object types
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
            wl_registry::Event::Global { name, interface, version } => {
                info!(
                    global_id = name,
                    interface = %interface,
                    version,
                    "Registry global (runtime)"
                );
                if interface == "wl_output" {
                    let proxy: wl_output::WlOutput = _proxy.bind(name, version.min(4), qh, ());
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
                    info!(global_id = name, proxy_id, "wl_output bound (runtime)");
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(idx) = state.outputs.iter().position(|o| o.global_id == name) {
                    let removed = state.outputs.remove(idx);
                    state.output_proxies.remove(idx);
                    info!(global_id = name, name = %removed.name, "Output removed");
                } else {
                    info!(global_id = name, "Global removed (not an output)");
                }
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
                if make.is_empty() {
                    // Fallback to synthetic name only when no real name is available
                    info.name = format!("output-{}", info.proxy_id);
                    info!(name = %info.name, global_id = info.global_id, "Output geometry (no make, synthetic name)");
                } else {
                    info.name = make.clone();
                    info!(name = %info.name, global_id = info.global_id, "Output geometry received");
                }
            }
            wl_output::Event::Scale { factor } => {
                info.scale = factor;
                info!(factor, global_id = info.global_id, "Output scale received");
            }
            wl_output::Event::Mode { width, height, flags, .. } => {
                if let WEnum::Value(f) = flags {
                    if f.bits() & 1 != 0 {
                        info.width = width;
                        info.height = height;
                        info!(width, height, global_id = info.global_id, "Output mode received");
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for AppState {
    fn event(_: &mut Self, _: &zwlr_layer_shell_v1::ZwlrLayerShellV1, _: zwlr_layer_shell_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for AppState {
    fn event(state: &mut Self, proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_layer_surface_v1::Event::Closed => warn!("Layer surface closed by compositor"),
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                info!(serial, width, height, "Layer surface configure received");
                proxy.ack_configure(serial);
                state.needs_surface_commit = true;
            }
            _ => {}
        }
    }
}
