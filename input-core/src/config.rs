use crate::overlay::{OverlayConfig, OverlayPosition, OverlayScale, Theme};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info, warn};

const CONFIG_DIR: &str = "echoinput";
const CONFIG_FILE: &str = "config.toml";
const DISPLAY_DURATION_MS_DEFAULT: u64 = 1500;

/// File-backed configuration for EchoInput.
///
/// Serializes to `~/.config/echoinput/config.toml`. All fields are
/// optional — missing fields fall back to defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    /// Overlay screen position.
    pub position: Option<String>,
    /// Size scale.
    pub scale: Option<String>,
    /// Opacity 0.0–1.0.
    pub opacity: Option<f32>,
    /// How long shortcuts stay visible (milliseconds).
    pub display_duration_ms: Option<u64>,
    /// Max history items shown simultaneously.
    pub history_length: Option<usize>,
    /// Color theme.
    pub theme: Option<String>,
    /// Monitor name (None = default output).
    pub monitor: Option<String>,
}

impl FileConfig {
    /// Resolve the config file path: `~/.config/echoinput/config.toml`.
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join(CONFIG_DIR).join(CONFIG_FILE))
    }

    /// Load config from disk. Returns defaults if the file doesn't exist
    /// or can't be parsed.
    pub fn load() -> Self {
        let path = match Self::config_path() {
            Some(p) => p,
            None => {
                warn!("Could not determine config directory, using defaults");
                return Self::defaults_toml();
            }
        };

        if !path.exists() {
            info!("No config file found at {}, creating default", path.display());
            let defaults = Self::defaults_toml();
            if let Err(e) = defaults.save() {
                warn!("Failed to write default config: {}", e);
            }
            return defaults;
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<FileConfig>(&contents) {
                Ok(config) => {
                    debug!("Loaded config from {}", path.display());
                    config
                }
                Err(e) => {
                    warn!("Failed to parse config at {}: {}. Using defaults.", path.display(), e);
                    Self::defaults_toml()
                }
            },
            Err(e) => {
                warn!("Failed to read config at {}: {}. Using defaults.", path.display(), e);
                Self::defaults_toml()
            }
        }
    }

    /// Save this config to disk.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = toml::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;
        info!("Config saved to {}", path.display());
        Ok(())
    }

    /// Convert to `OverlayConfig`, filling in defaults for missing fields.
    pub fn to_overlay_config(&self) -> OverlayConfig {
        OverlayConfig {
            position: self.position.as_deref().and_then(parse_position).unwrap_or(OverlayPosition::BottomCenter),
            scale: self.scale.as_deref().and_then(parse_scale).unwrap_or(OverlayScale::Medium),
            opacity: self.opacity.unwrap_or(0.9),
            display_duration: Duration::from_millis(
                self.display_duration_ms.unwrap_or(DISPLAY_DURATION_MS_DEFAULT),
            ),
            history_length: self.history_length.unwrap_or(3),
            theme: self.theme.as_deref().and_then(parse_theme).unwrap_or(Theme::Dark),
            monitor: self.monitor.clone(),
        }
    }

    /// Build a FileConfig from an OverlayConfig (for saving current state).
    pub fn from_overlay_config(config: &OverlayConfig) -> Self {
        Self {
            position: Some(format!("{:?}", config.position)),
            scale: Some(format!("{:?}", config.scale)),
            opacity: Some(config.opacity),
            display_duration_ms: Some(config.display_duration.as_millis() as u64),
            history_length: Some(config.history_length),
            theme: Some(format!("{:?}", config.theme)),
            monitor: config.monitor.clone(),
        }
    }

    fn defaults_toml() -> Self {
        Self {
            position: Some("BottomCenter".into()),
            scale: Some("Medium".into()),
            opacity: Some(0.9),
            display_duration_ms: Some(DISPLAY_DURATION_MS_DEFAULT),
            history_length: Some(3),
            theme: Some("Dark".into()),
            monitor: None,
        }
    }
}

impl Default for FileConfig {
    fn default() -> Self {
        Self::defaults_toml()
    }
}

