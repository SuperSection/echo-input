use crate::events::*;
use crate::key_resolver::KeyResolver;
use crate::keys::VirtualKey;
use crate::traits::{EventProcessor, ProcessorConfig};
use std::collections::VecDeque;
use std::time::Instant;

/// Default event processor implementation.
///
/// Handles modifier tracking, shortcut grouping, deduplication,
/// history management, and xkb-based character resolution.
pub struct DefaultEventProcessor {
    /// Current modifier state.
    modifiers: ModifierState,
    /// Keys currently held down (excluding modifiers).
    held_keys: Vec<VirtualKey>,
    /// Shortcut history (most recent first).
    history: VecDeque<ShortcutCombo>,
    /// Configuration.
    config: ProcessorConfig,
    /// Timestamp of last emitted event for deduplication.
    last_event_time: Option<Instant>,
    /// Last emitted shortcut for deduplication.
    last_shortcut: Option<ShortcutCombo>,
    /// XKB key resolver for translating keys to characters.
    resolver: KeyResolver,
}

impl DefaultEventProcessor {
    pub fn new(config: ProcessorConfig) -> Self {
        Self {
            modifiers: ModifierState::default(),
            held_keys: Vec::new(),
            history: VecDeque::with_capacity(config.history_length),
            config,
            last_event_time: None,
            last_shortcut: None,
            resolver: KeyResolver::new(),
        }
    }

    /// Create with a pre-configured key resolver (e.g., from Wayland compositor keymap).
    pub fn with_resolver(config: ProcessorConfig, resolver: KeyResolver) -> Self {
        Self {
            modifiers: ModifierState::default(),
            held_keys: Vec::new(),
            history: VecDeque::with_capacity(config.history_length),
            config,
            last_event_time: None,
            last_shortcut: None,
            resolver,
        }
    }

    /// Replace the key resolver (e.g., when compositor sends a new keymap).
    pub fn set_resolver(&mut self, resolver: KeyResolver) {
        self.resolver = resolver;
    }

    fn update_modifier(&mut self, key: VirtualKey, pressed: bool) {
        match key {
            VirtualKey::ControlLeft | VirtualKey::ControlRight => self.modifiers.ctrl = pressed,
            VirtualKey::AltLeft | VirtualKey::AltRight => self.modifiers.alt = pressed,
            VirtualKey::ShiftLeft | VirtualKey::ShiftRight => self.modifiers.shift = pressed,
            VirtualKey::SuperLeft | VirtualKey::SuperRight | VirtualKey::Meta => {
                self.modifiers.super_key = pressed
            }
            VirtualKey::CapsLock => {
                if pressed {
                    self.modifiers.capslock = !self.modifiers.capslock;
                }
            }
            VirtualKey::NumpadLock if pressed => {
                self.modifiers.numlock = !self.modifiers.numlock;
            }
            _ => {}
        }
    }

    fn is_modifier(key: VirtualKey) -> bool {
        matches!(
            key,
            VirtualKey::ControlLeft
                | VirtualKey::ControlRight
                | VirtualKey::AltLeft
                | VirtualKey::AltRight
                | VirtualKey::ShiftLeft
                | VirtualKey::ShiftRight
                | VirtualKey::SuperLeft
                | VirtualKey::SuperRight
                | VirtualKey::Meta
                | VirtualKey::CapsLock
                | VirtualKey::NumpadLock
        )
    }

    fn add_to_history(&mut self, combo: ShortcutCombo) {
        if let (Some(ref last), Some(last_time)) = (&self.last_shortcut, self.last_event_time) {
            if *last == combo && last_time.elapsed() < self.config.dedup_window {
                return;
            }
        }

        self.last_shortcut = Some(combo.clone());
        self.last_event_time = Some(Instant::now());

        self.history.push_front(combo);
        while self.history.len() > self.config.history_length {
            self.history.pop_back();
        }
    }

    /// Build a ShortcutCombo from current modifier state and an optional key.
    fn make_combo(&self, key: Option<VirtualKey>) -> ShortcutCombo {
        ShortcutCombo::new(self.modifiers, key)
    }

    /// Determine if a key+modifier combination should be treated as a character event.
    ///
    /// Character events display only the resolved symbol (e.g., "A", "!", "{").
    /// Shortcut events display modifier combinations (e.g., "Ctrl + C").
    fn is_character_event(&self, key: &VirtualKey) -> bool {
        // Ctrl + anything → always a shortcut
        if self.modifiers.ctrl {
            return false;
        }
        // Super + anything → always a shortcut
        if self.modifiers.super_key {
            return false;
        }

        // Use xkb to check if the key produces a printable character
        self.resolver.is_printable(key, &self.modifiers)
    }

