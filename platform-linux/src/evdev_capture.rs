use anyhow::{Context, Result};
use evdev::{Device, EventType, InputEvent as EvdevEvent, KeyCode};
use input_core::events::{InputEvent, KeyState, KeyboardEvent};
use input_core::traits::{CaptureFeatures, KeyboardCaptureProvider};
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::keymap::scancode_to_key;

/// Linux evdev-based keyboard capture provider.
///
/// Reads raw input events from `/dev/input/event*` devices.
/// Requires read permissions on input devices (user in `input` group
/// or custom udev rules).
pub struct EvdevCapture {
    /// Explicitly provided device paths (empty = auto-discover).
    device_paths: Vec<PathBuf>,
    /// Broadcast channel for input events.
    tx: broadcast::Sender<InputEvent>,
    /// Whether capture is running.
    running: bool,
    /// Handle to the capture task for cleanup.
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl EvdevCapture {
    /// Create a new evdev capture provider with auto-discovery.
    ///
    /// Creates its own internal broadcast channel. Use `with_sender()`
    /// to integrate with a `MessageBus`.
    pub fn new() -> Result<Self> {
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            device_paths: Vec::new(),
            tx,
            running: false,
            task_handle: None,
        })
    }

    /// Create a capture provider using an external broadcast sender.
    ///
    /// This integrates with the `MessageBus` — the bus owns the channel
    /// and the capture provider publishes events to it.
    pub fn with_sender(tx: broadcast::Sender<InputEvent>) -> Self {
        Self {
            device_paths: Vec::new(),
            tx,
            running: false,
            task_handle: None,
        }
    }

    /// Create capture for a specific device path.
    pub fn from_device(path: &Path) -> Result<Self> {
        // Validate device exists and is accessible
        Device::open(path)
            .with_context(|| format!("Failed to open device: {}", path.display()))?;

        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            device_paths: vec![path.to_path_buf()],
            tx,
            running: false,
            task_handle: None,
        })
    }

    /// Create capture for a specific device path with an external sender.
    pub fn from_device_with_sender(path: &Path, tx: broadcast::Sender<InputEvent>) -> Result<Self> {
        Device::open(path)
            .with_context(|| format!("Failed to open device: {}", path.display()))?;

        Ok(Self {
            device_paths: vec![path.to_path_buf()],
            tx,
            running: false,
            task_handle: None,
        })
    }

    /// Discover keyboard devices from /dev/input/.
    fn discover_devices() -> Vec<PathBuf> {
        let mut devices = Vec::new();

        // Read available devices
        let available: Vec<_> = evdev::enumerate().collect();

        for (path, device) in available {
            // Check if device supports key events
            if let Some(keys) = device.supported_keys() {
                // A keyboard supports at least the letter keys (KEY_A=30..KEY_Z=44)
                // or common modifier keys
                let has_letters = (30..=44).any(|code| keys.contains(KeyCode::new(code)));
                let has_modifiers = keys.contains(KeyCode::KEY_LEFTCTRL)
                    || keys.contains(KeyCode::KEY_LEFTSHIFT)
                    || keys.contains(KeyCode::KEY_LEFTALT);

                if has_letters || has_modifiers {
                    debug!("Found keyboard device: {}", path.display());
                    devices.push(path);
                }
            }
        }

        info!("Discovered {} keyboard device(s)", devices.len());
        devices
    }

    /// Spawn the capture loop on a tokio task.
    fn spawn_capture(&mut self) -> Result<()> {
        let paths = if self.device_paths.is_empty() {
            Self::discover_devices()
        } else {
            self.device_paths.clone()
        };

        if paths.is_empty() {
            anyhow::bail!(
                "No keyboard devices found. Check that you have permission \
                 to read /dev/input/event* devices. Try: sudo usermod -aG input $USER"
            );
        }

        let tx = self.tx.clone();

        let handle = tokio::task::spawn_blocking(move || {
            Self::capture_loop(paths, tx);
        });

        self.task_handle = Some(handle);
        Ok(())
    }

    /// Blocking capture loop - runs on a dedicated thread.
    fn capture_loop(paths: Vec<PathBuf>, tx: broadcast::Sender<InputEvent>) {
        // Open all devices
        let mut devices: Vec<(PathBuf, Device)> = Vec::new();
        for path in &paths {
            match Device::open(path) {
                Ok(device) => {
                    info!("Opened keyboard device: {}", path.display());
                    devices.push((path.clone(), device));
                }
                Err(e) => {
                    warn!("Failed to open {}: {}", path.display(), e);
                }
            }
        }

        if devices.is_empty() {
            error!("No keyboard devices could be opened");
            return;
        }

        info!("Capture loop started for {} device(s)", devices.len());

        loop {
            let mut had_events = false;

            for (path, device) in &mut devices {
                match device.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            had_events = true;
                            if let Err(e) = Self::process_evdev_event(&ev, &tx) {
                                debug!("Failed to process event from {}: {}", path.display(), e);
                            }
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            warn!("Error reading from {}: {}", path.display(), e);
                        }
                    }
                }
            }

            // Sleep longer if no events were available (idle)
            if had_events {
                std::thread::sleep(std::time::Duration::from_micros(100));
            } else {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    /// Process a single evdev event and broadcast it.
    fn process_evdev_event(
        ev: &EvdevEvent,
        tx: &broadcast::Sender<InputEvent>,
    ) -> Result<()> {
        if ev.event_type() != EventType::KEY {
            return Ok(());
        }

        let scancode = ev.code();
        let value = ev.value(); // 0=release, 1=press, 2=repeat

        // Ignore key repeats
        if value == 2 {
            return Ok(());
        }

        let key = scancode_to_key(scancode as u32);
        let state = if value == 1 {
            KeyState::Pressed
        } else {
            KeyState::Released
        };

        let event = InputEvent::Keyboard(KeyboardEvent {
            key,
            state,
            timestamp: std::time::SystemTime::now(),
            native_code: scancode as u32,
        });

        // Ignore send errors (no subscribers)
        let _ = tx.send(event);

        Ok(())
    }
}

#[async_trait::async_trait]
impl KeyboardCaptureProvider for EvdevCapture {
    async fn start(&mut self) -> Result<()> {
        if self.running {
            return Ok(());
        }

        self.spawn_capture()?;
        self.running = true;
        info!("EvdevCapture started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }

        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }

        self.running = false;
        info!("EvdevCapture stopped");
        Ok(())
    }

    fn subscribe(&self) -> broadcast::Receiver<InputEvent> {
        self.tx.subscribe()
    }

    fn features(&self) -> CaptureFeatures {
        CaptureFeatures {
            keyboard: true,
            mouse: false,
            scroll: false,
            gamepad: false,
            app_context: false,
        }
    }

    fn name(&self) -> &str {
        "evdev"
    }
}

impl Drop for EvdevCapture {
    fn drop(&mut self) {
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }
}
