use crate::events::{InputEvent, ShortcutCombo};
use crate::overlay::{
    AnimationType, BackgroundSettings, BorderSettings, ColorSettings, KeycapStyle, OverlayConfig,
    OverlayPosition, OverlayScale, TextSettings, Theme,
};
use std::time::{Duration, SystemTime};
use tokio::sync::broadcast;
use tracing::trace;

/// A processed shortcut event ready for overlay display.
#[derive(Debug, Clone)]
pub struct ShortcutEvent {
    pub combo: ShortcutCombo,
    pub timestamp: SystemTime,
}

impl ShortcutEvent {
    pub fn new(combo: ShortcutCombo) -> Self {
        Self {
            combo,
            timestamp: SystemTime::now(),
        }
    }
}

/// Commands to control the overlay lifecycle.
#[derive(Debug, Clone)]
pub enum OverlayCommand {
    /// Start the overlay.
    Start,
    /// Stop the overlay (keeps input capture running).
    Stop,
    /// Restart the overlay (stop then start).
    Restart,
    /// Clear all displayed items.
    Clear,
    /// Update overlay configuration.
    UpdateConfig(Box<OverlayConfig>),
}

/// Settings changes from the Tauri application.
///
/// Each variant updates a single setting. Use `Batch` to apply
/// multiple changes atomically.
#[derive(Debug, Clone)]
pub enum SettingsUpdate {
    /// How long shortcuts remain visible.
    DisplayDuration(Duration),
    /// Color theme.
    Theme(Theme),
    /// Screen position.
    Position(OverlayPosition),
    /// Window opacity (0.0 - 1.0).
    Opacity(f32),
    /// Size scale.
    Scale(OverlayScale),
    /// Maximum number of history items.
    HistoryLength(usize),
    /// Keycap visual style preset.
    KeycapStyle(KeycapStyle),
    /// Keycap color settings.
    Colors(ColorSettings),
    /// Text typography settings.
    Text(TextSettings),
    /// Border settings.
    Border(BorderSettings),
    /// Background fill settings.
    Background(BackgroundSettings),
    /// Animation type.
    AnimationType(AnimationType),
    /// Animation speed (0.05 - 1.0).
    AnimationSpeed(f32),
    /// Horizontal margin from screen edge.
    MarginX(f32),
    /// Vertical margin from screen edge.
    MarginY(f32),
    /// Apply multiple settings at once.
    Batch(Vec<SettingsUpdate>),
}

impl SettingsUpdate {
    /// Apply this update to an `OverlayConfig`.
    pub fn apply(&self, config: &mut OverlayConfig) {
        match self {
            Self::DisplayDuration(d) => config.display_duration = *d,
            Self::Theme(t) => config.theme = *t,
            Self::Position(p) => config.position = *p,
            Self::Opacity(o) => config.opacity = o.clamp(0.0, 1.0),
            Self::Scale(s) => config.scale = *s,
            Self::HistoryLength(n) => config.history_length = *n,
            Self::KeycapStyle(s) => config.keycap_style = *s,
            Self::Colors(c) => config.colors = c.clone(),
            Self::Text(t) => config.text = t.clone(),
            Self::Border(b) => config.border = b.clone(),
            Self::Background(b) => config.background = b.clone(),
            Self::AnimationType(a) => config.animation_type = *a,
            Self::AnimationSpeed(s) => config.animation_speed = s.clamp(0.05, 1.0),
            Self::MarginX(m) => config.margin_x = *m,
            Self::MarginY(m) => config.margin_y = *m,
            Self::Batch(updates) => {
                for update in updates {
                    update.apply(config);
                }
            }
        }
    }
}

/// Unified message type for the IPC bus.
#[derive(Debug, Clone)]
pub enum Message {
    /// Raw input event from a capture provider.
    Input(InputEvent),
    /// Processed shortcut event for overlay display.
    Shortcut(ShortcutEvent),
    /// Command to control the overlay.
    Command(Box<OverlayCommand>),
    /// Settings update from the application.
    Settings(SettingsUpdate),
}

