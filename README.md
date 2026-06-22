# EchoInput

A privacy-first keyboard visualization overlay for Wayland, X11, Windows, and macOS.

Shows your keyboard shortcuts on screen as you type. Like KeyViz, but cross-platform.

No telemetry, no cloud, no tracking.

## 📥 Installation

### Linux

#### Arch Linux / Manjaro (AUR)
```bash
yay -S echoinput
# or
paru -S echoinput
```

#### AppImage (Universal)
```bash
# Download from GitHub Releases
wget https://github.com/SuperSection/echoinput/releases/latest/download/echoinput-<version>-x86_64.AppImage
chmod +x echoinput-*.AppImage
./echoinput-*.AppImage
```

#### Debian / Ubuntu (.deb)
```bash
wget https://github.com/SuperSection/echoinput/releases/latest/download/echoinput-<version>-amd64.deb
sudo dpkg -i echoinput-*.deb
# Fix dependencies if needed
sudo apt-get install -f
```

#### Fedora / RHEL (.rpm)
```bash
wget https://github.com/SuperSection/echoinput/releases/latest/download/echoinput-<version>-x86_64.rpm
sudo dnf install echoinput-*.rpm
```

#### From Source
```bash
# Install dependencies
# Ubuntu/Debian:
sudo apt-get install libwayland-dev libxkbcommon-dev libcairo2-dev pkg-config libx11-dev libxext-dev libxfixes-dev libxrandr-dev libxi-dev libglib2.0-dev

# Fedora:
sudo dnf install wayland-devel libxkbcommon-devel cairo-devel pkg-config libX11-devel libXext-devel libXfixes-devel libXrandr-devel libXi-devel glib2-devel

# Arch:
sudo pacman -S wayland libxkbcommon cairo pkg-config libx11 libxext libxfixes libxrandr libxi glib2

# Build
cargo build --release
./target/release/echoinput
```

### Windows

#### Portable (Recommended)
```powershell
# Download from GitHub Releases
# Extract zip and run echoinput.exe
```

#### Installer
```powershell
# Download echoinput-<version>-x86_64-setup.exe from GitHub Releases
# Run the installer
```

#### WinGet (Coming Soon)
```powershell
winget install EchoInput.EchoInput
```

#### From Source
```powershell
# Install Rust: https://rustup.rs/
# Build
cargo build --release
.\target\release\echoinput.exe
```

### macOS

#### Homebrew (Coming Soon)
```bash
brew install echoinput/tap/echoinput
# or
brew install --cask echoinput
```

#### DMG Installer
```bash
# Download echoinput-<version>-universal.dmg from GitHub Releases
# Open DMG and drag EchoInput to Applications
```

#### From Source
```bash
# Install dependencies
brew install cairo pkg-config

# Build
cargo build --release
./target/release/echoinput
```

> **⚠️ macOS Permissions**: On first run, macOS will prompt for **Accessibility permissions**. Grant them in:
> System Settings → Privacy & Security → Accessibility → EchoInput

---

## 🚀 Quick Start

```sh
# Run the overlay
echoinput

# Run with settings GUI
echoinput --settings
```

### Linux Permissions
Requires read access to `/dev/input/event*`:
```bash
sudo usermod -aG input $USER
# Log out and back in, or run with sudo
```

---

## ✨ Features

| Feature | Description |
|---------|-------------|
| **Keycap-style rendering** | Rounded rectangles with key labels, like KeyViz |
| **Always on top** | Wayland layer-shell / Windows TopMost / macOS NSPanel |
| **Click-through** | Doesn't steal focus or intercept input |
| **Fade-out animation** | Appears on keypress, fades after configurable duration |
| **Multi-monitor** | Picks the right output, supports HiDPI |
| **Cross-platform** | Linux (Wayland/X11), Windows, macOS |
| **Settings GUI** | `echoinput --settings` for easy configuration |

---

## ⚙️ Configuration

On first run, creates `~/.config/echoinput/config.toml`:

```toml
# Position: BottomCenter (default), TopLeft, TopRight, TopCenter, BottomLeft, BottomRight, Center
position = "BottomCenter"

# Scale: Small (16pt), Medium (24pt), Large (32pt), ExtraLarge (48pt)
scale = "Medium"

opacity = 0.9
display_duration_ms = 1500
history_length = 3

# Theme: Dark (default), Light, System
theme = "Dark"
```

Or use the settings GUI: `echoinput --settings`

---

## 🏗️ Architecture

```
Input Provider  →  Event Processor  →  MessageBus  →  Overlay Renderer
(evdev/CGEventTap/WH_KEYBOARD_LL)  (grouping/dedup)   (broadcast)    (Cairo/GDI/CoreGraphics)
```

### Crate Structure

| Crate | Purpose |
|-------|---------|
| `input-core` | Types, traits, MessageBus, event processor, config |
| `overlay` | Cross-platform overlay state manager |
| `platform-linux` | evdev capture + Wayland/X11 renderers |
| `platform-windows` | WH_KEYBOARD_LL capture + GDI renderer |
| `platform-macos` | CGEventTap capture + NSPanel/Cairo renderer |
| `settings` | Settings GUI (egui/eframe) |
| `echoinput` | Main binary |

---

## 🖥️ Platform Support

| Platform | Input Capture | Overlay | Status |
|----------|---------------|---------|--------|
| Linux (Wayland) | evdev | wlr-layer-shell + Cairo | ✅ Working |
| Linux (X11) | evdev | X11 override-redirect + Cairo | ✅ Working |
| Windows | SetWindowsHookEx (WH_KEYBOARD_LL) | Layered WS_EX_TRANSPARENT window + GDI | ✅ Working |
| macOS | CGEventTap | NSPanel + Cairo | ✅ Working |

---

## 🔨 Building from Source

### Prerequisites
- Rust 1.75+ (`rustup default stable`)
- Platform-specific dependencies (see Installation sections above)

### Build
```bash
git clone https://github.com/SuperSection/echoinput.git
cd echoinput
cargo build --release
```

### Run
```bash
# Overlay mode
./target/release/echoinput

# Settings GUI
./target/release/echoinput --settings
```

---

## 📦 Creating a Release

Releases are automated via GitHub Actions:

1. Push a version tag: `git tag v0.1.0 && git push origin v0.1.0`
2. GitHub Actions builds for all platforms
3. Artifacts are attached to the GitHub Release

### Manual Release Build

```bash
# Linux
cargo build --release

# Windows (cross-compile from Linux)
cargo build --release --target x86_64-pc-windows-gnu

# macOS (cross-compile from Linux - requires osxcross)
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
```

---

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test --workspace`
5. Submit a PR

---

## 📄 License

MIT OR Apache-2.0

---

## 🙏 Acknowledgments

- Inspired by [KeyViz](https://github.com/mulaRahul/keyviz)
- Built with [Cairo](https://cairographics.org/), [wayland-rs](https://github.com/wayland-rs/wayland-rs), [winit](https://github.com/rust-windowing/winit)
