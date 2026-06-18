#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use input_core::ipc::{MessageBus, OverlayCommand, SettingsUpdate};
use input_core::overlay::OverlayConfig;
use input_core::processor::DefaultEventProcessor;
use input_core::traits::{EventProcessor, KeyboardCaptureProvider, OverlayRendererFactory, ProcessorConfig};
use overlay::OverlayManager;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    info!("EchoInput starting...");

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        if let Err(e) = run().await {
            error!("Fatal error: {}", e);
            std::process::exit(1);
        }
    });
}

async fn run() -> anyhow::Result<()> {
    // ── 1. Create the message bus ──────────────────────────────
    let bus = MessageBus::new(1024);
    info!("MessageBus created");

    // ── 2. Create platform-specific capture provider ───────────
    #[cfg(target_os = "linux")]
    let mut capture = {
        info!("Using evdev capture (Linux)");
        platform_linux::evdev_capture::EvdevCapture::with_sender(bus.input_sender())
    };

    #[cfg(target_os = "windows")]
    let mut capture = {
        info!("Using Windows hook capture");
        platform_windows::WindowsCapture::with_sender(bus.input_sender())
    };

    #[cfg(target_os = "macos")]
    let mut capture = {
        info!("Using CGEventTap capture (macOS)");
        platform_macos::MacosCapture::with_sender(bus.input_sender())
    };

    info!("Capture provider: {}", capture.name());

    // ── 3. Create event processor ──────────────────────────────
    let mut processor = DefaultEventProcessor::new(ProcessorConfig {
        group_shortcuts: true,
        history_length: 10,
        ..Default::default()
    });

    // ── 4. Create and start overlay manager ────────────────────
    let mut overlay = OverlayManager::new(OverlayConfig::default());
    overlay.run(bus.clone());

    // ── 5. Create and start Wayland renderer ───────────────────
    #[cfg(target_os = "linux")]
    let mut renderer = {
        let factory = overlay_wayland::WaylandRendererFactory::new();
        info!("Creating renderer: {}", factory.platform_name());
        factory.create(bus.clone())
    };

    #[cfg(not(target_os = "linux"))]
    let mut renderer = {
        // Use mock renderer on non-Linux platforms
        Box::new(overlay::MockRenderer::with_bus(bus.clone()))
    };

    renderer.start(OverlayConfig::default()).await?;
    info!("Renderer started: {}", renderer.name());

    // ── 6. Subscribe to bus channels ───────────────────────────
    let mut input_rx = bus.subscribe_input();

    // ── 7. Start capture ───────────────────────────────────────
    capture.start().await?;
    info!("Capture started");

    // ── 8. Spawn task: input → processor → shortcut bus ────────
    let bus_clone = bus.clone();
    tokio::spawn(async move {
        loop {
            match input_rx.recv().await {
                Ok(event) => {
                    let processed = processor.process(event);
                    for pe in processed {
                        match pe {
                            input_core::events::ProcessedEvent::Shortcut(combo) => {
                                let shortcut_event =
                                    input_core::ipc::ShortcutEvent::new(combo.clone());
                                bus_clone.publish_shortcut(shortcut_event);
                                println!("  [shortcut] {}", combo);
                            }
                            input_core::events::ProcessedEvent::ModifierChange(_mods) => {
                                if let Some(compose) = processor.current_compose() {
                                    print!("\r  {}...", compose);
                                    use std::io::Write;
                                    std::io::stdout().flush().unwrap_or(());
                                }
                            }
                            input_core::events::ProcessedEvent::RawKey(kbd) => {
                                println!(
                                    "  [raw] {:?} {:?} (scancode: {})",
                                    kbd.key, kbd.state, kbd.native_code
                                );
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Missed {} input events", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Input channel closed");
                    break;
                }
            }
        }
    });

    // ── 9. Demonstrate settings updates (simulated Tauri) ─────
    let bus_demo = bus.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        info!("Demo: Publishing settings update (theme -> Light)");
        bus_demo.publish_settings(SettingsUpdate::Theme(
            input_core::overlay::Theme::Light,
        ));

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        info!("Demo: Publishing overlay command (Restart)");
        bus_demo.publish_command(OverlayCommand::Restart);

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        info!("Demo: Publishing batch settings update");
        bus_demo.publish_settings(SettingsUpdate::Batch(vec![
            SettingsUpdate::Opacity(0.6),
            SettingsUpdate::Position(input_core::overlay::OverlayPosition::TopCenter),
            SettingsUpdate::Theme(input_core::overlay::Theme::Dark),
        ]));
    });

    info!("All tasks spawned. Press Ctrl+C to exit.");

    // ── 10. Wait for shutdown ─────────────────────────────────
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received");

    renderer.stop().await?;
    bus.publish_command(OverlayCommand::Stop);
    capture.stop().await?;

    info!("EchoInput stopped");
    Ok(())
}
