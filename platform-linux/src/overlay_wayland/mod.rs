pub mod animation;
pub mod error;
pub mod keymap_compat;
pub mod renderer;

pub use renderer::WaylandRenderer;

use input_core::ipc::MessageBus;
use platform::overlay::OverlayRendererFactory;

/// Factory for creating Wayland overlay renderers.
pub struct WaylandRendererFactory;

impl WaylandRendererFactory {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WaylandRendererFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayRendererFactory for WaylandRendererFactory {
    fn create(&self, bus: MessageBus) -> Box<dyn platform::overlay::OverlayRenderer> {
        Box::new(WaylandRenderer::new(bus))
    }

    fn platform_name(&self) -> &str {
        "wayland"
    }
}