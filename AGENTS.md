# AGENTS.md

A live-coding IDE for cellular automata: users write WGSL, the app wraps it and runs it on the GPU. Rust, egui, wgpu; one codebase builds for desktop and the browser (WebGPU). `README.md` is the user manual and the reference for every shader helper.

## Done means

A change is done when all three are green, because CI gates on all three:

- `cargo test --all-targets`
- `cargo clippy --all-targets -- -D warnings`
- `cargo clippy --target wasm32-unknown-unknown -- -D warnings` (catches native-only code leaking into the web build)

GPU behaviour is covered by `#[ignore]`d tests in `src/sim/simulation.rs` (`mod gpu_tests`); run `cargo test gpu_tests -- --ignored` whenever you touch the simulation, the shader assembly or the uniforms. CI has no GPU, so these never run there.

## Two targets

Desktop and web differ through `#[cfg(target_arch = "wasm32")]`. Keep the split at the edges: each platform-dependent subsystem has a `mod.rs` with the shared API plus `*_native.rs` / `*_web.rs` (or `native.rs` / `web.rs`) implementations (`platform/`, `audio/`, `midi/`). Native-only crates (`rfd`, `cpal`, `midir`, `pollster`) live under the non-wasm target in `Cargo.toml`. `rand` has default features off so `getrandom` stays out of the wasm build; seed from the preset, never from OS entropy.

## Shader contract

User code is spliced between a prelude and an epilogue in `src/shader/assemble.rs`. The contract with users lives in three places that move together:

- **Helpers**: a helper added to or changed in a prelude also goes in the README's helper tables. The rule and render preludes each define their own `other` / `cell` / `tri_is_up`; change both.
- **Uniforms**: `GLOBALS_WGSL` and `src/sim/uniforms.rs::Globals` are one layout written twice; edit both and keep the size test passing (48 bytes, padded to 16).
- **Error lines**: compile errors are reported in the user's line numbers via `user_line_offset`. Any prelude edit shifts it; the offset is computed from the text, so keep it that way rather than hard-coding counts.

Plain-language error hints live in `src/shader/hints.rs`.

## Presets

A preset is a folder with `preset.toml`, `rule.wgsl`, `render.wgsl`, and optionally `post.wgsl` / `rule_b.wgsl`. Built-ins and templates are compiled in with `include_str!` from `presets/`, so a new one needs a folder **and** an entry in `BUILTINS` or `TEMPLATES` in `src/preset/builtin.rs`. The tests there compile every embedded preset through naga, so a broken shader fails `cargo test`. Templates are teaching material: commented, and runnable unchanged.

New `preset.toml` fields need `#[serde(default)]` so older presets, saved bundles and share links (`src/preset/share.rs`) still load.

## UI

All colours, spacing and section headers come from `src/app/theme.rs`; take a named constant from there (add one if needed) so panels stay consistent.

## Verifying UI changes

egui renders menus and popovers inside the window, so a window screenshot shows them. On Windows: `cargo build --release`, launch `target/release/cellular-automata.exe` detached, capture with `PrintWindow`, and drive it with real mouse input from a PowerShell script. Kill the running exe (`taskkill //IM cellular-automata.exe //F`) before rebuilding, or the build cannot replace it.

## Commits

Conventional prefixes, matching recent history: `feat:`, `fix:`, `style:`, `docs:`, `test:`. Pushing to `main` deploys the web build to GitHub Pages.
