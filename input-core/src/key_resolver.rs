use crate::events::ModifierState;
use crate::keys::VirtualKey;
#[cfg(target_os = "linux")]
use xkbcommon::xkb;

/// Resolves VirtualKey + modifier state to the character produced.
///
/// Uses xkbcommon as the source of truth for keyboard layout translation.
/// On Linux, this loads the default system keymap and resolves keys through
/// the full XKB state machine, respecting the active keyboard layout.
#[cfg(target_os = "linux")]
pub struct KeyResolver {
    #[allow(dead_code)]
    context: xkb::Context,
    #[allow(dead_code)]
    keymap: xkb::Keymap,
    state: xkb::State,
    /// Keymap-dependent modifier indices (looked up once at init).
    mod_shift: xkb::ModIndex,
    mod_ctrl: xkb::ModIndex,
    mod_alt: xkb::ModIndex,
    mod_caps: xkb::ModIndex,
    mod_num: xkb::ModIndex,
    mod_super: xkb::ModIndex,
}

// SAFETY: KeyResolver is only used within DefaultEventProcessor which is
// accessed from a single tokio task. The xkb types are not shared across threads.
#[cfg(target_os = "linux")]
unsafe impl Send for KeyResolver {}
#[cfg(target_os = "linux")]
unsafe impl Sync for KeyResolver {}

#[cfg(target_os = "linux")]
impl Default for KeyResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
impl KeyResolver {
    /// Create a new resolver with the default system keymap.
    pub fn new() -> Self {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "",
            "",
            "",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .expect("Failed to load default XKB keymap");
        let state = xkb::State::new(&keymap);

        let mod_shift = keymap.mod_get_index(xkb::MOD_NAME_SHIFT);
        let mod_ctrl = keymap.mod_get_index(xkb::MOD_NAME_CTRL);
        let mod_alt = keymap.mod_get_index(xkb::MOD_NAME_ALT);
        let mod_caps = keymap.mod_get_index(xkb::MOD_NAME_CAPS);
        let mod_num = keymap.mod_get_index(xkb::MOD_NAME_NUM);
        let mod_super = keymap.mod_get_index(xkb::MOD_NAME_LOGO);

        Self {
            context,
            keymap,
            state,
            mod_shift,
            mod_ctrl,
            mod_alt,
            mod_caps,
            mod_num,
            mod_super,
        }
    }

    /// Create a resolver from an existing keymap (e.g., from Wayland compositor).
    pub fn from_keymap(keymap: xkb::Keymap) -> Self {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let state = xkb::State::new(&keymap);

        let mod_shift = keymap.mod_get_index(xkb::MOD_NAME_SHIFT);
        let mod_ctrl = keymap.mod_get_index(xkb::MOD_NAME_CTRL);
        let mod_alt = keymap.mod_get_index(xkb::MOD_NAME_ALT);
        let mod_caps = keymap.mod_get_index(xkb::MOD_NAME_CAPS);
        let mod_num = keymap.mod_get_index(xkb::MOD_NAME_NUM);
        let mod_super = keymap.mod_get_index(xkb::MOD_NAME_LOGO);

        Self {
            context,
            keymap,
            state,
            mod_shift,
            mod_ctrl,
            mod_alt,
            mod_caps,
            mod_num,
            mod_super,
        }
    }

    /// Replace the keymap (e.g., when Wayland compositor sends a new one).
    pub fn set_keymap(&mut self, keymap: xkb::Keymap) {
        self.mod_shift = keymap.mod_get_index(xkb::MOD_NAME_SHIFT);
        self.mod_ctrl = keymap.mod_get_index(xkb::MOD_NAME_CTRL);
        self.mod_alt = keymap.mod_get_index(xkb::MOD_NAME_ALT);
        self.mod_caps = keymap.mod_get_index(xkb::MOD_NAME_CAPS);
        self.mod_num = keymap.mod_get_index(xkb::MOD_NAME_NUM);
        self.mod_super = keymap.mod_get_index(xkb::MOD_NAME_LOGO);
        self.state = xkb::State::new(&keymap);
    }

