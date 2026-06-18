use input_core::keys::VirtualKey;

/// Map Linux evdev scancode (KEY_*) to VirtualKey.
///
/// Based on linux/input-event-codes.h. The evdev scancode is the
/// `code` field from `input_event`.
pub fn scancode_to_key(scancode: u32) -> VirtualKey {
    match scancode {
        // Letters (KEY_A=30 through KEY_Z=44 are not contiguous!)
        // Actually in evdev: KEY_A=30, KEY_B=48, KEY_C=46, etc.
        // Let's use the correct mappings from input-event-codes.h
        30 => VirtualKey::A,
        48 => VirtualKey::B,
        46 => VirtualKey::C,
        32 => VirtualKey::D,
        18 => VirtualKey::E,
        33 => VirtualKey::F,
        34 => VirtualKey::G,
        35 => VirtualKey::H,
        23 => VirtualKey::I,
        36 => VirtualKey::J,
        37 => VirtualKey::K,
        38 => VirtualKey::L,
        50 => VirtualKey::M,
        49 => VirtualKey::N,
        24 => VirtualKey::O,
        25 => VirtualKey::P,
        16 => VirtualKey::Q,
        19 => VirtualKey::R,
        31 => VirtualKey::S,
        20 => VirtualKey::T,
        22 => VirtualKey::U,
        47 => VirtualKey::V,
        17 => VirtualKey::W,
        45 => VirtualKey::X,
        21 => VirtualKey::Y,
        44 => VirtualKey::Z,

        // Numbers
        11 => VirtualKey::Key0,
        2 => VirtualKey::Key1,
        3 => VirtualKey::Key2,
        4 => VirtualKey::Key3,
        5 => VirtualKey::Key4,
        6 => VirtualKey::Key5,
        7 => VirtualKey::Key6,
        8 => VirtualKey::Key7,
        9 => VirtualKey::Key8,
        10 => VirtualKey::Key9,

        // Function keys
        59 => VirtualKey::F1,   // KEY_F1
        60 => VirtualKey::F2,
        61 => VirtualKey::F3,
        62 => VirtualKey::F4,
        63 => VirtualKey::F5,
        64 => VirtualKey::F6,
        65 => VirtualKey::F7,
        66 => VirtualKey::F8,
        67 => VirtualKey::F9,
        68 => VirtualKey::F10,
        87 => VirtualKey::F11,  // KEY_F11
        88 => VirtualKey::F12,  // KEY_F12
        183 => VirtualKey::F13, // KEY_F13
        184 => VirtualKey::F14,
        185 => VirtualKey::F15,
        186 => VirtualKey::F16,
        187 => VirtualKey::F17,
        188 => VirtualKey::F18,
        189 => VirtualKey::F19,
        190 => VirtualKey::F20,
        191 => VirtualKey::F21,
        192 => VirtualKey::F22,
        193 => VirtualKey::F23,
        194 => VirtualKey::F24,

        // Modifiers
        29 => VirtualKey::ControlLeft,   // KEY_LEFTCTRL
        97 => VirtualKey::ControlRight,  // KEY_RIGHTCTRL
        42 => VirtualKey::ShiftLeft,     // KEY_LEFTSHIFT
        54 => VirtualKey::ShiftRight,    // KEY_RIGHTSHIFT
        56 => VirtualKey::AltLeft,       // KEY_LEFTALT
        100 => VirtualKey::AltRight,     // KEY_RIGHTALT
        125 => VirtualKey::SuperLeft,    // KEY_LEFTMETA
        126 => VirtualKey::SuperRight,   // KEY_RIGHTMETA

        // Navigation & editing
        15 => VirtualKey::Tab,          // KEY_TAB
        1 => VirtualKey::Escape,        // KEY_ESC
        57 => VirtualKey::Space,        // KEY_SPACE
        28 => VirtualKey::Enter,        // KEY_ENTER
        14 => VirtualKey::Backspace,    // KEY_BACKSPACE
        111 => VirtualKey::Delete,      // KEY_DELETE
        110 => VirtualKey::Insert,      // KEY_INSERT
        102 => VirtualKey::Home,        // KEY_HOME
        107 => VirtualKey::End,         // KEY_END
        104 => VirtualKey::PageUp,      // KEY_PAGEUP
        109 => VirtualKey::PageDown,    // KEY_PAGEDOWN
        103 => VirtualKey::Up,          // KEY_UP
        108 => VirtualKey::Down,        // KEY_DOWN
        105 => VirtualKey::Left,        // KEY_LEFT
        106 => VirtualKey::Right,       // KEY_RIGHT
        99 => VirtualKey::PrintScreen,  // KEY_SYSRQ
        70 => VirtualKey::ScrollLock,   // KEY_SCROLLLOCK
        119 => VirtualKey::Pause,       // KEY_PAUSE
        58 => VirtualKey::CapsLock,     // KEY_CAPSLOCK

        // Punctuation & symbols
        12 => VirtualKey::Minus,        // KEY_MINUS
        13 => VirtualKey::Equal,        // KEY_EQUAL
        26 => VirtualKey::LeftBracket,  // KEY_LEFTBRACE
        27 => VirtualKey::RightBracket, // KEY_RIGHTBRACE
        43 => VirtualKey::Backslash,    // KEY_BACKSLASH
        39 => VirtualKey::Semicolon,    // KEY_SEMICOLON
        40 => VirtualKey::Quote,        // KEY_APOSTROPHE
        51 => VirtualKey::Comma,        // KEY_COMMA
        52 => VirtualKey::Period,       // KEY_DOT
        53 => VirtualKey::Slash,        // KEY_SLASH
        41 => VirtualKey::Backtick,     // KEY_GRAVE

        // Numpad
        82 => VirtualKey::Numpad0,      // KEY_KP0
        79 => VirtualKey::Numpad1,      // KEY_KP1
        80 => VirtualKey::Numpad2,      // KEY_KP2
        81 => VirtualKey::Numpad3,      // KEY_KP3
        75 => VirtualKey::Numpad4,      // KEY_KP4
        76 => VirtualKey::Numpad5,      // KEY_KP5
        77 => VirtualKey::Numpad6,      // KEY_KP6
        71 => VirtualKey::Numpad7,      // KEY_KP7
        72 => VirtualKey::Numpad8,      // KEY_KP8
        73 => VirtualKey::Numpad9,      // KEY_KP9
        78 => VirtualKey::NumpadAdd,    // KEY_KPPLUS
        74 => VirtualKey::NumpadSubtract, // KEY_KPMINUS
        55 => VirtualKey::NumpadMultiply, // KEY_KPASTERISK
        98 => VirtualKey::NumpadDivide, // KEY_KPSLASH
        83 => VirtualKey::NumpadDecimal, // KEY_KPDOT
        96 => VirtualKey::NumpadEnter,  // KEY_KPENTER
        69 => VirtualKey::NumpadLock,   // KEY_NUMLOCK

        // Media
        164 => VirtualKey::MediaPlay,    // KEY_PLAYPAUSE
        166 => VirtualKey::MediaStop,    // KEY_STOPCD
        163 => VirtualKey::MediaNext,    // KEY_NEXTSONG
        165 => VirtualKey::MediaPrev,    // KEY_PREVIOUSSONG
        113 => VirtualKey::Mute,         // KEY_MUTE
        114 => VirtualKey::VolumeDown,   // KEY_VOLUMEDOWN
        115 => VirtualKey::VolumeUp,     // KEY_VOLUMEUP

        // Browser
        158 => VirtualKey::BrowserBack,      // KEY_BACK
        159 => VirtualKey::BrowserForward,   // KEY_FORWARD
        181 => VirtualKey::BrowserRefresh,   // KEY_REFRESH

        _ => VirtualKey::Unknown(scancode),
    }
}

