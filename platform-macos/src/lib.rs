pub mod keymap;
pub mod overlay;

use anyhow::Result;
use input_core::events::InputEvent;
use input_core::keys::VirtualKey;
use input_core::ipc::MessageBus;
use platform::capture::{CaptureFeatures, KeyboardCaptureProvider, KeyboardCaptureFactory};
use platform::overlay::OverlayRendererFactory;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};

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

        let tx = self.tx.clone();
        let running = self.running.clone();
        let shutdown = self.shutdown.clone();

        let thread = std::thread::Builder::new()
            .name("macos-cgevent-tap".into())
            .spawn(move || {
                #[cfg(target_os = "macos")]
                {
                    if let Err(e) = run_macos_event_tap(tx, running, shutdown) {
                        error!("macOS event tap error: {}", e);
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    warn!("macOS event tap not available on this platform");
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
    use core_foundation::base::{TCFType, ToVoid};
    use core_foundation::mach_port::{CFMachPort, CFMachPortRef};
    use core_foundation::runloop::*;
    use core_foundation::string::CFString;
    use std::ffi::c_void;
    use std::sync::mpsc;
    use input_core::events::{KeyboardEvent, KeyState};
    use std::time::SystemTime;

    // Channel for the event tap callback to send events back
    let (event_tx, event_rx) = mpsc::channel::<(u32, bool)>();

    // Store the sender in thread-local storage for the C callback
    thread_local! {
        static EVENT_TX: std::cell::RefCell<Option<mpsc::Sender<(u32, bool)>>> = std::cell::RefCell::new(None);
    }

    extern "C" fn event_tap_callback(
        _proxy: core_graphics::sys::CGEventTapProxy,
        event_type: core_graphics::sys::CGEventType,
        event: core_graphics::sys::CGEventRef,
        _user_info: *mut c_void,
    ) -> core_graphics::sys::CGEventRef {
        unsafe {
            let event_type_u32 = event_type.0 as u32;

            // Only process keyDown (10) and keyUp (11)
            if event_type_u32 == 10 || event_type_u32 == 11 {
                let keycode = core_graphics::sys::CGEventGetIntegerValueField(
                    event,
                    core_graphics::sys::kCGKeyboardEventKeycode as u64,
                ) as u32;

                let key_down = event_type_u32 == 10;

                EVENT_TX.with(|tx| {
                    if let Some(ref sender) = *tx.borrow() {
                        let _ = sender.send((keycode, key_down));
                    }
                });
            }
        }

        // Return the event unchanged (pass it through)
        event
    }

    EVENT_TX.with(|tx| {
        *tx.borrow_mut() = Some(event_tx);
    });

    unsafe {
        // Create the event tap
        let event_mask = (1u64 << 10) | (1u64 << 11); // keyDown | keyUp

        let tap = core_graphics::sys::CGEventTapCreate(
            core_graphics::sys::kCGHIDEventTap,
            core_graphics::sys::kCGHeadInsertEventTap,
            core_graphics::sys::kCGEventTapOptionDefault,
            event_mask,
            event_tap_callback,
            std::ptr::null_mut(),
        );

        if tap.is_null() {
            return Err(anyhow::anyhow!(
                "Failed to create CGEventTap. Check Accessibility permissions."
            ));
        }

        let tap_port = CFMachPort::wrap_under_rule(0, tap as *mut _);

        // Add to run loop
        let run_loop = CFRunLoop::get_current();
        let source = core_foundation::runloop::CFRunLoopSource::new(
            tap_port.as_concrete_TypeRef(),
            0,
        );
        run_loop.add_source(&source, kCFRunLoopDefaultMode);

        // Enable the tap
        core_graphics::sys::CGEventTapEnable(tap, true);

        running.store(true, Ordering::Relaxed);

        // Run the event loop
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            // Process CFRunLoop events with a short timeout
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, true);

            // Receive events from the tap callback
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

        // Cleanup
        run_loop.remove_source(&source, kCFRunLoopDefaultMode);
    }

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
    pub fn new() -> Self { Self }
}

impl Default for MacosCaptureFactory {
    fn default() -> Self { Self::new() }
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
    pub fn new() -> Self { Self { inner: overlay::MacRendererFactory::new() } }
}

impl Default for MacRendererFactory {
    fn default() -> Self { Self::new() }
}

impl OverlayRendererFactory for MacRendererFactory {
    fn create(&self, bus: MessageBus) -> Box<dyn platform::overlay::OverlayRenderer> {
        self.inner.create(bus)
    }

    fn platform_name(&self) -> &str {
        "macos"
    }
}