    /// Resolve a VirtualKey + modifier state to the UTF-8 character it produces.
    ///
    /// Returns `Some(char_string)` for printable characters, `None` for
    /// non-printable keys (F-keys, navigation, numpad-without-numlock, etc.).
    pub fn resolve(&self, key: &VirtualKey, modifiers: &ModifierState) -> Option<String> {
        let evdev_code = key.evdev_code();
        if evdev_code == 0 {
            return None;
        }

        // Numpad keys: when NumLock is off, these produce navigation actions
        // (Ins, Down, PgDn, etc.) which are non-printable. Let the caller
        // handle the display via `numlock_off_label()`.
        if !modifiers.numlock && key.is_numpad_digit() {
            return None;
        }

        // Space: xkb returns " " but we want to show "Space" in the overlay.
        if *key == VirtualKey::Space {
            return Some("Space".into());
        }

        // XKB keycodes = evdev keycodes + 8
        let xkb_keycode = xkb::Keycode::new(evdev_code + 8);

        // Create a fresh state and apply modifier masks using keymap indices
        let mut state = self.state.clone();

        let mut mods_depressed = 0u32;
        let mut mods_locked = 0u32;

        if modifiers.shift {
            mods_depressed |= 1u32 << self.mod_shift;
        }
        if modifiers.ctrl {
            mods_depressed |= 1u32 << self.mod_ctrl;
        }
        if modifiers.alt {
            mods_depressed |= 1u32 << self.mod_alt;
        }
        if modifiers.super_key {
            mods_depressed |= 1u32 << self.mod_super;
        }
        if modifiers.capslock {
            mods_locked |= 1u32 << self.mod_caps;
        }
        if modifiers.numlock {
            mods_locked |= 1u32 << self.mod_num;
        }

        state.update_mask(mods_depressed, 0, mods_locked, 0, 0, 0);

        // Get the UTF-8 string for this key
        let utf8 = state.key_get_utf8(xkb_keycode);

        if !utf8.is_empty() && utf8.len() <= 6 {
            let ch = utf8.chars().next()?;
            if ch.is_control() && ch != '\t' {
                return None;
            }
            Some(utf8)
        } else {
            None
        }
    }

    /// Check if the key produces a printable character given the modifier state.
    pub fn is_printable(&self, key: &VirtualKey, modifiers: &ModifierState) -> bool {
        self.resolve(key, modifiers).is_some()
    }
}

// ── Windows KeyResolver ────────────────────────────────────────

#[cfg(target_os = "windows")]
pub struct KeyResolver {
    cached_layout: std::sync::atomic::AtomicPtr<std::ffi::c_void>,
}

#[cfg(target_os = "windows")]
unsafe impl Send for KeyResolver {}
#[cfg(target_os = "windows")]
unsafe impl Sync for KeyResolver {}

#[cfg(target_os = "windows")]
impl Default for KeyResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
impl KeyResolver {
    pub fn new() -> Self {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

        let layout = unsafe { GetKeyboardLayout(0) };
        Self {
            cached_layout: std::sync::atomic::AtomicPtr::new(layout as *mut _),
        }
    }

    pub fn resolve(&self, key: &VirtualKey, modifiers: &ModifierState) -> Option<String> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

        let vk = key_to_windows_vk(key);
        if vk == 0 {
            return None;
        }

        // Numpad keys without NumLock produce navigation keys — let caller handle
        if !modifiers.numlock && key.is_numpad_digit() {
            return None;
        }

        // Space: show label
        if *key == VirtualKey::Space {
            return Some("Space".into());
        }

        let layout = self
            .cached_layout
            .load(std::sync::atomic::Ordering::Relaxed);

        // Build keyboard state from modifier flags
        let mut state = [0u8; 256];
        if modifiers.shift {
            state[VK_SHIFT as usize] = 0x80;
        }
        if modifiers.ctrl {
            state[VK_CONTROL as usize] = 0x80;
        }
        if modifiers.alt {
            state[VK_MENU as usize] = 0x80;
        }
        if modifiers.capslock {
            state[VK_CAPITAL as usize] |= 0x01;
        }
        if modifiers.numlock {
            state[VK_NUMLOCK as usize] |= 0x01;
        }

        let scan_code = ((vk as u32 & 0xFF) << 16);
        let mut buf = [0u16; 4];

        let result = unsafe {
            ToUnicodeEx(
                vk as u32,
                scan_code,
                state.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i32,
                0,
                layout,
            )
        };

        if result == 1 {
            let ch = buf[0] as u8 as char;
            if !ch.is_control() {
                return Some(ch.to_string());
            }
        }

        // Fallback to US QWERTY label
        Some(key.label())
    }

    pub fn is_printable(&self, key: &VirtualKey, modifiers: &ModifierState) -> bool {
        self.resolve(key, modifiers).is_some()
    }
}

