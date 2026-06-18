use crate::keys::VirtualKey;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// State of a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyState {
    Pressed,
    Released,
}

/// Modifier key state bitmask.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModifierState {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl ModifierState {
    /// Returns true if no modifiers are active.
    pub fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.super_key
    }

    /// Returns the number of active modifier keys.
    pub fn count(&self) -> u8 {
        [self.ctrl, self.alt, self.shift, self.super_key]
            .iter()
            .filter(|&&b| b)
            .count() as u8
    }

    /// Build display prefix like "⌃⌥⇧⌘" for standard mode.
    pub fn symbol_prefix(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("⌃ ");
        }
        if self.alt {
            s.push_str("⌥ ");
        }
        if self.shift {
            s.push_str("⇧ ");
        }
        if self.super_key {
            s.push_str("⌘ ");
        }
        s
    }
}

impl std::fmt::Display for ModifierState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.super_key {
            parts.push("Super");
        }
        write!(f, "{}", parts.join(" + "))
    }
}

/// A raw keyboard event from the capture provider.
#[derive(Debug, Clone)]
pub struct KeyboardEvent {
    pub key: VirtualKey,
    pub state: KeyState,
    pub timestamp: SystemTime,
    /// Raw platform scancode (evdev code on Linux, virtual key code on Windows).
    pub native_code: u32,
}

/// A grouped keyboard shortcut ready for display.
///
/// This represents a completed shortcut like "Ctrl + Shift + P"
/// rather than individual key events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutCombo {
    pub modifiers: ModifierState,
    pub key: Option<VirtualKey>,
    /// Human-readable display string: "Ctrl + Shift + P"
    pub display: String,
}

impl ShortcutCombo {
    /// Create a new shortcut combo from modifiers and key.
    pub fn new(modifiers: ModifierState, key: Option<VirtualKey>) -> Self {
        let display = Self::build_display(&modifiers, key.as_ref());
        Self {
            modifiers,
            key,
            display,
        }
    }

    fn build_display(mods: &ModifierState, key: Option<&VirtualKey>) -> String {
        let mut parts = Vec::new();
        if mods.ctrl {
            parts.push("Ctrl".to_string());
        }
        if mods.alt {
            parts.push("Alt".to_string());
        }
        if mods.shift {
            parts.push("Shift".to_string());
        }
        if mods.super_key {
            parts.push("Super".to_string());
        }
        if let Some(k) = key {
            parts.push(k.label());
        }
        parts.join(" + ")
    }

    /// Returns true if this is a modifier-only combo (no non-modifier key).
    pub fn is_modifier_only(&self) -> bool {
        self.key.is_none()
    }
}

impl std::fmt::Display for ShortcutCombo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display)
    }
}

/// Mouse button identifier for future mouse support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Unknown(u32),
}

/// A mouse event for future mouse visualization support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MouseEvent {
    Button {
        button: MouseButton,
        state: KeyState,
        timestamp: SystemTime,
    },
    Scroll {
        direction: ScrollDirection,
        amount: i32,
        timestamp: SystemTime,
    },
}

/// Scroll direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Unified input event from any capture provider.
#[derive(Debug, Clone)]
pub enum InputEvent {
    Keyboard(KeyboardEvent),
    Mouse(MouseEvent),
}

/// A processed event ready for overlay display.
#[derive(Debug, Clone)]
pub enum ProcessedEvent {
    /// A completed shortcut combo (grouped mode).
    Shortcut(ShortcutCombo),
    /// Modifier state changed (for tracking held modifiers).
    ModifierChange(ModifierState),
    /// A single key event (raw mode).
    RawKey(KeyboardEvent),
}

/// Application context info for future app-aware shortcuts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppContext {
    /// Application name (e.g., "Code", "Firefox").
    pub name: String,
    /// Window title if available.
    pub title: Option<String>,
    /// Desktop entry / WM_CLASS if available.
    pub class: Option<String>,
}

/// An enriched shortcut with optional app context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedShortcut {
    pub shortcut: ShortcutCombo,
    pub app_context: Option<AppContext>,
    pub description: Option<String>,
}
