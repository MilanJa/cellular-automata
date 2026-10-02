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

Each cell is an RGBA float texel; return its next state. Helpers in scope (all read the
previous generation, with wraparound at the edges):

| Rule helper | Meaning |
|------|---------|
| `alive(x, y) -> bool` | is the cell on (`.r > 0.5`)? |
| `neighbours(x, y) -> u32` | live cells among the 8 surrounding cells |
| `neighbours4(x, y) -> u32` | live cells among the 4 orthogonal neighbours |
| `cell(x, y) -> vec4<f32>` | the full previous value |
| `moore_sum(x, y) -> vec4<f32>` | sum of the 8 neighbours |
| `laplacian(x, y) -> vec4<f32>` | 9-point Laplacian, for diffusion |
| `on()`, `off()`, `on_if(flag)` | the on / off cell values |
| `neighbours_hex(x, y) -> u32` | hex grid (odd rows shifted half a cell): the 6 neighbours |
| `neighbours_tri(x, y)`, `neighbours_tri12(x, y)` | triangular grid: 3 edge or 12 corner neighbours; `tri_is_up(x, y)` tells the orientation |
| `prev_cell(x)`, `prev_alive(x)` | 1D mode: the previous row at column `x` |
| `noise(pos) -> f32` | a random `0..1` per cell per step |
| `rand(pos, salt)`, `hash(u)` | deterministic hashing |
| `globals.size`, `.frame`, `.time`, `.seed`, `.mode` | grid and clock |

Life in full:

    fn rule(pos: vec2<u32>) -> vec4<f32> {
        let x = i32(pos.x);
        let y = i32(pos.y);
        let n = neighbours(x, y);
        let me = alive(x, y);
        return on_if((me && (n == 2u || n == 3u)) || (!me && n == 3u));
    }

The **Render** editor maps a cell to a colour:

    fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32>

| Render helper | Meaning |
|------|---------|
| `gray(t)`, `rgb(r, g, b)` | quick RGBA colours |
| `hsv(h, s, v) -> vec3<f32>` | hue, saturation, value in `0..1` |
| `palette(t) -> vec3<f32>` | a smooth gradient for `t` in `0..1` |
| `cell_at(x, y)` | another cell's value, by grid coordinate |
| `hex_cell(uv)`, `hex_local(uv)`, `hex_dist(p)` | hex layout: the cell under a pixel, the position inside it, a hexagon distance for drawing |
| `tri_cell(uv)`, `tri_is_up(x, y)` | triangular layout: the triangle under a pixel and its orientation |

WGSL has no implicit numeric conversions: `pos.x` is `u32`, so write `f32(pos.x)` or
`i32(pos.x) - 1`, and give integer literals a suffix (`3u`). When a shader fails to compile,
the error panel shows the line and, for the common mistakes, a plain-language hint.

## Starting from a template

The **+ New** menu creates an unsaved preset from a commented skeleton that runs as-is:
*2D binary*, *Life-like with B/S switches* (no code, just checkboxes), *1D elementary*,
*2D continuous* (diffusion with a reaction term), and *Render only* (Life with a render shader
to play with). Edit, press Ctrl+Enter, then **Save as…** when you like the result.

Compile errors show in the bottom panel with the line number in *your* source. Click an
error to jump to it. The previous working shader keeps running until the new one compiles.

## Crossfading two rules

**Rule B** is an optional second rule with the same `fn rule(...)` signature. When it is present,
both rules run on every cell and the **Blend A → B** slider mixes their results per cell, so you
can morph Life into HighLife or Seeds live (helper functions defined in A and B must have
different names). Stored as `rule_b.wgsl` plus `blend` in `preset.toml`.

## Post-processing

A third editor, **Post (WGSL)**, runs on the finished picture at viewport resolution:

    fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32>

`color` is the pixel the render shader produced, `scene(uv)` samples the picture anywhere
(linearly filtered), `prev(uv)` is the previous frame's post output for feedback and trails, and
`scene_px()` is one pixel in uv units. It is collapsed and a pass-through until you change it.
Exports go through it too. Neon Life uses it for bloom and trails; the *Post effects* template
shows a CRT look (curvature, chromatic aberration, scanlines, vignette) with feedback. A preset
stores it as `post.wgsl` next to the other two files.

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

Do **not** read the previous generation with `cell(x, i32(pos.y) - 1)` in 1D mode: once the
diagram scrolls, the row being written stays at the bottom and that expression reads two
generations back. `prev_cell` always reads the right row.

## Presets

