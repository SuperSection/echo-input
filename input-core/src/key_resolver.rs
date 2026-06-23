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

/// Non-Linux stub — no xkbcommon available.
#[cfg(not(target_os = "linux"))]
pub struct KeyResolver;

#[cfg(not(target_os = "linux"))]
impl Default for KeyResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "linux"))]
impl KeyResolver {
    pub fn new() -> Self {
        Self
    }

    pub fn resolve(&self, _key: &VirtualKey, _modifiers: &ModifierState) -> Option<String> {
        None
    }

    pub fn is_printable(&self, _key: &VirtualKey, _modifiers: &ModifierState) -> bool {
        false
    }
}

#[cfg(test)]
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
