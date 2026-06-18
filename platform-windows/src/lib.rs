use anyhow::Result;
use input_core::events::InputEvent;
use input_core::traits::{CaptureFeatures, KeyboardCaptureProvider};
use tokio::sync::broadcast;
use tracing::info;

/// Windows keyboard capture provider (stub).
///
/// Will use `SetWindowsHookEx(WH_KEYBOARD_LL, ...)` for global
/// keyboard hook on Windows.
pub struct WindowsCapture {
    tx: broadcast::Sender<InputEvent>,
    running: bool,
}

impl WindowsCapture {
    pub fn new() -> Result<Self> {
        let (tx, _) = broadcast::channel(1024);
        Ok(Self { tx, running: false })
    }
}

#[async_trait::async_trait]
impl KeyboardCaptureProvider for WindowsCapture {
    async fn start(&mut self) -> Result<()> {
        info!("WindowsCapture: stub - not yet implemented");
        self.running = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.running = false;
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
