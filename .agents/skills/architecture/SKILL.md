# EchoInput Architecture

Rules:

- Input capture must be platform-specific.
- Overlay must never depend directly on platform input code.
- Communication must happen through MessageBus.
- Use trait abstractions for all platform implementations.
- Avoid tight coupling between crates.
- New features should be added through interfaces, not direct dependencies.

Current architecture:

Input Provider
↓
Event Processor
↓
MessageBus
↓
Overlay Renderer
↓
Tauri Settings UI

Never bypass MessageBus.
