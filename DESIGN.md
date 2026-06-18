# EchoInput

Mission:

Build the best keyboard visualization tool for
Wayland, Hyprland, and modern Linux desktops.

Priorities:

1. Wayland support
2. Performance
3. Reliability
4. Simplicity
5. Cross-platform support

Non-goals:

- Telemetry
- Cloud features
- User tracking
- Heavy dependencies

Architecture:

Input Provider
→ Event Processor
→ MessageBus
→ Overlay Renderer
→ Settings UI