    /// Resolve a key to its display representation.
    ///
    /// Uses xkb for character resolution, with fallbacks for numpad keys
    /// (navigation labels when NumLock is off) and non-printable keys.
    fn resolve_key(&self, key: &VirtualKey) -> String {
        if let Some(text) = self.resolver.resolve(key, &self.modifiers) {
            return text;
        }
        // xkb returned None — use fallback labels
        // Numpad without NumLock: show navigation actions
        if !self.modifiers.numlock {
            if let Some(nav_label) = key.numlock_off_label() {
                return nav_label;
            }
        }
        key.label()
    }
}

impl Default for DefaultEventProcessor {
    fn default() -> Self {
        Self::new(ProcessorConfig::default())
    }
}

impl EventProcessor for DefaultEventProcessor {
    fn process(&mut self, event: InputEvent) -> Vec<ProcessedEvent> {
        let mut out = Vec::new();

        match event {
            InputEvent::Keyboard(kbd_event) => {
                let key = kbd_event.key;
                let pressed = kbd_event.state == KeyState::Pressed;

                if Self::is_modifier(key) {
                    self.update_modifier(key, pressed);
                    out.push(ProcessedEvent::ModifierChange(self.modifiers));
                    return out;
                }

                if pressed {
                    if !self.held_keys.contains(&key) {
                        self.held_keys.push(key);
                    }

                    if self.config.group_shortcuts && !self.modifiers.is_empty() {
                        if self.is_character_event(&key) {
                            // Character event with modifiers: only show the resolved symbol
                            let text = self.resolve_key(&key);
                            let combo = ShortcutCombo::character(key, text);
                            out.push(ProcessedEvent::Shortcut(combo));
                        } else {
                            // Shortcut event: show modifier + key
                            let combo = self.make_combo(Some(key));
                            self.add_to_history(combo.clone());
                            out.push(ProcessedEvent::Shortcut(combo));
                        }
                    } else if self.config.group_shortcuts && self.modifiers.is_empty() {
                        // No modifiers — resolve the character
                        let text = self.resolve_key(&key);
                        let combo = ShortcutCombo::character(key, text);
                        out.push(ProcessedEvent::Shortcut(combo));
                    } else {
                        out.push(ProcessedEvent::RawKey(kbd_event));
                    }
                } else {
                    self.held_keys.retain(|k| *k != key);
                }
            }
            InputEvent::Mouse(_) => {}
        }

        out
    }

    fn modifier_state(&self) -> ModifierState {
        self.modifiers
    }

    fn current_compose(&self) -> Option<ShortcutCombo> {
        if self.modifiers.is_empty() && self.held_keys.is_empty() {
            None
        } else {
            let key = self.held_keys.first().copied();
            Some(self.make_combo(key))
        }
    }

    fn history(&self) -> &[ShortcutCombo] {
        self.history.as_slices().0
    }

    fn clear_history(&mut self) {
        self.history.clear();
        self.last_shortcut = None;
        self.last_event_time = None;
    }

    fn update_config(&mut self, config: ProcessorConfig) {
        self.config = config;
        while self.history.len() > self.config.history_length {
            self.history.pop_back();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn key_press(key: VirtualKey) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent {
            key,
            state: KeyState::Pressed,
            timestamp: SystemTime::now(),
            native_code: 0,
        })
    }

    fn key_release(key: VirtualKey) -> InputEvent {
        InputEvent::Keyboard(KeyboardEvent {
            key,
            state: KeyState::Released,
            timestamp: SystemTime::now(),
            native_code: 0,
        })
    }

    #[test]
    fn test_modifier_tracking() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        proc.process(key_press(VirtualKey::ControlLeft));
        assert!(proc.modifier_state().ctrl);
        assert!(!proc.modifier_state().shift);

        proc.process(key_press(VirtualKey::ShiftLeft));
        assert!(proc.modifier_state().ctrl);
        assert!(proc.modifier_state().shift);

