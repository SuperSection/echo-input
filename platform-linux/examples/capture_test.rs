//! Standalone test binary for validating evdev keyboard capture.
//!
//! Usage:
//!   cargo run -p platform-linux --example capture_test
//!
//! This will capture keyboard events and print them to the terminal.
//! Press Ctrl+C to exit.

use input_core::processor::DefaultEventProcessor;
use input_core::traits::{EventProcessor, KeyboardCaptureProvider, ProcessorConfig};
use std::io::Write;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("debug")
        .init();

    // Create capture
    let mut capture = platform_linux::evdev_capture::EvdevCapture::new()?;
    let mut rx = capture.subscribe();

    // Create processor
    let mut processor = DefaultEventProcessor::new(ProcessorConfig {
        group_shortcuts: true,
        history_length: 5,
        ..Default::default()
    });

    // Start
    capture.start().await?;

    println!("=== EchoInput Capture Test ===");
    println!("Capture provider: {}", capture.name());
    println!("Features: {:?}", capture.features());
    println!();
    println!("Type keys to see shortcuts. Press Ctrl+C to exit.");
    println!("---");

    // Process events
    loop {
        match rx.recv().await {
            Ok(event) => {
                let processed = processor.process(event);
                for pe in processed {
                    match pe {
                        input_core::events::ProcessedEvent::Shortcut(combo) => {
                            println!("  {}", combo);
                        }
                        input_core::events::ProcessedEvent::ModifierChange(_mods) => {
                            // Show composing state
                            if let Some(compose) = processor.current_compose() {
                                print!("\r  {}...", compose);
                                std::io::stdout().flush().unwrap_or(());
                            } else {
                                print!("\r{}\r", " ".repeat(60));
                                std::io::stdout().flush().unwrap_or(());
                            }
                        }
                        input_core::events::ProcessedEvent::RawKey(kbd) => {
                            println!(
                                "  {:?} {:?} (scancode: {})",
                                kbd.key, kbd.state, kbd.native_code
                            );
                        }
                    }
                }
            }
            Err(_) => break,
        }
    }

    capture.stop().await?;
    println!("\n=== Test Complete ===");

    // Print history
    let history = processor.history();
    if !history.is_empty() {
        println!("\nShortcut History:");
        for (i, combo) in history.iter().enumerate() {
            println!("  {}. {}", i + 1, combo);
        }
    }

    Ok(())
}
