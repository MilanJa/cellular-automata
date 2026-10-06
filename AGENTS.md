# AGENTS.md

A live-coding IDE for cellular automata: users write WGSL, the app wraps it in a prelude (`src/shader/assemble.rs`) and runs it on the GPU. Rust, egui, wgpu; one codebase builds for desktop and the browser. `README.md` is the user manual and the reference for every shader helper.

## Done means

All three are green, as in CI:

- `cargo test --all-targets` (the tests pin the cross-file contracts: README helper tables, uniform layout, embedded presets, old preset files loading)
- `cargo clippy --all-targets -- -D warnings`
- `cargo clippy --target wasm32-unknown-unknown -- -D warnings`

CI has no GPU. When you touch the simulation, the shader assembly or the uniforms, also run `cargo test gpu_tests -- --ignored`.

## Verifying UI changes

egui draws menus and popovers inside the window, so a window capture shows them. On Windows: `cargo build --release`, launch `target/release/cellular-automata.exe` detached, capture it with `PrintWindow` and drive it with real mouse input from a PowerShell script. Kill the exe (`taskkill //IM cellular-automata.exe //F`) before rebuilding, or the build cannot replace it.

## Review

Reviewers apply `CODING_STANDARDS.md`. Pushing to `main` deploys the web build to GitHub Pages.