/// Map VirtualKey to Windows VK_* code.
#[cfg(target_os = "windows")]
fn key_to_windows_vk(key: &VirtualKey) -> u16 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    match key {
        VirtualKey::A => VK_A,
        VirtualKey::B => VK_B,
        VirtualKey::C => VK_C,
        VirtualKey::D => VK_D,
        VirtualKey::E => VK_E,
        VirtualKey::F => VK_F,
        VirtualKey::G => VK_G,
        VirtualKey::H => VK_H,
        VirtualKey::I => VK_I,
        VirtualKey::J => VK_J,
        VirtualKey::K => VK_K,
        VirtualKey::L => VK_L,
        VirtualKey::M => VK_M,
        VirtualKey::N => VK_N,
        VirtualKey::O => VK_O,
        VirtualKey::P => VK_P,
        VirtualKey::Q => VK_Q,
        VirtualKey::R => VK_R,
        VirtualKey::S => VK_S,
        VirtualKey::T => VK_T,
        VirtualKey::U => VK_U,
        VirtualKey::V => VK_V,
        VirtualKey::W => VK_W,
        VirtualKey::X => VK_X,
        VirtualKey::Y => VK_Y,
        VirtualKey::Z => VK_Z,
        VirtualKey::Key0 => VK_0,
        VirtualKey::Key1 => VK_1,
        VirtualKey::Key2 => VK_2,
        VirtualKey::Key3 => VK_3,
        VirtualKey::Key4 => VK_4,
        VirtualKey::Key5 => VK_5,
        VirtualKey::Key6 => VK_6,
        VirtualKey::Key7 => VK_7,
        VirtualKey::Key8 => VK_8,
        VirtualKey::Key9 => VK_9,
        VirtualKey::F1 => VK_F1,
        VirtualKey::F2 => VK_F2,
        VirtualKey::F3 => VK_F3,
        VirtualKey::F4 => VK_F4,
        VirtualKey::F5 => VK_F5,
        VirtualKey::F6 => VK_F6,
        VirtualKey::F7 => VK_F7,
        VirtualKey::F8 => VK_F8,
        VirtualKey::F9 => VK_F9,
        VirtualKey::F10 => VK_F10,
        VirtualKey::F11 => VK_F11,
        VirtualKey::F12 => VK_F12,
        VirtualKey::ShiftLeft | VirtualKey::ShiftRight => VK_SHIFT,
        VirtualKey::ControlLeft | VirtualKey::ControlRight => VK_CONTROL,
        VirtualKey::AltLeft | VirtualKey::AltRight => VK_MENU,
        VirtualKey::SuperLeft | VirtualKey::SuperRight => VK_LWIN,
        VirtualKey::Tab => VK_TAB,
        VirtualKey::Escape => VK_ESCAPE,
        VirtualKey::Space => VK_SPACE,
        VirtualKey::Enter => VK_RETURN,
        VirtualKey::Backspace => VK_BACK,
        VirtualKey::Delete => VK_DELETE,
        VirtualKey::Insert => VK_INSERT,
        VirtualKey::Home => VK_HOME,
        VirtualKey::End => VK_END,
        VirtualKey::PageUp => VK_PRIOR,
        VirtualKey::PageDown => VK_NEXT,
        VirtualKey::Up => VK_UP,
        VirtualKey::Down => VK_DOWN,
        VirtualKey::Left => VK_LEFT,
        VirtualKey::Right => VK_RIGHT,
        VirtualKey::PrintScreen => VK_SNAPSHOT,
        VirtualKey::ScrollLock => VK_SCROLL,
        VirtualKey::Pause => VK_PAUSE,
        VirtualKey::CapsLock => VK_CAPITAL,
        VirtualKey::Minus => VK_OEM_MINUS,
        VirtualKey::Equal => VK_OEM_PLUS,
        VirtualKey::LeftBracket => VK_OEM_4,
        VirtualKey::RightBracket => VK_OEM_6,
        VirtualKey::Backslash => VK_OEM_5,
        VirtualKey::Semicolon => VK_OEM_1,
        VirtualKey::Quote => VK_OEM_7,
        VirtualKey::Comma => VK_OEM_COMMA,
        VirtualKey::Period => VK_OEM_PERIOD,
        VirtualKey::Slash => VK_OEM_2,
        VirtualKey::Backtick => VK_OEM_3,
        VirtualKey::Numpad0 => VK_NUMPAD0,
        VirtualKey::Numpad1 => VK_NUMPAD1,
        VirtualKey::Numpad2 => VK_NUMPAD2,
        VirtualKey::Numpad3 => VK_NUMPAD3,
        VirtualKey::Numpad4 => VK_NUMPAD4,
        VirtualKey::Numpad5 => VK_NUMPAD5,
        VirtualKey::Numpad6 => VK_NUMPAD6,
        VirtualKey::Numpad7 => VK_NUMPAD7,
        VirtualKey::Numpad8 => VK_NUMPAD8,
        VirtualKey::Numpad9 => VK_NUMPAD9,
        VirtualKey::NumpadAdd => VK_ADD,
        VirtualKey::NumpadSubtract => VK_SUBTRACT,
        VirtualKey::NumpadMultiply => VK_MULTIPLY,
        VirtualKey::NumpadDivide => VK_DIVIDE,
        VirtualKey::NumpadDecimal => VK_DECIMAL,
        VirtualKey::NumpadEnter => VK_RETURN,
        VirtualKey::NumpadLock => VK_NUMLOCK,
        VirtualKey::Mute => VK_VOLUME_MUTE,
        VirtualKey::VolumeDown => VK_VOLUME_DOWN,
        VirtualKey::VolumeUp => VK_VOLUME_UP,
        VirtualKey::MediaPlay => VK_MEDIA_PLAY_PAUSE,
        VirtualKey::MediaStop => VK_MEDIA_STOP,
        VirtualKey::MediaNext => VK_MEDIA_NEXT_TRACK,
        VirtualKey::MediaPrev => VK_MEDIA_PREV_TRACK,
        _ => 0,
    }
}

