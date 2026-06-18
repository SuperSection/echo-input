# Wayland Guidelines

Wayland is the primary target.

Requirements:

- Prefer wlr-layer-shell.
- Overlay must be click-through.
- Overlay must never receive keyboard focus.
- Overlay should support Hyprland and Sway first.
- Avoid compositor-specific hacks unless isolated behind interfaces.
- Assume GNOME lacks layer-shell support.
- Provide fallback strategy for GNOME.

Design for Wayland first.
