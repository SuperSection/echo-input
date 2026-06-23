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
    // ── New customization fields ──
    pub keycap_style: KeycapStyle,
    pub colors: ColorSettings,
    pub text: TextSettings,
    pub border: BorderSettings,
    pub background: BackgroundSettings,
    pub animation_type: AnimationType,
    pub animation_speed: f32,
    pub margin_x: f32,
    pub margin_y: f32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            position: OverlayPosition::BottomCenter,
            scale: OverlayScale::Medium,
            opacity: 0.9,
            display_duration: Duration::from_millis(1500),
            history_length: 3,
            theme: Theme::Dark,
            monitor: None,
            keycap_style: KeycapStyle::Laptop,
            colors: ColorSettings::default(),
            text: TextSettings::default(),
            border: BorderSettings::default(),
            background: BackgroundSettings::default(),
            animation_type: AnimationType::Slide,
            animation_speed: 0.5,
            margin_x: 16.0,
            margin_y: 16.0,
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

/// Color theme (system-level).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Dark,
    Light,
    System,
}

/// Keycap visual style preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeycapStyle {
    /// Flat, no shadows or borders
    Minimal,
    /// Current default look with gradients and shadows
    Laptop,
    /// Thinner, darker, lower profile
    LowProfile,
    /// Rounded, colorful, like mechanical keycaps
    PBT,
}

/// Keycap color scheme.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorSettings {
    /// Primary (bottom) color for normal keys (hex).
    pub keycap_primary: String,
    /// Secondary (top) color for normal keys (hex).
    pub keycap_secondary: String,
    /// Whether to use a gradient between primary and secondary.
    pub use_gradient: bool,
    /// Whether modifier keys get separate colors.
    pub highlight_modifiers: bool,
    /// Primary color for modifier keys (hex).
    pub modifier_primary: String,
    /// Secondary color for modifier keys (hex).
    pub modifier_secondary: String,
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self {
            keycap_primary: "#1e1e24".into(),
            keycap_secondary: "#38383f".into(),
            use_gradient: true,
            highlight_modifiers: true,
            modifier_primary: "#3358a8".into(),
            modifier_secondary: "#4d80e6".into(),
        }
    }
}

/// Text typography settings for keycaps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextSettings {
    /// Font size override (None = use scale default).
    pub size: Option<f32>,
    /// Text color for normal keys (hex).
    pub color: String,
    /// Text color for modifier keys (hex).
    pub modifier_color: String,
    /// Text capitalization mode.
    pub caps: TextCaps,
    /// Text variant (full name, short, or icon).
    pub variant: TextVariant,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            size: None,
            color: "#f5f5f5".into(),
            modifier_color: "#b3d4fc".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        }
    }
}

/// Text capitalization mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextCaps {
    Uppercase,
    Capitalize,
    Lowercase,
}

/// Text display variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextVariant {
    /// Full key name (e.g., "Control", "Escape")
    Full,
    /// Short abbreviation (e.g., "Ctrl", "Esc")
    Short,
    /// Single character where possible
    Icon,
}

/// Keycap border settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BorderSettings {
    /// Whether borders are enabled.
    pub enabled: bool,
    /// Border color for normal keys (hex).
    pub color: String,
    /// Border width in pixels.
    pub width: f32,
    /// Corner radius as fraction of keycap height (0.0 - 1.0).
    pub radius: f32,
    /// Border color for modifier keys (hex).
    pub modifier_color: String,
}

impl Default for BorderSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            color: "#5a5a60".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#4d80e6".into(),
        }
    }
}

/// Background fill settings for keycaps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundSettings {
    /// Whether background fill is enabled.
    pub enabled: bool,
    /// Background color with optional alpha (hex, e.g., "#00000099").
    pub color: String,
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            color: "#00000080".into(),
        }
    }
}

/// Animation type for keycap entry/exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimationType {
    /// No animation, instant show/hide
    None,
    /// Opacity fade only
    Fade,
    /// Scale from small to full size
    Zoom,
    /// Gentle floating motion
    Float,
    /// Slide up/down with scale (default)
    Slide,
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
    UpdateConfig(Box<OverlayConfig>),
}
