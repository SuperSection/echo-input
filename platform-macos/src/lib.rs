#![allow(unexpected_cfgs)]

pub mod keymap;
pub mod overlay;

use anyhow::Result;
use input_core::events::InputEvent;
use input_core::ipc::MessageBus;
use platform::capture::{CaptureFeatures, KeyboardCaptureFactory, KeyboardCaptureProvider};
use platform::overlay::OverlayRendererFactory;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

/// macOS keyboard capture provider using CGEventTap.
///
/// Creates a Quartz event tap that monitors all keyboard events system-wide.
/// Requires Accessibility permissions in System Preferences.
pub struct MacosCapture {
    tx: broadcast::Sender<InputEvent>,
    running: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle>,
}

struct JoinHandle {
    thread: std::thread::JoinHandle<()>,
}

impl MacosCapture {
    pub fn new() -> Result<Self> {
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            tx,
            running: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            handle: None,
        })
    }

    pub fn with_shutdown(shutdown: Arc<AtomicBool>) -> Result<Self> {
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            tx,
            running: Arc::new(AtomicBool::new(false)),
            shutdown,
            handle: None,
        })
    }
}

#[async_trait::async_trait]
impl KeyboardCaptureProvider for MacosCapture {
    async fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let running = self.running.clone();
        let shutdown = self.shutdown.clone();
        let _tx = self.tx.clone();
        let _ = _tx; // suppress unused warning

        let thread = std::thread::Builder::new()
            .name("macos-cgevent-tap".into())
            .spawn(move || {
                #[cfg(target_os = "macos")]
                {
                    if let Err(e) = run_macos_event_tap(_tx, running, shutdown) {
                        tracing::error!("macOS event tap error: {}", e);
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    tracing::warn!("macOS event tap not available on this platform");
                    running.store(true, Ordering::Relaxed);
                    while !shutdown.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    running.store(false, Ordering::Relaxed);
                }
            })?;

        self.handle = Some(JoinHandle { thread });
        self.running.store(true, Ordering::Relaxed);
        info!("MacosCapture started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.thread.join();
        }
        self.running.store(false, Ordering::Relaxed);
        info!("MacosCapture stopped");
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
        "cgevent-tap"
    }
}

#[cfg(target_os = "macos")]
fn run_macos_event_tap(
    tx: broadcast::Sender<InputEvent>,
    running: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    use crate::keymap::keycode_to_key;
    use core_foundation::runloop::*;
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        EventField,
    };
    use input_core::events::{KeyState, KeyboardEvent};
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime};

    let (event_tx, event_rx) = mpsc::channel::<(u32, bool)>();
    let event_tx_for_closure = event_tx.clone();

    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::KeyDown, CGEventType::KeyUp],
        move |_proxy, event_type, event| {
            let event_type_u32 = event_type as u32;
            if event_type_u32 == CGEventType::KeyDown as u32
                || event_type_u32 == CGEventType::KeyUp as u32
            {
                let keycode =
                    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;
                let key_down = event_type_u32 == CGEventType::KeyDown as u32;
                let _ = event_tx_for_closure.send((keycode, key_down));
            }
            None
        },
    )
    .map_err(|_| {
        anyhow::anyhow!("Failed to create CGEventTap. Check Accessibility permissions.")
    })?;

    let source = tap
        .mach_port
        .create_runloop_source(0)
        .map_err(|_| anyhow::anyhow!("Failed to create run loop source"))?;
    let run_loop = CFRunLoop::get_current();
    run_loop.add_source(&source, unsafe { kCFRunLoopDefaultMode });

    tap.enable();
    running.store(true, Ordering::Relaxed);

    info!("macOS event tap running");

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            Duration::from_millis(10),
            true,
        );

        while let Ok((keycode, key_down)) = event_rx.try_recv() {
            let key = keycode_to_key(keycode);
            let state = if key_down {
                KeyState::Pressed
            } else {
                KeyState::Released
            };

            let event = InputEvent::Keyboard(KeyboardEvent {
                key,
                state,
                timestamp: SystemTime::now(),
                native_code: keycode,
            });

            let _ = tx.send(event);
        }
    }

    run_loop.remove_source(&source, unsafe { kCFRunLoopDefaultMode });

    running.store(false, Ordering::Relaxed);
    Ok(())
}

impl Drop for MacosCapture {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.thread.join();
        }
    }
}

/// Factory for creating macOS keyboard capture providers.
pub struct MacosCaptureFactory;

impl MacosCaptureFactory {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacosCaptureFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardCaptureFactory for MacosCaptureFactory {
    fn create(&self) -> Box<dyn platform::capture::KeyboardCaptureProvider> {
        Box::new(MacosCapture::new().expect("Failed to create MacosCapture"))
    }

    fn platform_name(&self) -> &str {
        "macos-cgevent-tap"
    }
}

/// Factory for creating macOS overlay renderer.
pub struct MacRendererFactory {
    inner: overlay::MacRendererFactory,
}

impl MacRendererFactory {
    pub fn new() -> Self {
        Self {
            inner: overlay::MacRendererFactory::new(),
        }
    }
}

impl Default for MacRendererFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayRendererFactory for MacRendererFactory {
    fn create(&self, bus: MessageBus) -> Box<dyn platform::overlay::OverlayRenderer> {
        self.inner.create(bus)
    }

    fn platform_name(&self) -> &str {
        "macos"
    }
}
