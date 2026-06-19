use input_core::events::ShortcutCombo;
use input_core::ipc::{MessageBus, OverlayCommand, SettingsUpdate, ShortcutEvent};
use input_core::overlay::{DisplayEvent, OverlayConfig};
use input_core::traits::OverlayRenderer;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::broadcast;
use tracing::{debug, trace, warn};

/// Cross-platform overlay state manager.
///
/// Manages what's displayed on the overlay, handles fade-in/fade-out
/// timing, and history display. The actual rendering is delegated to
/// platform-specific `OverlayRenderer` implementations.
pub struct OverlayState {
    /// Current display items.
    items: VecDeque<DisplayedShortcut>,
    /// Configuration.
    config: OverlayConfig,
    /// Channel to send display events to the renderer.
    tx: broadcast::Sender<DisplayEvent>,
}

struct DisplayedShortcut {
    combo: ShortcutCombo,
    shown_at: Instant,
}

impl OverlayState {
    pub fn new(config: OverlayConfig) -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            items: VecDeque::new(),
            config,
            tx,
        }
    }

    /// Subscribe to display events (for the renderer).
    pub fn subscribe(&self) -> broadcast::Receiver<DisplayEvent> {
        self.tx.subscribe()
    }

    /// Show a new shortcut on the overlay.
    pub fn show_shortcut(&mut self, combo: ShortcutCombo) {
        self.items.push_front(DisplayedShortcut {
            combo: combo.clone(),
            shown_at: Instant::now(),
        });

        self.trim_expired();

        while self.items.len() > self.config.history_length {
            self.items.pop_back();
        }

        let event = if self.config.history_length > 1 {
            DisplayEvent::History(self.items.iter().map(|i| i.combo.clone()).collect())
        } else {
            DisplayEvent::Shortcut(combo)
        };

        let _ = self.tx.send(event);
    }

    /// Clear the overlay.
    pub fn clear(&mut self) {
        self.items.clear();
        let _ = self.tx.send(DisplayEvent::Clear);
    }

    /// Update configuration.
    pub fn update_config(&mut self, config: OverlayConfig) {
        self.config = config;
        let _ = self.tx.send(DisplayEvent::UpdateConfig(self.config.clone()));
    }

    /// Get current configuration.
    pub fn config(&self) -> &OverlayConfig {
        &self.config
    }

    fn trim_expired(&mut self) {
        let now = Instant::now();
        while let Some(back) = self.items.back() {
            if now.duration_since(back.shown_at) > self.config.display_duration {
                self.items.pop_back();
            } else {
                break;
            }
        }
    }
}

/// Overlay manager that coordinates state and rendering.
///
/// Integrates with the `MessageBus` to receive shortcuts, commands,
/// and settings updates. The overlay can be restarted independently
/// of the input capture by sending `OverlayCommand::Stop` followed
/// by `OverlayCommand::Start`.
///
/// # Usage
///
/// ```ignore
/// let mut overlay = OverlayManager::new(config);
/// overlay.run(bus);  // Spawns internal task, runs independently
/// ```
pub struct OverlayManager {
    state: OverlayState,
    running: bool,
}

impl OverlayManager {
    pub fn new(config: OverlayConfig) -> Self {
        Self {
            state: OverlayState::new(config),
            running: false,
        }
    }