fn parse_position(s: &str) -> Option<OverlayPosition> {
    match s {
        "TopLeft" => Some(OverlayPosition::TopLeft),
        "TopRight" => Some(OverlayPosition::TopRight),
        "TopCenter" => Some(OverlayPosition::TopCenter),
        "BottomLeft" => Some(OverlayPosition::BottomLeft),
        "BottomRight" => Some(OverlayPosition::BottomRight),
        "BottomCenter" => Some(OverlayPosition::BottomCenter),
        "Center" => Some(OverlayPosition::Center),
        _ => None,
    }
}

fn parse_scale(s: &str) -> Option<OverlayScale> {
    match s {
        "Small" => Some(OverlayScale::Small),
        "Medium" => Some(OverlayScale::Medium),
        "Large" => Some(OverlayScale::Large),
        "ExtraLarge" => Some(OverlayScale::ExtraLarge),
        _ => None,
    }
}

fn parse_theme(s: &str) -> Option<Theme> {
    match s {
        "Dark" => Some(Theme::Dark),
        "Light" => Some(Theme::Light),
        "System" => Some(Theme::System),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_config_defaults() {
        let config = FileConfig::default();
        assert_eq!(config.position.as_deref(), Some("BottomCenter"));
        assert_eq!(config.scale.as_deref(), Some("Medium"));
        assert_eq!(config.opacity, Some(0.9));
        assert_eq!(config.display_duration_ms, Some(1500));
        assert_eq!(config.history_length, Some(3));
        assert_eq!(config.theme.as_deref(), Some("Dark"));
        assert!(config.monitor.is_none());
    }

    #[test]
    fn test_to_overlay_config() {
        let file_config = FileConfig::default();
        let overlay_config = file_config.to_overlay_config();
        assert_eq!(overlay_config.position, OverlayPosition::BottomCenter);
        assert_eq!(overlay_config.scale, OverlayScale::Medium);
        assert_eq!(overlay_config.opacity, 0.9);
        assert_eq!(overlay_config.display_duration, Duration::from_millis(1500));
        assert_eq!(overlay_config.history_length, 3);
        assert_eq!(overlay_config.theme, Theme::Dark);
    }

    #[test]
    fn test_roundtrip() {
        let original = FileConfig {
            position: Some("TopLeft".into()),
            scale: Some("Large".into()),
            opacity: Some(0.7),
            display_duration_ms: Some(2000),
            history_length: Some(5),
            theme: Some("Light".into()),
            monitor: Some("DP-1".into()),
        };

        let overlay_config = original.to_overlay_config();
        let restored = FileConfig::from_overlay_config(&overlay_config);

        assert_eq!(restored.position.as_deref(), Some("TopLeft"));
        assert_eq!(restored.scale.as_deref(), Some("Large"));
        assert_eq!(restored.opacity, Some(0.7));
        assert_eq!(restored.display_duration_ms, Some(2000));
        assert_eq!(restored.history_length, Some(5));
        assert_eq!(restored.theme.as_deref(), Some("Light"));
        assert_eq!(restored.monitor.as_deref(), Some("DP-1"));
    }

    #[test]
    fn test_parse_position_valid() {
        assert_eq!(parse_position("TopLeft"), Some(OverlayPosition::TopLeft));
        assert_eq!(parse_position("BottomCenter"), Some(OverlayPosition::BottomCenter));
        assert_eq!(parse_position("Center"), Some(OverlayPosition::Center));
    }

    #[test]
    fn test_parse_position_invalid() {
        assert_eq!(parse_position("invalid"), None);
        assert_eq!(parse_position(""), None);
    }

    #[test]
    fn test_partial_config() {
        let toml_str = r#"
position = "TopRight"
opacity = 0.5
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.position.as_deref(), Some("TopRight"));
        assert_eq!(config.opacity, Some(0.5));
        assert!(config.scale.is_none());
        assert!(config.theme.is_none());

        let overlay_config = config.to_overlay_config();
        assert_eq!(overlay_config.position, OverlayPosition::TopRight);
        assert_eq!(overlay_config.opacity, 0.5);
        assert_eq!(overlay_config.scale, OverlayScale::Medium); // default
    }
}