/// Central IPC message bus.
///
/// Uses typed `tokio::sync::broadcast` channels for each message category.
/// Subscribers receive only the message types they care about, with
/// independent backpressure per channel.
///
/// # Usage
///
/// ```ignore
/// let bus = MessageBus::new(1024);
///
/// // Capture provider publishes input events
/// bus.publish_input(InputEvent::Keyboard(kbd_event))?;
///
/// // Event processor subscribes to input, publishes shortcuts
/// let mut input_rx = bus.subscribe_input();
/// bus.publish_shortcut(ShortcutEvent::new(combo))?;
///
/// // Overlay subscribes to shortcuts, commands, and settings
/// let mut shortcut_rx = bus.subscribe_shortcut();
/// let mut command_rx = bus.subscribe_command();
/// let mut settings_rx = bus.subscribe_settings();
/// ```
#[derive(Debug, Clone)]
pub struct MessageBus {
    input_tx: broadcast::Sender<InputEvent>,
    shortcut_tx: broadcast::Sender<ShortcutEvent>,
    command_tx: broadcast::Sender<OverlayCommand>,
    settings_tx: broadcast::Sender<SettingsUpdate>,
}

impl MessageBus {
    /// Create a new message bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (input_tx, _) = broadcast::channel(capacity);
        let (shortcut_tx, _) = broadcast::channel(capacity);
        let (command_tx, _) = broadcast::channel(capacity);
        let (settings_tx, _) = broadcast::channel(capacity);
        Self {
            input_tx,
            shortcut_tx,
            command_tx,
            settings_tx,
        }
    }

    // ── Input channel ──────────────────────────────────────────

    /// Get a clone of the input sender (for capture providers).
    pub fn input_sender(&self) -> broadcast::Sender<InputEvent> {
        self.input_tx.clone()
    }

    /// Publish a raw input event.
    pub fn publish_input(&self, event: InputEvent) -> usize {
        let count = self.input_tx.receiver_count();
        let _ = self.input_tx.send(event);
        trace!("Input event published to {} subscriber(s)", count);
        count
    }

    /// Subscribe to raw input events.
    pub fn subscribe_input(&self) -> broadcast::Receiver<InputEvent> {
        self.input_tx.subscribe()
    }

    // ── Shortcut channel ───────────────────────────────────────

    /// Publish a processed shortcut event.
    pub fn publish_shortcut(&self, event: ShortcutEvent) -> usize {
        let count = self.shortcut_tx.receiver_count();
        let _ = self.shortcut_tx.send(event);
        trace!("Shortcut event published to {} subscriber(s)", count);
        count
    }

    /// Subscribe to processed shortcut events.
    pub fn subscribe_shortcut(&self) -> broadcast::Receiver<ShortcutEvent> {
        self.shortcut_tx.subscribe()
    }

    // ── Command channel ────────────────────────────────────────

    /// Publish an overlay command.
    pub fn publish_command(&self, cmd: OverlayCommand) -> usize {
        let count = self.command_tx.receiver_count();
        let _ = self.command_tx.send(cmd);
        trace!("Overlay command published to {} subscriber(s)", count);
        count
    }

    /// Subscribe to overlay commands.
    pub fn subscribe_command(&self) -> broadcast::Receiver<OverlayCommand> {
        self.command_tx.subscribe()
    }

    // ── Settings channel ───────────────────────────────────────

    /// Publish a settings update.
    pub fn publish_settings(&self, update: SettingsUpdate) -> usize {
        let count = self.settings_tx.receiver_count();
        let _ = self.settings_tx.send(update);
        trace!("Settings update published to {} subscriber(s)", count);
        count
    }

    /// Subscribe to settings updates.
    pub fn subscribe_settings(&self) -> broadcast::Receiver<SettingsUpdate> {
        self.settings_tx.subscribe()
    }

    // ── Utility ────────────────────────────────────────────────

    /// Check if any subscribers exist for any channel.
    pub fn has_subscribers(&self) -> bool {
        self.input_tx.receiver_count() > 0
            || self.shortcut_tx.receiver_count() > 0
            || self.command_tx.receiver_count() > 0
            || self.settings_tx.receiver_count() > 0
    }

    /// Get subscriber counts for each channel.
    pub fn subscriber_counts(&self) -> SubscriberCounts {
        SubscriberCounts {
            input: self.input_tx.receiver_count(),
            shortcut: self.shortcut_tx.receiver_count(),
            command: self.command_tx.receiver_count(),
            settings: self.settings_tx.receiver_count(),
        }
    }
}

