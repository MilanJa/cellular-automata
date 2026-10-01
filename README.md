# Cellular Automata Shader IDE

Live-code 1D and 2D cellular automata as WGSL compute shaders and render them with
WGSL fragment shaders. Rust, egui, wgpu. Everything runs on the GPU.

## Run

    cargo run --release
    cargo run --release -- --preset rule30        # start on a built-in (rule30, rule110, life, gray_scott)
    cargo run --release -- --preset presets/life  # or on a preset folder

## Writing a rule

Edit the **Rule** editor and press **Ctrl+Enter** (or click Apply). You write one function:

    fn rule(pos: vec2<u32>) -> vec4<f32>

Each cell is an RGBA float texel; return its next state. Helpers in scope:

| Name | Meaning |
|------|---------|
| `cell(x, y)` | previous state at `(x, y)`, wrapping at the edges |
| `prev_cell(x)` | 1D mode: the previous row at column `x` |
| `rand(pos, salt)` | deterministic hash noise in `0..1`, seeded by the grid seed |
| `hash(u)` | integer hash |
| `globals.size`, `globals.frame`, `globals.time`, `globals.seed`, `globals.mode` | grid and clock |

The **Render** editor maps a cell to a colour:

    fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32>

It also has `cell(x, y)` and `globals`.

Compile errors show in the bottom panel with the line number in *your* source. Click an
error to jump to it. The previous working shader keeps running until the new one compiles.

## Parameters

Declare live sliders in either shader with a comment:

    // @param threshold: f32 = 0.5 range 0.0 .. 1.0
    // @param n: i32 = 3 range 0 .. 8
    // @param on: bool = true            // in WGSL: params.on != 0u
    // @param tint: vec3<f32> = (1, 0.5, 0.2) color

They appear under **Params** and are read as `params.<name>`. Up to 16 params; values
survive re-applies as long as the name and type stay the same.

## 1D automata

Switch **Grid → mode** to *1D (space-time)*. The grid becomes a space-time diagram: each step
writes the next row below the previous one, and once the bottom is reached the diagram scrolls.
Use `prev_cell(x - 1)`, `prev_cell(x)`, `prev_cell(x + 1)` for an elementary automaton.

## Presets

A preset is a folder containing `preset.toml`, `rule.wgsl` and `render.wgsl`. Folders under
`./presets` appear in the dropdown (click **Rescan** after adding one). **Save** writes back to
a preset's folder; **Save as…** picks a new folder. Built-ins: Rule 30, Rule 110, Game of
Life, Gray-Scott reaction-diffusion.

## Controls

| Control | Action |
|---------|--------|
| Ctrl+Enter | apply both shaders |
| Space (when no text field has focus) | play / pause |
| Step | advance one step while paused |
| steps/frame | simulation steps per rendered frame |
| Reset | apply grid settings (mode, size, init pattern, seed) and re-initialise |

## Tests

    cargo test                                  # no GPU needed
    cargo test gpu_tests -- --ignored           # headless GPU tests (need an adapter)
