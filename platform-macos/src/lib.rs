use anyhow::Result;
use input_core::events::InputEvent;
use input_core::traits::{CaptureFeatures, KeyboardCaptureProvider};
use tokio::sync::broadcast;
use tracing::info;

/// macOS keyboard capture provider (stub).
///
/// Will use `CGEventTap` for global keyboard event monitoring.
pub struct MacosCapture {
    tx: broadcast::Sender<InputEvent>,
    running: bool,
}

impl MacosCapture {
    pub fn new() -> Result<Self> {
        let (tx, _) = broadcast::channel(1024);
        Ok(Self { tx, running: false })
    }
}

#[async_trait::async_trait]
impl KeyboardCaptureProvider for MacosCapture {
    async fn start(&mut self) -> Result<()> {
        info!("MacosCapture: stub - not yet implemented");
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
        "cgevent-tap"
    }
}