        proc.process(key_release(VirtualKey::ControlLeft));
        assert!(!proc.modifier_state().ctrl);
        assert!(proc.modifier_state().shift);
    }

    #[test]
    fn test_ctrl_c_is_shortcut() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        proc.process(key_press(VirtualKey::ControlLeft));
        let events = proc.process(key_press(VirtualKey::C));

        assert_eq!(events.len(), 1);
        match &events[0] {
            ProcessedEvent::Shortcut(combo) => {
                assert!(combo.modifiers.ctrl);
                assert_eq!(combo.key, Some(VirtualKey::C));
                assert_eq!(combo.display, "Ctrl + C");
            }
            _ => panic!("Expected Shortcut event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_shift_a_is_character() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        proc.process(key_press(VirtualKey::ShiftLeft));
        let events = proc.process(key_press(VirtualKey::A));

        assert_eq!(events.len(), 1);
        match &events[0] {
            ProcessedEvent::Shortcut(combo) => {
                assert_eq!(combo.resolved_text.as_deref(), Some("A"));
                assert_eq!(combo.key, Some(VirtualKey::A));
            }
            _ => panic!("Expected Shortcut event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_a_is_character() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        let events = proc.process(key_press(VirtualKey::A));

        assert_eq!(events.len(), 1);
        match &events[0] {
            ProcessedEvent::Shortcut(combo) => {
                assert_eq!(combo.resolved_text.as_deref(), Some("a"));
                assert_eq!(combo.key, Some(VirtualKey::A));
            }
            _ => panic!("Expected Shortcut event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_capslock_a_is_character_uppercase() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        proc.process(key_press(VirtualKey::CapsLock));
        let events = proc.process(key_press(VirtualKey::A));

        assert_eq!(events.len(), 1);
        match &events[0] {
            ProcessedEvent::Shortcut(combo) => {
                assert_eq!(combo.resolved_text.as_deref(), Some("A"));
                assert_eq!(combo.key, Some(VirtualKey::A));
            }
            _ => panic!("Expected Shortcut event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_capslock_shift_a_is_character_lowercase() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        proc.process(key_press(VirtualKey::CapsLock));
        proc.process(key_press(VirtualKey::ShiftLeft));
        let events = proc.process(key_press(VirtualKey::A));

        assert_eq!(events.len(), 1);
        match &events[0] {
            ProcessedEvent::Shortcut(combo) => {
                assert_eq!(combo.resolved_text.as_deref(), Some("a"));
                assert_eq!(combo.key, Some(VirtualKey::A));
            }
            _ => panic!("Expected Shortcut event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_shift_1_is_character() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        proc.process(key_press(VirtualKey::ShiftLeft));
        let events = proc.process(key_press(VirtualKey::Key1));

        assert_eq!(events.len(), 1);
        match &events[0] {
            ProcessedEvent::Shortcut(combo) => {
                assert_eq!(combo.resolved_text.as_deref(), Some("!"));
                assert_eq!(combo.key, Some(VirtualKey::Key1));
            }
            _ => panic!("Expected Shortcut event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_shift_0_is_character() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        proc.process(key_press(VirtualKey::ShiftLeft));
        let events = proc.process(key_press(VirtualKey::Key0));

        assert_eq!(events.len(), 1);
        match &events[0] {
            ProcessedEvent::Shortcut(combo) => {
                assert_eq!(combo.resolved_text.as_deref(), Some(")"));
                assert_eq!(combo.key, Some(VirtualKey::Key0));
            }
            _ => panic!("Expected Shortcut event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_shift_leftbracket_is_character() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        proc.process(key_press(VirtualKey::ShiftLeft));
        let events = proc.process(key_press(VirtualKey::LeftBracket));

        assert_eq!(events.len(), 1);
        match &events[0] {
            ProcessedEvent::Shortcut(combo) => {
                assert_eq!(combo.resolved_text.as_deref(), Some("{"));
                assert_eq!(combo.key, Some(VirtualKey::LeftBracket));
            }
            _ => panic!("Expected Shortcut event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_history() {
        let mut proc = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            history_length: 3,
            dedup_window: Duration::from_millis(0),
        });

        proc.process(key_press(VirtualKey::ControlLeft));
        proc.process(key_press(VirtualKey::C));
        proc.process(key_release(VirtualKey::C));
        proc.process(key_release(VirtualKey::ControlLeft));

        proc.process(key_press(VirtualKey::ControlLeft));
        proc.process(key_press(VirtualKey::V));
        proc.process(key_release(VirtualKey::V));
        proc.process(key_release(VirtualKey::ControlLeft));

        let history = proc.history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].display, "Ctrl + V");
        assert_eq!(history[1].display, "Ctrl + C");
    }
}