// ── macOS KeyResolver ──────────────────────────────────────────

#[cfg(target_os = "macos")]
pub struct KeyResolver {
    layout: std::sync::OnceLock<*const std::ffi::c_void>,
}

#[cfg(target_os = "macos")]
unsafe impl Send for KeyResolver {}
#[cfg(target_os = "macos")]
unsafe impl Sync for KeyResolver {}

#[cfg(target_os = "macos")]
impl Default for KeyResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl KeyResolver {
    pub fn new() -> Self {
        Self {
            layout: std::sync::OnceLock::new(),
        }
    }

    fn get_layout(&self) -> *const std::ffi::c_void {
        *self.layout.get_or_init(|| {
            extern "C" {
                fn TISCopyCurrentKeyboardInputSource() -> *const std::ffi::c_void;
                fn TISGetInputSourceProperty(
                    input_source: *const std::ffi::c_void,
                    property_key: *const std::ffi::c_void,
                ) -> *const std::ffi::c_void;
                static kTISPropertyUnicodeKeyLayoutData: *const std::ffi::c_void;
            }
            unsafe {
                let src = TISCopyCurrentKeyboardInputSource();
                if src.is_null() {
                    return std::ptr::null();
                }
                let data = TISGetInputSourceProperty(src, kTISPropertyUnicodeKeyLayoutData);
                if data.is_null() {
                    return std::ptr::null();
                }
                // Retain the layout data so it survives TIS source release
                extern "C" {
                    fn CFRetain(cf: *const std::ffi::c_void) -> *const std::ffi::c_void;
                }
                CFRetain(data)
            }
        })
    }

    pub fn resolve(&self, key: &VirtualKey, modifiers: &ModifierState) -> Option<String> {
        if !modifiers.numlock && key.is_numpad_digit() {
            return None;
        }

        if *key == VirtualKey::Space {
            return Some("Space".into());
        }

        let layout = self.get_layout();
        if layout.is_null() {
            return Some(key.label());
        }

        let mac_keycode = key_to_macos_keycode(key)?;
        let mut chars = [0u16; 4];

        let result = unsafe {
            uc_key_translate(
                layout as *const _,
                mac_keycode,
                modifiers,
                chars.as_mut_ptr(),
            )
        };

        if result == 1 {
            let ch = chars[0] as u8 as char;
            if !ch.is_control() {
                return Some(ch.to_string());
            }
        }

        Some(key.label())
    }

    pub fn is_printable(&self, key: &VirtualKey, modifiers: &ModifierState) -> bool {
        self.resolve(key, modifiers).is_some()
    }
}

