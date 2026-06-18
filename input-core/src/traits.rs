use crate::events::{InputEvent, ProcessedEvent, ModifierState, ShortcutCombo};
use crate::overlay::{DisplayEvent, OverlayConfig};
use anyhow::Result;
use std::sync::Arc;

/// Feature flags describing what a capture provider supports.
#[derive(Debug, Clone, Default)]
pub struct CaptureFeatures {
    pub keyboard: bool,
    pub mouse: bool,
    pub scroll: bool,
    pub gamepad: bool,
    /// Provider can detect which application has focus.
    pub app_context: bool,
}

/// Platform-specific keyboard capture provider.
///
/// Implementations read raw input events from the platform and broadcast
/// them for processing. The provider owns the capture lifecycle.
///
/// # Platform Implementations
///
/// - **Linux:** `EvdevCapture` reads from `/dev/input/event*` via evdev
/// - **Windows:** `WindowsCapture` uses `SetWindowsHookEx`
/// - **macOS:** `MacosCapture` uses `CGEventTap`
#[async_trait::async_trait]
pub trait KeyboardCaptureProvider: Send + Sync {
    /// Start capturing keyboard events.
    ///
    /// After this returns, events will be available via `subscribe()`.
    async fn start(&mut self) -> Result<()>;

    /// Stop capturing keyboard events.
    async fn stop(&mut self) -> Result<()>;

    /// Subscribe to input events.
    ///
    /// Returns a broadcast receiver. Each subscriber gets its own copy
    /// of every event. Use `broadcast::Receiver::resubscribe()` for
    /// multiple consumers.
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<InputEvent>;

    /// Report which features this provider supports.
    fn features(&self) -> CaptureFeatures;

    /// Provider name for logging/debugging.
    fn name(&self) -> &str;
}

/// Platform-specific mouse capture provider.
///
/// Can be added later without modifying existing keyboard capture code.
/// Follows the same lifecycle pattern as `KeyboardCaptureProvider`.
#[async_trait::async_trait]
pub trait MouseCaptureProvider: Send + Sync {
    /// Start capturing mouse events.
    async fn start(&mut self) -> Result<()>;

    /// Stop capturing mouse events.
    async fn stop(&mut self) -> Result<()>;

    /// Subscribe to mouse events.
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<InputEvent>;

    /// Report features.
    fn features(&self) -> CaptureFeatures;

    /// Provider name.
    fn name(&self) -> &str;
}

/// Cross-platform overlay renderer.
///
/// Each platform provides its own renderer:
/// - **Linux Wayland:** Layer-shell surface with Cairo/EGL rendering
/// - **Linux X11:** Transparent always-on-top window
/// - **Windows:** Transparent layered window
/// - **macOS:** NSPanel with panel level
#[async_trait::async_trait]
pub trait OverlayRenderer: Send + Sync {
    /// Initialize the overlay with configuration.
    async fn start(&mut self, config: OverlayConfig) -> Result<()>;

    /// Tear down the overlay.
    async fn stop(&mut self) -> Result<()>;

    /// Update the overlay display content.
    fn update(&self, event: DisplayEvent) -> Result<()>;

    /// Check if the overlay is currently running.
    fn is_running(&self) -> bool;

    /// Renderer name for logging.
    fn name(&self) -> &str;
}

/// Platform-independent event processor.
///
/// Consumes raw `InputEvent`s and produces `ProcessedEvent`s ready
/// for the overlay. Manages modifier state, event grouping, and
/// history.
pub trait EventProcessor: Send + Sync {
    /// Process a raw input event.
    ///
    /// Returns zero or more display-ready events. For example, a key
    /// release might complete a shortcut combo and produce a
    /// `ProcessedEvent::Shortcut`.
    fn process(&mut self, event: InputEvent) -> Vec<ProcessedEvent>;

    /// Get current modifier state.
    fn modifier_state(&self) -> ModifierState;

    /// Get the shortcut currently being composed (held modifiers + key).
    fn current_compose(&self) -> Option<ShortcutCombo>;

    /// Get recent shortcut history (most recent first).
    fn history(&self) -> &[ShortcutCombo];

    /// Clear history.
    fn clear_history(&mut self);

    /// Update processor configuration.
    fn update_config(&mut self, config: ProcessorConfig);
}

/// Configuration for the event processor.
#[derive(Debug, Clone)]
pub struct ProcessorConfig {
    /// Maximum number of shortcuts to keep in history.
    pub history_length: usize,
    /// Whether to group modifier+key combos into single shortcuts.
    pub group_shortcuts: bool,
    /// Minimum time between duplicate events (deduplication).
    pub dedup_window: std::time::Duration,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            history_length: 10,
            group_shortcuts: true,
            dedup_window: std::time::Duration::from_millis(50),
        }
    }
}

/// Type alias for a shared, boxed capture provider.
pub type SharedCapture = Arc<dyn KeyboardCaptureProvider>;

/// Factory for creating platform-specific overlay renderers.
///
/// Each platform provides its own factory implementation:
/// - **Linux:** `WaylandRendererFactory`
/// - **Windows:** `WindowsRendererFactory` (future)
/// - **macOS:** `MacRendererFactory` (future)
///
/// The factory pattern allows the application to create the correct
/// renderer at startup without platform-specific imports in main.
pub trait OverlayRendererFactory: Send + Sync {
    /// Create a new renderer for this platform.
    fn create(&self, bus: crate::ipc::MessageBus) -> Box<dyn OverlayRenderer>;

    /// Platform name for logging.
    fn platform_name(&self) -> &str;
}

/// Type alias for a shared, boxed renderer factory.
pub type SharedRendererFactory = Arc<dyn OverlayRendererFactory>;