    /// Start listening to the message bus.
    ///
    /// Spawns a single task that uses `select!` to handle all three
    /// channels (shortcuts, commands, settings) concurrently. The
    /// overlay runs independently after this call.
    pub fn run(&mut self, bus: MessageBus) {
        let mut shortcut_rx = bus.subscribe_shortcut();
        let mut command_rx = bus.subscribe_command();
        let mut settings_rx = bus.subscribe_settings();

        // Subscribe to display events (for future renderer integration)
        let _display_rx = self.state.subscribe();

        // Move state into the task
        let mut state = OverlayState {
            items: VecDeque::new(),
            config: self.state.config().clone(),
            tx: {
                let (tx, _) = broadcast::channel(64);
                tx
            },
        };

        // Copy the existing display sender
        state.tx = self.state.tx.clone();

        // Copy existing items
        state.items = std::mem::take(&mut self.state.items);

        let mut running = true;

        tokio::spawn(async move {
            debug!("Overlay task started");

            loop {
                tokio::select! {
                    // Handle shortcut events
                    result = shortcut_rx.recv() => {
                        match result {
                            Ok(event) => {
                                if running {
                                    state.show_shortcut(event.combo);
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!("Overlay missed {} shortcut events", n);
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                debug!("Overlay shortcut channel closed");
                                break;
                            }
                        }
                    }
                    // Handle overlay commands
                    result = command_rx.recv() => {
                        match result {
                            Ok(cmd) => {
                                trace!(command = ?cmd, "Overlay received command");
                                match cmd {
                                    OverlayCommand::Start => {
                                        running = true;
                                    }
                                    OverlayCommand::Stop => {
                                        state.clear();
                                        running = false;
                                    }
                                    OverlayCommand::Restart => {
                                        state.clear();
                                        running = true;
                                    }
                                    OverlayCommand::Clear => {
                                        state.clear();
                                    }
                                    OverlayCommand::UpdateConfig(config) => {
                                        state.update_config(config);
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!("Overlay missed {} commands", n);
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                debug!("Overlay command channel closed");
                                break;
                            }
                        }
                    }
                    // Handle settings updates
                    result = settings_rx.recv() => {
                        match result {
                            Ok(update) => {
                                trace!(update = ?update, "Overlay received settings update");
                                let mut config = state.config().clone();
                                update.apply(&mut config);
                                state.update_config(config);
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!("Overlay missed {} settings updates", n);
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                debug!("Overlay settings channel closed");
                                break;
                            }
                        }
                    }
                }
            }

            debug!("Overlay task ended");
        });

        self.running = true;
        debug!("OverlayManager started with bus integration");
    }

    pub fn start(&mut self) {
        self.running = true;
        debug!("OverlayManager started");
    }

    pub fn stop(&mut self) {
        self.state.clear();
        self.running = false;
        debug!("OverlayManager stopped");
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Handle a shortcut event directly (without the bus).
    pub fn handle_shortcut(&mut self, event: ShortcutEvent) {
        if self.running {
            self.state.show_shortcut(event.combo);
        }
    }

    /// Handle an overlay command directly (without the bus).
    pub fn handle_command(&mut self, cmd: OverlayCommand) {
        match cmd {
            OverlayCommand::Start => self.start(),
            OverlayCommand::Stop => self.stop(),
            OverlayCommand::Restart => {
                self.stop();
                self.start();
            }
            OverlayCommand::Clear => self.clear(),
            OverlayCommand::UpdateConfig(config) => self.update_config(config),
        }
    }

    /// Handle a settings update directly (without the bus).
    pub fn handle_settings(&mut self, update: SettingsUpdate) {
        let mut config = self.state.config().clone();
        update.apply(&mut config);
        self.update_config(config);
    }

    pub fn clear(&mut self) {
        self.state.clear();
    }

    pub fn update_config(&mut self, config: OverlayConfig) {
        self.state.update_config(config);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DisplayEvent> {
        self.state.subscribe()
    }
}

/// Mock overlay renderer for testing.
///
/// Records all events it receives without actually rendering anything.
/// Useful for verifying that the event pipeline works correctly.
pub struct MockRenderer {
    running: AtomicBool,
    start_count: AtomicUsize,
    stop_count: AtomicUsize,
    update_count: AtomicUsize,
    last_display_event: std::sync::Mutex<Option<DisplayEvent>>,
    _bus: Option<MessageBus>,
}

impl MockRenderer {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            start_count: AtomicUsize::new(0),
            stop_count: AtomicUsize::new(0),
            update_count: AtomicUsize::new(0),
            last_display_event: std::sync::Mutex::new(None),
            _bus: None,
        }
    }

    pub fn with_bus(bus: MessageBus) -> Self {
        Self {
            running: AtomicBool::new(false),
            start_count: AtomicUsize::new(0),
            stop_count: AtomicUsize::new(0),
            update_count: AtomicUsize::new(0),
            last_display_event: std::sync::Mutex::new(None),
            _bus: Some(bus),
        }
    }

    pub fn start_count(&self) -> usize {
        self.start_count.load(Ordering::Relaxed)
    }

    pub fn stop_count(&self) -> usize {
        self.stop_count.load(Ordering::Relaxed)
    }

    pub fn update_count(&self) -> usize {
        self.update_count.load(Ordering::Relaxed)
    }

    pub fn last_display_event(&self) -> Option<DisplayEvent> {
        self.last_display_event.lock().unwrap().clone()
    }
}

impl Default for MockRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl OverlayRenderer for MockRenderer {
    async fn start(&mut self, _config: OverlayConfig) -> anyhow::Result<()> {
        self.running.store(true, Ordering::Relaxed);
        self.start_count.fetch_add(1, Ordering::Relaxed);
        debug!("MockRenderer started");
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::Relaxed);
        self.stop_count.fetch_add(1, Ordering::Relaxed);
        debug!("MockRenderer stopped");
        Ok(())
    }

    fn update(&self, event: DisplayEvent) -> anyhow::Result<()> {
        self.update_count.fetch_add(1, Ordering::Relaxed);
        *self.last_display_event.lock().unwrap() = Some(event);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    fn name(&self) -> &str {
        "MockRenderer"
    }
}