A preset is a folder containing `preset.toml`, `rule.wgsl` and `render.wgsl`. Folders under
`./presets` appear in the dropdown (click **Rescan** after adding one). **Save** writes back to
a preset's folder; **Save as…** picks a new folder. Built-ins: Rule 30, Rule 110, Game of
Life, Life-like (B/S), Gray-Scott reaction-diffusion, and Neon Life, whose rule is plain Life but
whose render shader draws glowing beads, age colours, halos and fading trails: a good example of
how much the render side alone can do.

## Sharing a scene

**Share** copies a link to the clipboard that reproduces the current scene exactly (shaders,
grid, params, modulations) in the web version: the preset is compressed into the URL fragment,
so nothing is uploaded anywhere. The desktop build produces the same links.

## Animating parameters

Every numeric slider has a **~** button. It opens a small panel where you pick a wave (sine,
triangle, square, saw or smooth noise), a frequency in Hz, an amount (as a fraction of the
slider's range) and a phase. The slider keeps setting the centre; the live value is shown next
to it. By default the wave runs on the wall clock so a paused simulation still animates; tick
*follow simulation* to make it pause with the simulation instead. Modulations are saved with
the preset (`[modulation.<name>]` in `preset.toml`). Neon Life ships with a breathing `glow`.

## Rule explorer

**Explore** opens a window with sixteen random Life-like rules (B/S notation) running live at
low resolution. Click a thumbnail to load that rule as a new preset; **Shuffle** draws a new batch.
Rules with no births or with B0 are skipped.

## Recording an animation

**Image > Record animation…** captures a number of frames while the simulation plays and saves
them as a looping animated PNG (APNG, which browsers and most viewers play). Choose pixels per
cell, frame count and playback rate; frames are captured as fast as the GPU returns them and are
buffered within a 256 MB budget. **Stop** in the status bar ends a recording early and keeps what
was captured.

## Rewind

The strip under the viewport is a timeline of snapshots, taken every *n* steps (choose *n* in
the dropdown; default 5) into a ring that uses about 256 MB of GPU memory, so small grids keep
hundreds of snapshots and large ones a few dozen. Drag the slider to pause and jump to any
snapshot; press Play to continue from there. Reset clears the history.

## Seeding from an image

**Image → Seed grid from image…** (or drop a PNG onto the window) resamples the picture onto the
grid. *Brightness → on/off* thresholds the luminance into live cells; *RGBA → channels* copies
the four colour channels into the four cell channels. Change the mode or threshold and press
**Re-apply image** to try again. Dropping a `*.capreset.toml` bundle imports it as a preset.

## Painting

Drag on the grid with the left mouse button to paint cells, right button to erase. **Brush** (under
Grid) sets the radius in cells and the value written, default `on()` = (1, 0, 0, 1); for a
continuous rule such as Gray-Scott, paint into the channel the rule reads (for example
`0, 1, 0, 1` to seed V). In 1D mode a stroke lands on the most recently written row, which is
what the next generation reads. Painting works while paused.

## Exporting an image

**Image → 1× / 2× / 4×** saves the current grid as a lossless PNG, coloured by the render shader,
at one, two or four pixels per cell (no UI, no letterbox). The desktop asks where to save; the
browser downloads `<preset>-step<N>.png`.

## Controls

| Control | Action |
|---------|--------|
| Ctrl+Enter | apply both shaders |
| Space (when no text field has focus) | play / pause |
| Step | advance one step while paused |
| steps/frame | simulation steps per rendered frame |
| Reset | apply grid settings (mode, size, init pattern, seed) and re-initialise |

## Web version

The same app runs in the browser with WebGPU (recent Chrome or Edge, Firefox 141+, Safari 26+).
Every push to `main` publishes it to https://milanja.github.io/cellular-automata/ .
Add `?preset=rule30` to the URL to start on a built-in.

Differences from the desktop build: presets are saved in the browser's local storage
(the "Browser storage" section of the dropdown) instead of folders, and **Export** / **Import**
move a single `*.capreset.toml` bundle in and out. Export and Import exist on the desktop too, so
a preset can travel between the two.

Build it yourself:

    rustup target add wasm32-unknown-unknown
    cargo install trunk
    trunk serve                                  # http://127.0.0.1:8080, rebuilds on change
    trunk build --release --cargo-profile web    # static files in dist/, size-optimised

The `web` cargo profile uses `opt-level = "z"` with fat LTO: the GPU does the heavy lifting, so
the smaller bundle (faster download and compile) is worth more than CPU micro-optimisation.

## Tests

    cargo test                                        # no GPU needed
    cargo test gpu_tests -- --ignored                 # headless GPU tests (need an adapter)
    cargo clippy --target wasm32-unknown-unknown      # the web build must stay warning-free