/// Snapshot of subscriber counts per channel.
#[derive(Debug, Clone, Copy)]
pub struct SubscriberCounts {
    pub input: usize,
    pub shortcut: usize,
    pub command: usize,
    pub settings: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{KeyState, KeyboardEvent};
    use crate::keys::VirtualKey;

    #[tokio::test]
    async fn test_message_bus_input_publish_subscribe() {
        let bus = MessageBus::new(16);
        let mut rx = bus.subscribe_input();

        let event = InputEvent::Keyboard(KeyboardEvent {
            key: VirtualKey::A,
            state: KeyState::Pressed,
            timestamp: SystemTime::now(),
            native_code: 30,
        });

        bus.publish_input(event.clone());

        let received = rx.recv().await.unwrap();
        assert!(matches!(received, InputEvent::Keyboard(_)));
    }

    #[tokio::test]
    async fn test_message_bus_shortcut_publish_subscribe() {
        let bus = MessageBus::new(16);
        let mut rx = bus.subscribe_shortcut();

        let combo = ShortcutCombo::new(
            crate::events::ModifierState {
                ctrl: true,
                ..Default::default()
            },
            Some(VirtualKey::C),
        );
        let event = ShortcutEvent::new(combo);

        bus.publish_shortcut(event);

        let received = rx.recv().await.unwrap();
        assert_eq!(received.combo.display, "Ctrl + C");
    }

    #[tokio::test]
    async fn test_message_bus_command_publish_subscribe() {
        let bus = MessageBus::new(16);
        let mut rx = bus.subscribe_command();

        bus.publish_command(OverlayCommand::Start);

        let received = rx.recv().await.unwrap();
        assert!(matches!(received, OverlayCommand::Start));
    }

    #[tokio::test]
    async fn test_message_bus_settings_publish_subscribe() {
        let bus = MessageBus::new(16);
        let mut rx = bus.subscribe_settings();

        bus.publish_settings(SettingsUpdate::Opacity(0.5));

        let received = rx.recv().await.unwrap();
        assert!(matches!(received, SettingsUpdate::Opacity(0.5)));
    }

