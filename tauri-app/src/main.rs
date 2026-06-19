use eframe::egui;
use input_core::events::{ModifierState, ProcessedEvent};
use input_core::ipc::MessageBus;
use input_core::processor::DefaultEventProcessor;
use input_core::traits::{EventProcessor, KeyboardCaptureProvider, ProcessorConfig};
use std::sync::mpsc;
use tracing::{error, info};

fn parse_log_level() -> String {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--trace") {
        return "trace".into();
    }
    if args.iter().any(|a| a == "--debug") {
        return "debug".into();
    }
    std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(parse_log_level())
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    info!("EchoInput starting");

    let (display_tx, display_rx) = mpsc::channel::<String>();

    std::thread::spawn(move || {
        if let Err(e) = run_capture(display_tx) {
            error!("Capture failed: {}", e);
        }
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([260.0, 50.0])
            .with_min_inner_size([260.0, 50.0])
            .with_decorations(false)
            .with_always_on_top()
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "EchoInput",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(OverlayApp {
                text: "EchoInput Ready".into(),
                rx: display_rx,
                positioned: false,
            }))
        }),
    )
    .unwrap();
}

struct OverlayApp {
    text: String,
    rx: mpsc::Receiver<String>,
    positioned: bool,
}

impl eframe::App for OverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(text) = self.rx.try_recv() {
            self.text = text;
        }

        if !self.positioned {
            let screen = ctx.input(|i| i.screen_rect);
            let x = screen.max.x - 280.0;
            let y = 20.0;
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
            self.positioned = true;
        }

        ctx.request_repaint();

        let frame = egui::Frame::NONE
            .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 20, 210))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(16, 8));

        egui::CentralPanel::default()
            .frame(frame)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(&self.text)
                            .size(24.0)
                            .color(egui::Color32::WHITE)
                            .family(egui::FontFamily::Monospace),
                    );
                });
            });
    }
}

fn run_capture(tx: mpsc::Sender<String>) -> anyhow::Result<()> {
    let bus = MessageBus::new(1024);

    let mut capture = platform_linux::evdev_capture::EvdevCapture::with_sender(bus.input_sender());

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        capture.start().await?;
        info!("Capture started");

        let mut input_rx = bus.subscribe_input();
        let mut processor = DefaultEventProcessor::new(ProcessorConfig {
            group_shortcuts: true,
            ..Default::default()
        });

        loop {
            match input_rx.recv().await {
                Ok(event) => {
                    let processed = processor.process(event);
                    for pe in processed {
                        match pe {
                            ProcessedEvent::Shortcut(combo) => {
                                info!("Shortcut: {}", combo.display);
                                let _ = tx.send(combo.display);
                            }
                            ProcessedEvent::RawKey(kbd) => {
                                let combo = input_core::events::ShortcutCombo::new(
                                    ModifierState::default(),
                                    Some(kbd.key),
                                );
                                info!("Key: {}", combo.display);
                                let _ = tx.send(combo.display);
                            }
                            ProcessedEvent::ModifierChange(_) => {}
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
        Ok(())
    })
}
