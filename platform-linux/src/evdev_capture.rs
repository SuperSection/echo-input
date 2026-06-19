use anyhow::{Context, Result};
use evdev::{Device, EventType, InputEvent as EvdevEvent, KeyCode};
use input_core::events::{InputEvent, KeyState, KeyboardEvent};
use input_core::traits::{CaptureFeatures, KeyboardCaptureProvider};
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;
use tracing::{debug, error, info, trace, warn};

use crate::keymap::scancode_to_key;

pub struct EvdevCapture {
    device_paths: Vec<PathBuf>,
    tx: broadcast::Sender<InputEvent>,
    running: bool,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl EvdevCapture {
    pub fn new() -> Result<Self> {
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            device_paths: Vec::new(),
            tx,
            running: false,
            task_handle: None,
        })
    }

    pub fn with_sender(tx: broadcast::Sender<InputEvent>) -> Self {
        Self {
            device_paths: Vec::new(),
            tx,
            running: false,
            task_handle: None,
        }
    }

    pub fn from_device(path: &Path) -> Result<Self> {
        let device = Device::open(path)
            .with_context(|| format!("Failed to open device: {}", path.display()))?;
        device
            .set_nonblocking(true)
            .with_context(|| format!("Failed to set non-blocking on {}", path.display()))?;

        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            device_paths: vec![path.to_path_buf()],
            tx,
            running: false,
            task_handle: None,
        })
    }

    pub fn from_device_with_sender(
        path: &Path,
        tx: broadcast::Sender<InputEvent>,
    ) -> Result<Self> {
        let device = Device::open(path)
            .with_context(|| format!("Failed to open device: {}", path.display()))?;
        device
            .set_nonblocking(true)
            .with_context(|| format!("Failed to set non-blocking on {}", path.display()))?;

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
        let mut skipped_no_keys = 0u32;

        let available: Vec<_> = evdev::enumerate().collect();
        let scanned = available.len() as u32;

        for (path, device) in available {
            if let Some(keys) = device.supported_keys() {
                let has_letters = (30..=44).any(|code| keys.contains(KeyCode::new(code)));
                let has_modifiers = keys.contains(KeyCode::KEY_LEFTCTRL)
                    || keys.contains(KeyCode::KEY_LEFTSHIFT)
                    || keys.contains(KeyCode::KEY_LEFTALT);

                if has_letters || has_modifiers {
                    debug!(device = %path.display(), "Keyboard device found");
                    devices.push(path);
                } else {
                    skipped_no_keys += 1;
                }
            } else {
                skipped_no_keys += 1;
            }
        }

        info!(
            scanned,
            keyboards = devices.len(),
            skipped_non_keyboard = skipped_no_keys,
            "Device discovery complete"
        );

        if devices.is_empty() {
            warn!(
                "No keyboard devices found. Check permissions on /dev/input/event*. \
                 Try: sudo usermod -aG input $USER"
            );
        }

        devices
    }

    fn spawn_capture(&mut self) -> Result<()> {
        let paths = if self.device_paths.is_empty() {
            debug!("No explicit device paths — running auto-discovery");
            Self::discover_devices()
        } else {
            debug!(
                count = self.device_paths.len(),
                "Using explicit device paths"
            );
            self.device_paths.clone()
        };

        if paths.is_empty() {
            anyhow::bail!(
                "No keyboard devices found. Check that you have permission \
                 to read /dev/input/event* devices. Try: sudo usermod -aG input $USER"
            );
        }

        let tx = self.tx.clone();
        let subscriber_count = tx.receiver_count();
        debug!(
            devices = paths.len(),
            subscribers = subscriber_count,
            "Spawning capture thread"
        );

        let handle = tokio::task::spawn_blocking(move || {
            Self::capture_loop(paths, tx);
        });

        self.task_handle = Some(handle);
        Ok(())
    }

    /// Blocking capture loop - runs on a dedicated thread.
    fn capture_loop(paths: Vec<PathBuf>, tx: broadcast::Sender<InputEvent>) {
        debug!(thread_id = ?std::thread::current().id(), "Capture thread spawned");

        let mut devices: Vec<(PathBuf, Device)> = Vec::new();
        for path in &paths {
            match Device::open(path) {
                Ok(device) => {
                    if let Err(e) = device.set_nonblocking(true) {
                        warn!(
                            device = %path.display(),
                            error = %e,
                            "Failed to set non-blocking mode"
                        );
                    }
                    debug!(device = %path.display(), "Opened keyboard device");
                    devices.push((path.clone(), device));
                }
                Err(e) => {
                    warn!(device = %path.display(), error = %e, "Failed to open device");
                }
            }
        }

        if devices.is_empty() {
            error!("No keyboard devices could be opened — capture thread exiting");
            return;
        }

        info!(devices = devices.len(), "Capture loop started");

        loop {
            let mut had_events = false;

            for (path, device) in &mut devices {
                match device.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            had_events = true;

                            let ev_type = ev.event_type();
                            let scancode = ev.code();
                            let value = ev.value();
                            let key_name = crate::keymap::scancode_to_key(scancode as u32);

                            trace!(
                                device = %path.display(),
                                event_type = ?ev_type,
                                scancode,
                                value,
                                key = ?key_name,
                                "evdev event"
                            );

                            if let Err(e) = Self::process_evdev_event(&ev, &tx) {
                                warn!(
                                    device = %path.display(),
                                    error = %e,
                                    "Failed to process event"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            warn!(
                                device = %path.display(),
                                error = %e,
                                error_kind = ?e.kind(),
                                "fetch_events error"
                            );
                        }
                    }
                }
            }

            if had_events {
                std::thread::sleep(std::time::Duration::from_micros(100));
            } else {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    fn process_evdev_event(
        ev: &EvdevEvent,
        tx: &broadcast::Sender<InputEvent>,
    ) -> Result<()> {
        if ev.event_type() != EventType::KEY {
            return Ok(());
        }

        let scancode = ev.code();
        let value = ev.value();

        if value == 2 {
            return Ok(());
        }

        let key = scancode_to_key(scancode as u32);
        let state = if value == 1 {
            KeyState::Pressed
        } else {
            KeyState::Released
        };

        info!(
            key = ?key,
            state = ?state,
            "EvdevCapture received {:?}",
            key
        );

        let event = InputEvent::Keyboard(KeyboardEvent {
            key,
            state,
            timestamp: std::time::SystemTime::now(),
            native_code: scancode as u32,
        });

        match tx.send(event) {
            Ok(received) => {
                trace!(
                    scancode,
                    key = ?key,
                    state = ?state,
                    receivers = received,
                    "Event sent"
                );
            }
            Err(e) => {
                warn!(
                    scancode,
                    key = ?key,
                    state = ?state,
                    error = %e,
                    "No active receivers — event dropped"
                );
            }
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl KeyboardCaptureProvider for EvdevCapture {
    async fn start(&mut self) -> Result<()> {
        if self.running {
            debug!("EvdevCapture.start() called while already running — ignoring");
            return Ok(());
        }

        info!("Starting keyboard capture");
        self.spawn_capture()?;
        self.running = true;
        info!("Keyboard capture started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if !self.running {
            debug!("EvdevCapture.stop() called but not running");
            return Ok(());
        }

        if let Some(handle) = self.task_handle.take() {
            debug!("Aborting capture thread");
            handle.abort();
            match handle.await {
                Ok(()) => debug!("Capture thread exited"),
                Err(e) if e.is_cancelled() => debug!("Capture thread cancelled"),
                Err(e) => warn!(error = %e, "Capture thread panicked"),
            }
        }

        self.running = false;
        info!("Keyboard capture stopped");
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
            warn!("EvdevCapture dropped while capture thread still running");
            handle.abort();
        }
    }
}