    #[test]
    fn test_settings_update_apply() {
        use crate::overlay::{ColorSettings, TextCaps, TextSettings, TextVariant};

        let mut config = OverlayConfig::default();

        SettingsUpdate::Theme(Theme::Light).apply(&mut config);
        assert_eq!(config.theme, Theme::Light);

        SettingsUpdate::Opacity(0.3).apply(&mut config);
        assert_eq!(config.opacity, 0.3);

        SettingsUpdate::Position(crate::overlay::OverlayPosition::TopLeft).apply(&mut config);
        assert_eq!(config.position, crate::overlay::OverlayPosition::TopLeft);

        // Clamp opacity
        SettingsUpdate::Opacity(2.0).apply(&mut config);
        assert_eq!(config.opacity, 1.0);

        SettingsUpdate::Opacity(-1.0).apply(&mut config);
        assert_eq!(config.opacity, 0.0);

        // Test new variants
        SettingsUpdate::KeycapStyle(crate::overlay::KeycapStyle::PBT).apply(&mut config);
        assert_eq!(config.keycap_style, crate::overlay::KeycapStyle::PBT);

        SettingsUpdate::AnimationType(crate::overlay::AnimationType::Fade).apply(&mut config);
        assert_eq!(config.animation_type, crate::overlay::AnimationType::Fade);

        SettingsUpdate::AnimationSpeed(0.8).apply(&mut config);
        assert_eq!(config.animation_speed, 0.8);

        // Clamp animation speed
        SettingsUpdate::AnimationSpeed(2.0).apply(&mut config);
        assert_eq!(config.animation_speed, 1.0);

        SettingsUpdate::MarginX(32.0).apply(&mut config);
        assert_eq!(config.margin_x, 32.0);

        SettingsUpdate::MarginY(48.0).apply(&mut config);
        assert_eq!(config.margin_y, 48.0);

        let colors = ColorSettings {
            keycap_primary: "#ff0000".into(),
            keycap_secondary: "#cc0000".into(),
            use_gradient: false,
            highlight_modifiers: false,
            modifier_primary: "#00ff00".into(),
            modifier_secondary: "#00cc00".into(),
        };
        SettingsUpdate::Colors(colors.clone()).apply(&mut config);
        assert_eq!(config.colors.keycap_primary, "#ff0000");
        assert!(!config.colors.use_gradient);

        let text = TextSettings {
            size: Some(32.0),
            color: "#ffffff".into(),
            modifier_color: "#aaaaaa".into(),
            caps: TextCaps::Lowercase,
            variant: TextVariant::Short,
        };
        SettingsUpdate::Text(text).apply(&mut config);
        assert_eq!(config.text.size, Some(32.0));
        assert_eq!(config.text.caps, TextCaps::Lowercase);
    }

    #[test]
    fn test_settings_batch_apply() {
        use crate::overlay::KeycapStyle;

        let mut config = OverlayConfig::default();

        let batch = SettingsUpdate::Batch(vec![
            SettingsUpdate::Theme(Theme::Light),
            SettingsUpdate::Opacity(0.7),
            SettingsUpdate::Position(crate::overlay::OverlayPosition::Center),
            SettingsUpdate::KeycapStyle(KeycapStyle::Minimal),
            SettingsUpdate::AnimationSpeed(0.3),
        ]);

        batch.apply(&mut config);
        assert_eq!(config.theme, Theme::Light);
        assert_eq!(config.opacity, 0.7);
        assert_eq!(config.position, crate::overlay::OverlayPosition::Center);
        assert_eq!(config.keycap_style, KeycapStyle::Minimal);
        assert_eq!(config.animation_speed, 0.3);
    }

    #[test]
    fn test_subscriber_counts() {
        let bus = MessageBus::new(16);
        let _input_rx = bus.subscribe_input();
        let _input_rx2 = bus.subscribe_input();
        let _shortcut_rx = bus.subscribe_shortcut();

        let counts = bus.subscriber_counts();
        assert_eq!(counts.input, 2);
        assert_eq!(counts.shortcut, 1);
        assert_eq!(counts.command, 0);
        assert_eq!(counts.settings, 0);
    }

    #[test]
    fn test_has_subscribers() {
        let bus = MessageBus::new(16);
        assert!(!bus.has_subscribers());

        let _rx = bus.subscribe_input();
        assert!(bus.has_subscribers());
    }

    #[test]
    fn test_overlay_command_variants() {
        let bus = MessageBus::new(16);

        // Test all command variants
        bus.publish_command(OverlayCommand::Start);
        bus.publish_command(OverlayCommand::Stop);
        bus.publish_command(OverlayCommand::Restart);
        bus.publish_command(OverlayCommand::Clear);
        bus.publish_command(OverlayCommand::UpdateConfig(Box::new(OverlayConfig::default())));
    }

    #[test]
    fn test_shortcut_event_timestamp() {
        let before = SystemTime::now();
        let combo = ShortcutCombo::new(Default::default(), Some(VirtualKey::A));
        let event = ShortcutEvent::new(combo);
        let after = SystemTime::now();

        assert!(event.timestamp >= before);
        assert!(event.timestamp <= after);
    }
}
