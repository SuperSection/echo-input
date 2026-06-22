//! Standalone test binary for validating evdev keyboard capture.
//!
//! Usage:
//!   cargo run -p platform-linux --example capture_test
//!
//! This will capture keyboard events and print them to the terminal.
//! Press Ctrl+C to exit.

use input_core::processor::DefaultEventProcessor;
use input_core::traits::{EventProcessor, ProcessorConfig};
use platform::capture::KeyboardCaptureProvider;
use std::io::Write;
use std::time::{Duration, Instant};

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
    println!();
    println!("Waiting for keyboard events...");
    println!();

    // Track events for diagnostics
    let mut event_count: u64 = 0;
    let mut shortcut_count: u64 = 0;
    let mut modifier_count: u64 = 0;
    let mut raw_count: u64 = 0;
    let start = Instant::now();
    let mut last_event_at: Option<Instant> = None;

    // Process events
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        event_count += 1;
                        last_event_at = Some(Instant::now());
                        let elapsed = start.elapsed();
                        eprintln!(
                            "[recv #{} at {:.3}s] event received by receiver",
                            event_count, elapsed.as_secs_f64()
                        );
                        let processed = processor.process(event);
                        if processed.is_empty() {
                            eprintln!(
                                "  [recv #{}] processor returned 0 events (filtered or release)",
                                event_count
                            );
                        }
                        for pe in &processed {
                            match pe {
                                input_core::events::ProcessedEvent::Shortcut(combo) => {
                                    shortcut_count += 1;
                                    println!("  [shortcut #{}] {}", shortcut_count, combo);
                                }
                                input_core::events::ProcessedEvent::ModifierChange(_mods) => {
                                    modifier_count += 1;
                                    if let Some(compose) = processor.current_compose() {
                                        print!("\r  [mod #{}] {}...", modifier_count, compose);
                                        std::io::stdout().flush().unwrap_or(());
                                    } else {
                                        print!("\r{}\r", " ".repeat(60));
                                        std::io::stdout().flush().unwrap_or(());
                                    }
                                }
                                input_core::events::ProcessedEvent::RawKey(kbd) => {
                                    raw_count += 1;
                                    println!(
                                        "  [raw #{}] {} {:?} (scancode: {})",
                                        raw_count, kbd.key, kbd.state, kbd.native_code
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("\n=== Channel error: {:?} ===", e);
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(3)) => {
                // Heartbeat: check if we're still receiving
                let since_last = last_event_at.map(|t| t.elapsed());
                eprintln!(
                    "[heartbeat at {:.3}s] events={} shortcuts={} mods={} raw={} last_event_ago={:?}",
                    start.elapsed().as_secs_f64(),
                    event_count,
                    shortcut_count,
                    modifier_count,
                    raw_count,
                    since_last.map(|d| format!("{:.3}s", d.as_secs_f64())).unwrap_or_else(|| "never".into())
                );
            }
        }
    }

    capture.stop().await?;
    println!("\n=== Test Complete ===");
    println!("Events received: {}", event_count);
    println!("Shortcuts emitted: {}", shortcut_count);
    println!("Modifier changes: {}", modifier_count);
    println!("Raw keys: {}", raw_count);

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
