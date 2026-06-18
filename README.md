# EchoInput

A privacy-first keyboard visualization overlay for Wayland, Hyprland, and modern Linux desktops.

Shows your keyboard shortcuts on screen as you type. No telemetry, no cloud, no tracking.

## Architecture

```
Input Provider  →  Event Processor  →  MessageBus  →  Overlay Renderer
(platform)        (grouping/dedup)     (broadcast)     (Wayland/Win/Mac)
```

Each layer communicates only through the `MessageBus`. Platform-specific code never touches the overlay directly.

## Crate Structure

| Crate | Purpose |
|---|---|
| `input-core` | Types, traits, MessageBus, event processor |
| `overlay` | Cross-platform overlay state manager, MockRenderer |
| `overlay-wayland` | Wayland layer-shell renderer with Cairo |
| `platform-linux` | evdev keyboard capture |
| `platform-windows` | Windows hook capture (stub) |
| `platform-macos` | CGEventTap capture (stub) |
| `tauri-app` | Application binary, wires everything together |

## Requirements

- Rust 2021 edition
- Linux: `wayland-client`, `wayland-protocols`, `wayland-protocols-wlr`, `cairo-rs`
- A Wayland compositor with wlr-layer-shell support (Hyprland, Sway, etc.)

## Build

```sh
cargo build
```

## Run

```sh
cargo run -p tauri-app
```

Requires permission to read `/dev/input/event*` (add your user to the `input` group, or run with `sudo`).

## Platform Support

| Platform | Input Capture | Overlay | Status |
|---|---|---|---|
| Linux (Wayland) | evdev | wlr-layer-shell + Cairo | Working |
| Linux (X11) | evdev | — | Planned |
| Windows | SetWindowsHookEx | Transparent window | Stub |
| macOS | CGEventTap | NSPanel | Stub |

## Overlay Features

- Always on top (layer-shell `Overlay` layer)
- Click-through (exclusive zone -1, no keyboard interactivity)
- Transparent rounded rectangle background
- Fade-out animation after configurable duration
- Multi-monitor support with HiDPI scaling
- Configurable position, opacity, scale, and display duration

## Configuration

Overlay behavior is controlled via `OverlayConfig`:

```rust
OverlayConfig {
    position: OverlayPosition::TopRight,
    scale: OverlayScale::Medium,
    opacity: 0.9,
    display_duration: Duration::from_secs(3),
    history_length: 5,
    theme: Theme::Dark,
    monitor: None, // None = default output
}
```

Settings can be updated at runtime through the `MessageBus` using `SettingsUpdate`.

## License

MIT OR Apache-2.0
