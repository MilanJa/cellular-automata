# Coding standards

Read during review. These are judgement calls; the mechanical rules are enforced by `cargo test` and `clippy.toml`, so they are not repeated here.

## Platform split stays at the edges

Desktop/web differences live behind a subsystem's `mod.rs`, implemented in `native.rs` / `web.rs` or `*_native.rs` / `*_web.rs` (`platform/`, `audio/`, `midi/`). Shared code calls the shared API. Flag a `#[cfg(target_arch = "wasm32")]` that appears in app, sim or shader logic when the difference could sit behind such an API.

## Shader preludes move together

The rule and render preludes in `src/shader/assemble.rs` each define their own `other`, `other_alive`, `cell` and `tri_is_up`. A change to one copy is made to the other, unless the two stages genuinely need different behaviour (the diff should say why). The line offset that maps compile errors to user lines is computed from the assembled text; flag hard-coded line counts.

## Templates teach

Templates in `presets/templates/` are the first code a new user reads. Each explains what it does in comments, uses the README's helpers rather than raw texture reads, and shows the one idea it is named for. Flag a template that has grown into a showcase.

## Plain-language errors

User-facing messages (error hints in `src/shader/hints.rs`, status lines, dialogs) say what to do next in words a beginner understands, and name the helper or menu involved.