/// Map VirtualKey back to evdev scancode.
pub fn key_to_scancode(key: VirtualKey) -> u32 {
    match key {
        VirtualKey::A => 30,
        VirtualKey::B => 48,
        VirtualKey::C => 46,
        VirtualKey::D => 32,
        VirtualKey::E => 18,
        VirtualKey::F => 33,
        VirtualKey::G => 34,
        VirtualKey::H => 35,
        VirtualKey::I => 23,
        VirtualKey::J => 36,
        VirtualKey::K => 37,
        VirtualKey::L => 38,
        VirtualKey::M => 50,
        VirtualKey::N => 49,
        VirtualKey::O => 24,
        VirtualKey::P => 25,
        VirtualKey::Q => 16,
        VirtualKey::R => 19,
        VirtualKey::S => 31,
        VirtualKey::T => 20,
        VirtualKey::U => 22,
        VirtualKey::V => 47,
        VirtualKey::W => 17,
        VirtualKey::X => 45,
        VirtualKey::Y => 21,
        VirtualKey::Z => 44,

        VirtualKey::Key0 => 11,
        VirtualKey::Key1 => 2,
        VirtualKey::Key2 => 3,
        VirtualKey::Key3 => 4,
        VirtualKey::Key4 => 5,
        VirtualKey::Key5 => 6,
        VirtualKey::Key6 => 7,
        VirtualKey::Key7 => 8,
        VirtualKey::Key8 => 9,
        VirtualKey::Key9 => 10,

        VirtualKey::F1 => 59,
        VirtualKey::F2 => 60,
        VirtualKey::F3 => 61,
        VirtualKey::F4 => 62,
        VirtualKey::F5 => 63,
        VirtualKey::F6 => 64,
        VirtualKey::F7 => 65,
        VirtualKey::F8 => 66,
        VirtualKey::F9 => 67,
        VirtualKey::F10 => 68,
        VirtualKey::F11 => 87,
        VirtualKey::F12 => 88,

        VirtualKey::ControlLeft => 29,
        VirtualKey::ControlRight => 97,
        VirtualKey::ShiftLeft => 42,
        VirtualKey::ShiftRight => 54,
        VirtualKey::AltLeft => 56,
        VirtualKey::AltRight => 100,
        VirtualKey::SuperLeft => 125,
        VirtualKey::SuperRight => 126,

        VirtualKey::Tab => 15,
        VirtualKey::Escape => 1,
        VirtualKey::Space => 57,
        VirtualKey::Enter => 28,
        VirtualKey::Backspace => 14,
        VirtualKey::Delete => 111,
        VirtualKey::Insert => 110,
        VirtualKey::Home => 102,
        VirtualKey::End => 107,
        VirtualKey::PageUp => 104,
        VirtualKey::PageDown => 109,
        VirtualKey::Up => 103,
        VirtualKey::Down => 108,
        VirtualKey::Left => 105,
        VirtualKey::Right => 106,

        VirtualKey::Minus => 12,
        VirtualKey::Equal => 13,
        VirtualKey::LeftBracket => 26,
        VirtualKey::RightBracket => 27,
        VirtualKey::Backslash => 43,
        VirtualKey::Semicolon => 39,
        VirtualKey::Quote => 40,
        VirtualKey::Comma => 51,
        VirtualKey::Period => 52,
        VirtualKey::Slash => 53,
        VirtualKey::Backtick => 41,

        VirtualKey::Numpad0 => 82,
        VirtualKey::Numpad1 => 79,
        VirtualKey::Numpad2 => 80,
        VirtualKey::Numpad3 => 81,
        VirtualKey::Numpad4 => 75,
        VirtualKey::Numpad5 => 76,
        VirtualKey::Numpad6 => 77,
        VirtualKey::Numpad7 => 71,
        VirtualKey::Numpad8 => 72,
        VirtualKey::Numpad9 => 73,
        VirtualKey::NumpadAdd => 78,
        VirtualKey::NumpadSubtract => 74,
        VirtualKey::NumpadMultiply => 55,
        VirtualKey::NumpadDivide => 98,
        VirtualKey::NumpadDecimal => 83,
        VirtualKey::NumpadEnter => 96,
        VirtualKey::NumpadLock => 69,

        VirtualKey::MediaPlay => 164,
        VirtualKey::MediaStop => 166,
        VirtualKey::MediaNext => 163,
        VirtualKey::MediaPrev => 165,
        VirtualKey::Mute => 113,
        VirtualKey::VolumeDown => 114,
        VirtualKey::VolumeUp => 115,

        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_common_keys() {
        let keys = [
            VirtualKey::A,
            VirtualKey::Z,
            VirtualKey::Key0,
            VirtualKey::Key9,
            VirtualKey::F1,
            VirtualKey::F12,
            VirtualKey::ControlLeft,
            VirtualKey::ShiftRight,
            VirtualKey::Enter,
            VirtualKey::Space,
            VirtualKey::Escape,
        ];

        for key in keys {
            let sc = key_to_scancode(key);
            let recovered = scancode_to_key(sc);
            assert_eq!(key, recovered, "Roundtrip failed for {:?}", key);
        }
    }

    #[test]
    fn test_unknown_scancode() {
        let key = scancode_to_key(9999);
        assert_eq!(key, VirtualKey::Unknown(9999));
    }
}
