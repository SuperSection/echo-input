pub mod evdev_capture;
pub mod keymap;
pub mod overlay_wayland;
pub mod overlay_x11;

use input_core::ipc::MessageBus;
use platform::capture::KeyboardCaptureFactory;
use platform::overlay::OverlayRendererFactory;

/// Factory for creating Linux keyboard capture providers.
pub struct EvdevCaptureFactory;

impl EvdevCaptureFactory {
    pub fn new() -> Self { Self }
}

impl Default for EvdevCaptureFactory {
    fn default() -> Self { Self::new() }
}

impl KeyboardCaptureFactory for EvdevCaptureFactory {
    fn create(&self) -> Box<dyn platform::capture::KeyboardCaptureProvider> {
        Box::new(evdev_capture::EvdevCapture::new().expect("Failed to create EvdevCapture"))
    }

    fn platform_name(&self) -> &str {
        "linux-evdev"
    }
}

/// Factory for creating Wayland overlay renderer on Linux.
pub struct LinuxWaylandRendererFactory {
    inner: overlay_wayland::WaylandRendererFactory,
}

impl LinuxWaylandRendererFactory {
    pub fn new() -> Self { Self { inner: overlay_wayland::WaylandRendererFactory::new() } }
}

impl Default for LinuxWaylandRendererFactory {
    fn default() -> Self { Self::new() }
}

impl OverlayRendererFactory for LinuxWaylandRendererFactory {
    fn create(&self, bus: MessageBus) -> Box<dyn platform::overlay::OverlayRenderer> {
        self.inner.create(bus)
    }

    fn platform_name(&self) -> &str {
        "linux-wayland"
    }
}

/// Factory for creating X11 overlay renderer on Linux.
pub struct LinuxX11RendererFactory {
    inner: overlay_x11::X11RendererFactory,
}

impl LinuxX11RendererFactory {
    pub fn new() -> Self { Self { inner: overlay_x11::X11RendererFactory::new() } }
}

impl Default for LinuxX11RendererFactory {
    fn default() -> Self { Self::new() }
}

impl OverlayRendererFactory for LinuxX11RendererFactory {
    fn create(&self, bus: MessageBus) -> Box<dyn platform::overlay::OverlayRenderer> {
        self.inner.create(bus)
    }

    fn platform_name(&self) -> &str {
        "linux-x11"
    }
}