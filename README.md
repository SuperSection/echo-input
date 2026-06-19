# EchoInput

A privacy-first keyboard visualization overlay for Wayland, Hyprland, and modern Linux desktops.

Shows your keyboard shortcuts on screen as you type. Like KeyViz, but for Wayland.

No telemetry, no cloud, no tracking.

## Quick Start

```sh
# Build
cargo build --release

# Run (Hyprland / Wayland)
./target/release/echoinput

# Run with settings GUI
./target/release/echoinput --settings
```

Requires read access to `/dev/input/event*` (add your user to the `input` group, or run with `sudo`).

## What It Does

Each keystroke pops up as a visual keycap overlay on your screen:

- **Keycap-style rendering** — rounded rectangles with key labels, like KeyViz
- **Always on top** — Wayland layer-shell `Overlay` layer
- **Click-through** — doesn't steal focus or intercept input
- **Fade-out animation** — appears on keypress, fades after configurable duration
- **Multi-monitor** — picks the right output, supports HiDPI

## Configuration

On first run, creates `~/.config/echoinput/config.toml`:

```toml
[position]      # BottomCenter (default), TopLeft, TopRight, TopCenter,
                # BottomLeft, BottomRight, Center
position = "BottomCenter"

[scale]         # Small (16pt), Medium (24pt), Large (32pt), ExtraLarge (48pt)
scale = "Medium"

opacity = 0.9
display_duration_ms = 1500
history_length = 3

[theme]         # Dark (default), Light, System
theme = "Dark"
```

Or use the settings GUI: `echoinput --settings`

## Architecture

```
Input Provider  →  Event Processor  →  MessageBus  →  Overlay Renderer
(evdev)           (grouping/dedup)     (broadcast)     (Wayland+Cairo)
```

## Crate Structure

| Crate | Purpose |
|---|---|
| `input-core` | Types, traits, MessageBus, event processor, config |
| `overlay` | Cross-platform overlay state manager |
| `overlay-wayland` | Wayland layer-shell renderer with Cairo |
| `platform-linux` | evdev keyboard capture |
| `tauri-app` | Legacy egui-based overlay (top-right corner) |

## Platform Support

| Platform | Input Capture | Overlay | Status |
|---|---|---|---|
| Linux (Wayland) | evdev | wlr-layer-shell + Cairo | Working |
| Linux (X11) | evdev | — | Planned |
| Windows | SetWindowsHookEx | Transparent window | Stub |
| macOS | CGEventTap | NSPanel | Stub |

## License

MIT OR Apache-2.0
