use serde::{Deserialize, Serialize};
use std::fmt;

/// Platform-independent virtual key representation.
///
/// This enum maps physical key positions to logical names using
/// US QWERTY labels. Display output should use the user's actual
/// keyboard layout via XKB translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum VirtualKey {
    // Letters
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,

    // Numbers
    Key0, Key1, Key2, Key3, Key4, Key5,
    Key6, Key7, Key8, Key9,

    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8,
    F9, F10, F11, F12, F13, F14, F15, F16,
    F17, F18, F19, F20, F21, F22, F23, F24,

    // Modifiers
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    SuperLeft,
    SuperRight,
    Meta,

    // Navigation & editing
    Tab,
    Escape,
    Space,
    Enter,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    PrintScreen,
    ScrollLock,
    Pause,
    CapsLock,

    // Punctuation & symbols
    Minus,
    Equal,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Backtick,

    // Numpad
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    NumpadEnter,
    NumpadLock,

    // Media
    MediaPlay,
    MediaPause,
    MediaStop,
    MediaNext,
    MediaPrev,
    VolumeUp,
    VolumeDown,
    Mute,

    // Browser
    BrowserBack,
    BrowserForward,
    BrowserRefresh,

    // Unknown (carries raw scancode)
    Unknown(u32),
}

impl VirtualKey {
    /// Returns the display label for this key.
    pub fn label(&self) -> String {
        match self {
            Self::A => "A".into(),
            Self::B => "B".into(),
            Self::C => "C".into(),
            Self::D => "D".into(),
            Self::E => "E".into(),
            Self::F => "F".into(),
            Self::G => "G".into(),
            Self::H => "H".into(),
            Self::I => "I".into(),
            Self::J => "J".into(),
            Self::K => "K".into(),
            Self::L => "L".into(),
            Self::M => "M".into(),
            Self::N => "N".into(),
            Self::O => "O".into(),
            Self::P => "P".into(),
            Self::Q => "Q".into(),
            Self::R => "R".into(),
            Self::S => "S".into(),
            Self::T => "T".into(),
            Self::U => "U".into(),
            Self::V => "V".into(),
            Self::W => "W".into(),
            Self::X => "X".into(),
            Self::Y => "Y".into(),
            Self::Z => "Z".into(),

            Self::Key0 => "0".into(),
            Self::Key1 => "1".into(),
            Self::Key2 => "2".into(),
            Self::Key3 => "3".into(),
            Self::Key4 => "4".into(),
            Self::Key5 => "5".into(),
            Self::Key6 => "6".into(),
            Self::Key7 => "7".into(),
            Self::Key8 => "8".into(),
            Self::Key9 => "9".into(),

            Self::F1 => "F1".into(),
            Self::F2 => "F2".into(),
            Self::F3 => "F3".into(),
            Self::F4 => "F4".into(),
            Self::F5 => "F5".into(),
            Self::F6 => "F6".into(),
            Self::F7 => "F7".into(),
            Self::F8 => "F8".into(),
            Self::F9 => "F9".into(),
            Self::F10 => "F10".into(),
            Self::F11 => "F11".into(),
            Self::F12 => "F12".into(),
            Self::F13 => "F13".into(),
            Self::F14 => "F14".into(),
            Self::F15 => "F15".into(),
            Self::F16 => "F16".into(),
            Self::F17 => "F17".into(),
            Self::F18 => "F18".into(),
            Self::F19 => "F19".into(),
            Self::F20 => "F20".into(),
            Self::F21 => "F21".into(),
            Self::F22 => "F22".into(),
            Self::F23 => "F23".into(),
            Self::F24 => "F24".into(),

            Self::ShiftLeft | Self::ShiftRight => "Shift".into(),
            Self::ControlLeft | Self::ControlRight => "Ctrl".into(),
            Self::AltLeft | Self::AltRight => "Alt".into(),
            Self::SuperLeft | Self::SuperRight => "Super".into(),
            Self::Meta => "Meta".into(),

            Self::Tab => "Tab".into(),
            Self::Escape => "Esc".into(),
            Self::Space => "Space".into(),
            Self::Enter => "Enter".into(),
            Self::Backspace => "Backspace".into(),
            Self::Delete => "Del".into(),
            Self::Insert => "Ins".into(),
            Self::Home => "Home".into(),
            Self::End => "End".into(),
            Self::PageUp => "PgUp".into(),
            Self::PageDown => "PgDn".into(),
            Self::Up => "Up".into(),
            Self::Down => "Down".into(),
            Self::Left => "Left".into(),
            Self::Right => "Right".into(),
            Self::PrintScreen => "PrtSc".into(),
            Self::ScrollLock => "ScrLk".into(),
            Self::Pause => "Pause".into(),
            Self::CapsLock => "CapsLk".into(),

            Self::Minus => "-".into(),
            Self::Equal => "=".into(),
            Self::LeftBracket => "[".into(),
            Self::RightBracket => "]".into(),
            Self::Backslash => "\\".into(),
            Self::Semicolon => ";".into(),
            Self::Quote => "'".into(),
            Self::Comma => ",".into(),
            Self::Period => ".".into(),
            Self::Slash => "/".into(),
            Self::Backtick => "`".into(),

            Self::Numpad0 => "Num0".into(),
            Self::Numpad1 => "Num1".into(),
            Self::Numpad2 => "Num2".into(),
            Self::Numpad3 => "Num3".into(),
            Self::Numpad4 => "Num4".into(),
            Self::Numpad5 => "Num5".into(),
            Self::Numpad6 => "Num6".into(),
            Self::Numpad7 => "Num7".into(),
            Self::Numpad8 => "Num8".into(),
            Self::Numpad9 => "Num9".into(),
            Self::NumpadAdd => "Num+".into(),
            Self::NumpadSubtract => "Num-".into(),
            Self::NumpadMultiply => "Num*".into(),
            Self::NumpadDivide => "Num/".into(),
            Self::NumpadDecimal => "Num.".into(),
            Self::NumpadEnter => "NumEnter".into(),
            Self::NumpadLock => "NumLk".into(),

            Self::MediaPlay => "Play".into(),
            Self::MediaPause => "Pause".into(),
            Self::MediaStop => "Stop".into(),
            Self::MediaNext => "Next".into(),
            Self::MediaPrev => "Prev".into(),
            Self::VolumeUp => "Vol+".into(),
            Self::VolumeDown => "Vol-".into(),
            Self::Mute => "Mute".into(),

            Self::BrowserBack => "Back".into(),
            Self::BrowserForward => "Fwd".into(),
            Self::BrowserRefresh => "Refresh".into(),

            Self::Unknown(code) => format!("Key{}", code),
        }
    }

    /// Returns true if this key is a modifier key.
    pub fn is_modifier(&self) -> bool {
        matches!(
            self,
            Self::ShiftLeft
                | Self::ShiftRight
                | Self::ControlLeft
                | Self::ControlRight
                | Self::AltLeft
                | Self::AltRight
                | Self::SuperLeft
                | Self::SuperRight
                | Self::Meta
        )
    }
}

impl fmt::Display for VirtualKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}
