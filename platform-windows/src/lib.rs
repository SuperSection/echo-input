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

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

/// Windows keyboard capture provider using SetWindowsHookEx(WH_KEYBOARD_LL).
///
/// Installs a low-level keyboard hook that captures all keyboard input
/// system-wide. The hook runs on a dedicated OS thread with a message pump.
pub struct WindowsCapture {
    tx: broadcast::Sender<InputEvent>,
    running: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    hook_handle: Option<JoinHandle>,
}

struct JoinHandle {
    thread: std::thread::JoinHandle<()>,
}

impl WindowsCapture {
    pub fn new() -> Result<Self> {
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            tx,
            running: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            hook_handle: None,
        })
    }

    pub fn with_shutdown(shutdown: Arc<AtomicBool>) -> Result<Self> {
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            tx,
            running: Arc::new(AtomicBool::new(false)),
            shutdown,
            hook_handle: None,
        })
    }
}

#[async_trait::async_trait]
impl KeyboardCaptureProvider for WindowsCapture {
    async fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let tx = self.tx.clone();
        let running = self.running.clone();
        let shutdown = self.shutdown.clone();

        let thread = std::thread::Builder::new()
            .name("windows-keyboard-hook".into())
            .spawn(move || {
                #[cfg(target_os = "windows")]
                {
                    if let Err(e) = run_windows_hook(tx, running, shutdown) {
                        error!("Windows keyboard hook error: {}", e);
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    warn!("Windows keyboard capture not available on this platform");
                    running.store(true, Ordering::Relaxed);
                    while !shutdown.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    running.store(false, Ordering::Relaxed);
                }
            })?;

        self.hook_handle = Some(JoinHandle { thread });
        self.running.store(true, Ordering::Relaxed);
        info!("WindowsCapture started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.hook_handle.take() {
            let _ = handle.thread.join();
        }
        self.running.store(false, Ordering::Relaxed);
        info!("WindowsCapture stopped");
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
        "windows-hook"
    }
}

#[cfg(target_os = "windows")]
fn run_windows_hook(
    tx: broadcast::Sender<InputEvent>,
    running: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    use crate::keymap::vk_with_scancode_to_key;
    use input_core::events::{KeyboardEvent, KeyState};
    use std::time::SystemTime;

    unsafe extern "system" fn keyboard_proc(
        n_code: i32,
        w_param: windows::Win32::Foundation::WPARAM,
        l_param: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        use std::sync::OnceLock;

        static TX_CHANNEL: OnceLock<crossbeam_channel::Sender<(
            u32,
            u32,
            bool,
        )>> = OnceLock::new();

        let tx = TX_CHANNEL.get();

        if n_code >= 0 {
            let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
            let key_down = w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_SYSKEYDOWN;
            let key_up = w_param.0 as u32 == WM_KEYUP || w_param.0 as u32 == WM_SYSKEYUP;

            if key_down || key_up {
                if let Some(tx) = tx {
                    let _ = tx.send((kb.vkCode, kb.scanCode, key_down));
                }
            }
        }

        CallNextHookEx(None, n_code, windows::Win32::Foundation::WPARAM(w_param.0), windows::Win32::Foundation::LPARAM(l_param.0))
    }

    use std::sync::OnceLock;
    static HOOK_TX: OnceLock<crossbeam_channel::Sender<(u32, u32, bool)>> = OnceLock::new();

    let (hook_tx, hook_rx) = crossbeam_channel::bounded::<(u32, u32, bool)>(1024);
    let _ = HOOK_TX.set(hook_tx);

    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0);
        if hook.is_err() {
            return Err(anyhow::anyhow!("Failed to install keyboard hook"));
        }
        let _hook = hook.unwrap();

        running.store(true, Ordering::Relaxed);

        let mut msg: MSG = std::mem::zeroed();

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            while let Ok((vk, scan_code, key_down)) = hook_rx.try_recv() {
                let key = vk_with_scancode_to_key(vk, scan_code);
                let state = if key_down {
                    KeyState::Pressed
                } else {
                    KeyState::Released
                };

                let event = InputEvent::Keyboard(KeyboardEvent {
                    key,
                    state,
                    timestamp: SystemTime::now(),
                    native_code: scan_code,
                });

                let _ = tx.send(event);
            }

            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        let _ = UnhookWindowsHookEx(_hook);
    }

    running.store(false, Ordering::Relaxed);
    Ok(())
}

impl Drop for WindowsCapture {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.hook_handle.take() {
            let _ = handle.thread.join();
        }
    }
}

/// Factory for creating Windows keyboard capture providers.
pub struct WindowsCaptureFactory;

impl WindowsCaptureFactory {
    pub fn new() -> Self { Self }
}

impl Default for WindowsCaptureFactory {
    fn default() -> Self { Self::new() }
}

impl KeyboardCaptureFactory for WindowsCaptureFactory {
    fn create(&self) -> Box<dyn platform::capture::KeyboardCaptureProvider> {
        Box::new(WindowsCapture::new().expect("Failed to create WindowsCapture"))
    }

    fn platform_name(&self) -> &str {
        "windows-hook"
    }
}

/// Factory for creating Windows overlay renderer.
pub struct WindowsRendererFactory {
    inner: overlay::WindowsRendererFactory,
}

impl WindowsRendererFactory {
    pub fn new() -> Self { Self { inner: overlay::WindowsRendererFactory::new() } }
}

impl Default for WindowsRendererFactory {
    fn default() -> Self { Self::new() }
}

impl OverlayRendererFactory for WindowsRendererFactory {
    fn create(&self, bus: MessageBus) -> Box<dyn platform::overlay::OverlayRenderer> {
        self.inner.create(bus)
    }

    fn platform_name(&self) -> &str {
        "windows"
    }
}