/// Translate a key using UCKeyTranslate.
#[cfg(target_os = "macos")]
unsafe fn uc_key_translate(
    layout: *const std::ffi::c_void,
    keycode: u16,
    modifiers: &ModifierState,
    chars: *mut u16,
) -> i32 {
    extern "C" {
        fn UCKeyTranslate(
            key_layout_ptr: *const std::ffi::c_void,
            virtual_key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            key_data_info_option: u32,
            actual_string_length: *mut u64,
            max_string_length: u64,
            actual_string: *mut u16,
        ) -> i32;
    }

    let mut modifier_state: u32 = 0;
    if modifiers.shift {
        modifier_state |= 0x0200; // shiftKey
    }
    if modifiers.ctrl {
        modifier_state |= 0x0400; // controlKey
    }
    if modifiers.alt {
        modifier_state |= 0x0800; // optionKey
    }
    if modifiers.super_key {
        modifier_state |= 0x0100; // cmdKey
    }
    if modifiers.capslock {
        modifier_state |= 0x0020; // alphaLock
    }

    let mut len: u64 = 0;
    UCKeyTranslate(
        layout,
        keycode,
        0, // keyDown
        modifier_state,
        0, // keyboard type
        0, // no cache
        &mut len,
        4,
        chars,
    );
    len as i32
}

