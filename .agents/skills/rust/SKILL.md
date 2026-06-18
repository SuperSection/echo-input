# Rust Development Standards

Requirements:

- Prefer composition over inheritance.
- Avoid unnecessary cloning.
- Use Result<T, E>.
- Use anyhow only at application boundaries.
- Use thiserror for library errors.
- Prefer traits over concrete implementations.
- Keep crates focused.

Every public API requires documentation comments.
