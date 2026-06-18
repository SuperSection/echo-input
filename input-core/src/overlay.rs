use crate::events::ShortcutCombo;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Overlay configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    pub position: OverlayPosition,
    pub scale: OverlayScale,
    pub opacity: f32,
    pub display_duration: Duration,
    pub history_length: usize,
    pub theme: Theme,
    pub monitor: Option<String>,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            position: OverlayPosition::TopRight,
            scale: OverlayScale::Medium,
            opacity: 0.9,
            display_duration: Duration::from_secs(3),
            history_length: 5,
            theme: Theme::Dark,
            monitor: None,
        }
    }
}

/// Position of the overlay on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayPosition {
    TopLeft,
    TopRight,
    TopCenter,
    BottomLeft,
    BottomRight,
    BottomCenter,
    Center,
}

/// Overlay size scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayScale {
    Small,
    Medium,
    Large,
    ExtraLarge,
}

impl OverlayScale {
    pub fn font_size(&self) -> f32 {
        match self {
            Self::Small => 16.0,
            Self::Medium => 24.0,
            Self::Large => 32.0,
            Self::ExtraLarge => 48.0,
        }
    }

    pub fn padding(&self) -> f32 {
        match self {
            Self::Small => 8.0,
            Self::Medium => 12.0,
            Self::Large => 16.0,
            Self::ExtraLarge => 24.0,
        }
    }
}

/// Color theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Dark,
    Light,
    System,
}

/// What to display on the overlay.
#[derive(Debug, Clone)]
pub enum DisplayEvent {
    /// Show a single shortcut combo.
    Shortcut(ShortcutCombo),
    /// Show history of shortcuts.
    History(Vec<ShortcutCombo>),
    /// Clear the overlay.
    Clear,
    /// Update configuration.
    UpdateConfig(OverlayConfig),
}