/// Map VirtualKey to macOS virtual keycode (kVK_*).
#[cfg(target_os = "macos")]
fn key_to_macos_keycode(key: &VirtualKey) -> Option<u16> {
    match key {
        VirtualKey::A => Some(0x00),
        VirtualKey::B => Some(0x0B),
        VirtualKey::C => Some(0x08),
        VirtualKey::D => Some(0x02),
        VirtualKey::E => Some(0x0E),
        VirtualKey::F => Some(0x03),
        VirtualKey::G => Some(0x05),
        VirtualKey::H => Some(0x04),
        VirtualKey::I => Some(0x22),
        VirtualKey::J => Some(0x26),
        VirtualKey::K => Some(0x28),
        VirtualKey::L => Some(0x25),
        VirtualKey::M => Some(0x2E),
        VirtualKey::N => Some(0x2D),
        VirtualKey::O => Some(0x1F),
        VirtualKey::P => Some(0x23),
        VirtualKey::Q => Some(0x0C),
        VirtualKey::R => Some(0x0F),
        VirtualKey::S => Some(0x01),
        VirtualKey::T => Some(0x11),
        VirtualKey::U => Some(0x20),
        VirtualKey::V => Some(0x09),
        VirtualKey::W => Some(0x0D),
        VirtualKey::X => Some(0x07),
        VirtualKey::Y => Some(0x10),
        VirtualKey::Z => Some(0x06),
        VirtualKey::Key0 => Some(0x1D),
        VirtualKey::Key1 => Some(0x12),
        VirtualKey::Key2 => Some(0x13),
        VirtualKey::Key3 => Some(0x14),
        VirtualKey::Key4 => Some(0x15),
        VirtualKey::Key5 => Some(0x17),
        VirtualKey::Key6 => Some(0x16),
        VirtualKey::Key7 => Some(0x1A),
        VirtualKey::Key8 => Some(0x1C),
        VirtualKey::Key9 => Some(0x19),
        VirtualKey::Minus => Some(0x1B),
        VirtualKey::Equal => Some(0x18),
        VirtualKey::LeftBracket => Some(0x21),
        VirtualKey::RightBracket => Some(0x1E),
        VirtualKey::Backslash => Some(0x2A),
        VirtualKey::Semicolon => Some(0x29),
        VirtualKey::Quote => Some(0x27),
        VirtualKey::Comma => Some(0x2B),
        VirtualKey::Period => Some(0x2F),
        VirtualKey::Slash => Some(0x2C),
        VirtualKey::Backtick => Some(0x32),
        VirtualKey::Numpad0 => Some(0x52),
        VirtualKey::Numpad1 => Some(0x53),
        VirtualKey::Numpad2 => Some(0x54),
        VirtualKey::Numpad3 => Some(0x55),
        VirtualKey::Numpad4 => Some(0x56),
        VirtualKey::Numpad5 => Some(0x57),
        VirtualKey::Numpad6 => Some(0x58),
        VirtualKey::Numpad7 => Some(0x59),
        VirtualKey::Numpad8 => Some(0x5B),
        VirtualKey::Numpad9 => Some(0x5C),
        VirtualKey::NumpadAdd => Some(0x45),
        VirtualKey::NumpadSubtract => Some(0x4E),
        VirtualKey::NumpadMultiply => Some(0x43),
        VirtualKey::NumpadDivide => Some(0x4B),
        VirtualKey::NumpadDecimal => Some(0x41),
        VirtualKey::NumpadEnter => Some(0x4C),
        _ => None,
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_a_no_modifiers() {
        let resolver = KeyResolver::new();
        let mods = ModifierState::default();
        assert_eq!(resolver.resolve(&VirtualKey::A, &mods), Some("a".into()));
    }

    #[test]
    fn test_resolve_a_shift() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            shift: true,
            ..Default::default()
        };
        assert_eq!(resolver.resolve(&VirtualKey::A, &mods), Some("A".into()));
    }

    #[test]
    fn test_resolve_a_capslock() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            capslock: true,
            ..Default::default()
        };
        assert_eq!(resolver.resolve(&VirtualKey::A, &mods), Some("A".into()));
    }

    #[test]
    fn test_resolve_a_capslock_shift() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            capslock: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(resolver.resolve(&VirtualKey::A, &mods), Some("a".into()));
    }

    #[test]
    fn test_resolve_1_no_modifiers() {
        let resolver = KeyResolver::new();
        let mods = ModifierState::default();
        assert_eq!(resolver.resolve(&VirtualKey::Key1, &mods), Some("1".into()));
    }

    #[test]
    fn test_resolve_1_shift() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            shift: true,
            ..Default::default()
        };
        assert_eq!(resolver.resolve(&VirtualKey::Key1, &mods), Some("!".into()));
    }

    #[test]
    fn test_resolve_0_shift() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            shift: true,
            ..Default::default()
        };
        assert_eq!(resolver.resolve(&VirtualKey::Key0, &mods), Some(")".into()));
    }

    #[test]
    fn test_resolve_leftbracket_shift() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            resolver.resolve(&VirtualKey::LeftBracket, &mods),
            Some("{".into())
        );
    }

    #[test]
    fn test_resolve_semicolon_shift() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            resolver.resolve(&VirtualKey::Semicolon, &mods),
            Some(":".into())
        );
    }

    #[test]
    fn test_resolve_slash_shift() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            resolver.resolve(&VirtualKey::Slash, &mods),
            Some("?".into())
        );
    }

    #[test]
    fn test_f1_not_printable() {
        let resolver = KeyResolver::new();
        let mods = ModifierState::default();
        assert_eq!(resolver.resolve(&VirtualKey::F1, &mods), None);
    }

    #[test]
    fn test_escape_not_printable() {
        let resolver = KeyResolver::new();
        let mods = ModifierState::default();
        assert_eq!(resolver.resolve(&VirtualKey::Escape, &mods), None);
    }

    #[test]
    fn test_ctrl_c_not_printable() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(resolver.resolve(&VirtualKey::C, &mods), None);
    }

    #[test]
    fn test_space_shows_space_label() {
        let resolver = KeyResolver::new();
        let mods = ModifierState::default();
        assert_eq!(
            resolver.resolve(&VirtualKey::Space, &mods),
            Some("Space".into())
        );
    }

    #[test]
    fn test_numpad_without_numlock_returns_none() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            numlock: false,
            ..Default::default()
        };
        assert_eq!(resolver.resolve(&VirtualKey::Numpad8, &mods), None);
        assert_eq!(resolver.resolve(&VirtualKey::Numpad7, &mods), None);
    }

    #[test]
    fn test_numpad_with_numlock_returns_digit() {
        let resolver = KeyResolver::new();
        let mods = ModifierState {
            numlock: true,
            ..Default::default()
        };
        assert_eq!(
            resolver.resolve(&VirtualKey::Numpad8, &mods),
            Some("8".into())
        );
        assert_eq!(
            resolver.resolve(&VirtualKey::Numpad7, &mods),
            Some("7".into())
        );
    }

    #[test]
    fn test_is_printable() {
        let resolver = KeyResolver::new();
        let mods = ModifierState::default();
        assert!(resolver.is_printable(&VirtualKey::A, &mods));
        assert!(resolver.is_printable(&VirtualKey::Key1, &mods));
        assert!(resolver.is_printable(&VirtualKey::Space, &mods));
        assert!(!resolver.is_printable(&VirtualKey::F1, &mods));
        assert!(!resolver.is_printable(&VirtualKey::Escape, &mods));
        assert!(!resolver.is_printable(&VirtualKey::Enter, &mods));
    }
}
