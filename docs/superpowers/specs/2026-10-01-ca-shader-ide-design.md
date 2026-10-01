# Cellular Automata Shader IDE — Design

Date: 2026-10-01
Status: approved design, pending implementation plan

## Purpose

A desktop tool for personal exploration of 1D and 2D cellular automata where
both the update rule and the visualisation are live-edited WGSL shaders.
The user types a rule, hits apply, and sees the result immediately. Interesting
discoveries are saved as plain-file presets that can be versioned and hand-edited.

Success looks like: Rule 30, Game of Life and a continuous-state automaton all
run from built-in presets within seconds of launching, a shader edit shows up
in the viewport on Ctrl+Enter, and a compile error never crashes or blanks the
display.

## Scope

In scope for v1:

- Single-window desktop app, Rust, `eframe` with the `wgpu` backend.
- One unified 2D grid model. 1D automata are a 2D texture where each step
  writes the next row (space-time diagram).
- Cell state is an `rgba32float` texel.
- Two live-editable WGSL shaders per preset: rule (compute) and render (fragment).
- Shader-declared parameters (`// @param`) exposed as egui sliders.
- Transport: play, pause, single step, steps-per-frame, reset.
- Reset with selectable init pattern (random, single cell, blank) and seed.
- Configurable grid size, default 512x512, up to device limits (~4096).
- Presets as a folder of plain files; built-in examples embedded in the binary.
- Compile error panel with line numbers; last working pipeline stays live.

Out of scope for v1 (noted for later): mouse painting, pan/zoom, second
output window, MIDI/audio input, autocomplete, frame export.

## Architecture

eframe owns the window and the wgpu device/queue. The simulation renders into
an egui central panel through an `egui_wgpu::CallbackTrait` paint callback.
`prepare()` uploads uniforms and dispatches compute steps; `paint()` draws a
fullscreen quad with the render shader into the panel rect.

```
src/
  main.rs          eframe entry, builds App
  app.rs           App: egui layout, wires editor, sim, presets, errors
  sim/
    mod.rs         Simulation: textures, pipelines, step(), reset(), resize()
    pipeline.rs    build compute + render pipelines from WGSL; Result with errors
    uniforms.rs    Globals uniform + user params buffer (bytemuck Pod structs)
    init.rs        init patterns -> CPU buffer -> queue.write_texture
  shader/
    params.rs      parse `// @param` annotations into Vec<ParamSpec>; emit prelude
    highlight.rs   WGSL syntax-highlighting layouter for egui TextEdit
    errors.rs      map naga/wgpu errors to (file, line, col, message)
  preset/
    mod.rs         Preset: load/save folder (preset.toml + rule.wgsl + render.wgsl)
    builtin.rs     embedded examples via include_str!
  viewport.rs      paint callback: prepare() runs N steps, paint() draws quad
presets/           built-in preset sources (also embedded)
```

Per-frame data flow:

1. egui builds the UI. Editor edits mark the corresponding shader dirty.
2. Ctrl+Enter or Apply calls `pipeline::build` for that shader. On success the
   new pipeline replaces the old one and params are re-parsed. On failure the
   errors are shown and the old pipeline stays.
3. Viewport callback `prepare`: write Globals and Params uniforms, dispatch the
   rule compute shader `steps_per_frame` times when playing (or once when a
   single step was requested), ping-ponging between textures A and B.
4. Viewport callback `paint`: fullscreen quad, render fragment shader samples
   the current texture, output to the panel rect.

## Simulation core

### Textures

Two `rgba32float` 2D textures with `STORAGE_BINDING | TEXTURE_BINDING |
COPY_SRC | COPY_DST`. Each step reads `src` and writes `dst`, then swaps.

### Globals uniform

```wgsl
struct Globals {
  size: vec2<u32>,   // grid width, height
  frame: u32,        // steps since reset
  seed: u32,
  time: f32,         // seconds since reset
  mode: u32,         // 0 = 2D, 1 = 1D
  row: u32,          // 1D: row being written this step
  _pad: u32,
}
```

### 1D mode

Compute dispatches only enough workgroups for one row. The rule reads row
`row - 1` (wrapping horizontally) and writes row `row`. The other rows are
copied unchanged from `src` to `dst` by a texture-to-texture copy before the
dispatch, so ping-pong semantics are preserved. When `row` reaches
`size.y - 1`, subsequent steps first scroll the texture up one row (copy rows
1..h into 0..h-1) and keep writing the bottom row, so the diagram flows
continuously. Reset places the init pattern in row 0 and sets `row = 1`.

### User parameters

A uniform buffer of 16 `vec4<f32>` slots. The `@param` parser assigns each
parameter a slot and component and emits a WGSL prelude:

```wgsl
struct Params { threshold: f32, speed: f32, ... }
@group(0) @binding(2) var<uniform> params: Params;
```

Annotation syntax, one per line, anywhere in the rule or render source:

```
// @param name: f32 = 0.5 range 0.0 .. 1.0
// @param name: i32 = 3 range 0 .. 8
// @param name: bool = true
// @param name: vec3<f32> = (1.0, 0.5, 0.2) color
```

Supported types: `f32`, `i32`, `bool`, `vec2<f32>`, `vec3<f32>`, `vec4<f32>`.
`color` on a vec3/vec4 gives a colour picker instead of sliders. Params from
both shaders are merged by name; a type conflict is a compile error. Values
survive recompiles when the name and type match.

### Shader contract

The user writes only the body functions. The app prepends a fixed prelude and
appends the entry point.

Rule shader, user part:

```wgsl
fn rule(pos: vec2<u32>) -> vec4<f32> {
  // cell(x, y) reads src with wraparound; globals and params are in scope
}
```

Prelude provides: bindings for `src` (texture_2d<f32>), `dst`
(texture_storage_2d<rgba32float, write>), `globals`, `params`;
helpers `cell(x: i32, y: i32) -> vec4<f32>` (wraparound read),
`hash(u32) -> u32`, `rand(vec2<u32>, u32) -> f32`.
Appended entry: `@compute @workgroup_size(16, 16)` main that bounds-checks,
in 1D mode fixes `pos.y = globals.row`, calls `rule`, and stores to `dst`.

Render shader, user part:

```wgsl
fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> { ... }
```

Prelude provides the current state texture, a sampler, `globals`, `params`.
Appended: vertex entry producing a fullscreen triangle and fragment entry that
samples the texture (nearest) at `uv` and calls `shade`.

### Reset and init

Init patterns are generated on the CPU with `rand_chacha` seeded from `seed`,
uploaded with `queue.write_texture` to texture A, and `frame`/`row`/`time` are
zeroed. Patterns: `Random { density }`, `SingleCell` (centre; row 0 centre in
1D), `Blank`. Changing width, height or mode resizes both textures and resets.

## UI

```
┌──────────────────────────────────────────────────────────────┐
│ [Preset ▼] [Save] [Save as] │ ▶ ⏸ ⏭ │ steps/frame [1] │ Reset ↺│
├────────────────────────┬─────────────────────────────────────┤
│ Rule (WGSL)   [Apply]  │                                     │
│ ┌────────────────────┐ │                                     │
│ │ fn rule(...) {     │ │          viewport                   │
│ └────────────────────┘ │                                     │
│ Render (WGSL) [Apply]  │                                     │
│ ┌────────────────────┐ │                                     │
│ │ fn shade(...) {    │ │                                     │
│ └────────────────────┘ │                                     │
├────────────────────────┤                                     │
│ Params                 │                                     │
│  threshold ───●── 0.5  │                                     │
│  mode [2D ▼] 512 x 512 │                                     │
├────────────────────────┴─────────────────────────────────────┤
│ Errors: rule.wgsl:12:5  unknown identifier `foo`             │
└──────────────────────────────────────────────────────────────┘
```

- Top bar: preset dropdown (built-ins plus presets found in `./presets`),
  Save, Save As, play/pause, single step, steps-per-frame, Reset.
- Left panel: two collapsible editors using `egui::TextEdit::multiline` with a
  syntax-highlighting layouter (keywords, types, builtins, comments, numbers,
  `@param` lines). Ctrl+Enter applies the focused editor. Below: params
  section regenerated from the parsed annotations; grid settings (mode,
  width, height, init pattern, density, seed).
- Central panel: viewport. Grid is drawn letterboxed, preserving aspect ratio.
- Bottom panel: error list. Clicking an entry moves the editor cursor to that
  line. Empty when the last build succeeded.
- A status line shows FPS and steps/second.

## Presets

```
presets/life/
  preset.toml
  rule.wgsl
  render.wgsl
```

`preset.toml`:

```toml
name = "Game of Life"
mode = "2d"            # or "1d"
width = 512
height = 512
steps_per_frame = 1
seed = 42

[init]
kind = "random"        # random | single | blank
density = 0.3

[params]
threshold = 0.5
color = [1.0, 0.5, 0.2]
```

Built-in presets, embedded via `include_str!` and listed first in the dropdown:

- `rule30` (1D), `rule110` (1D)
- `life` (2D binary)
- Gray-Scott reaction-diffusion (2D continuous, uses RGBA channels)

Load replaces all state (shaders, settings, params) and resets. Save writes
back to the preset's folder; built-ins cannot be saved over, Save on a built-in
behaves as Save As. Save As opens an `rfd` folder picker.

## Error handling

- Shader builds validate with `naga` first so errors carry spans mapped back
  to user line numbers (prelude line count subtracted). Only then is
  `create_shader_module` called, with `push_error_scope` to catch backend
  errors. A failed build never replaces the live pipeline.
- Grid dimensions are clamped to `limits.max_texture_dimension_2d` and the
  compute dispatch limit.
- Preset load failures (missing file, bad TOML, bad shader) are shown in the
  error panel; the previous state stays.
- The app survives device loss only by reporting it; no recovery in v1.

## Testing

Unit tests, no GPU required:

- `shader::params`: parsing every supported type, defaults, ranges, `color`
  flag, duplicate names, type conflicts, slot assignment, prelude output.
- `shader::errors`: span-to-line mapping with prelude offset.
- `preset`: TOML round-trip, loading a folder, built-in listing.
- `sim` index math: 1D row advance and scroll boundaries.
- Shader validation: every built-in preset's assembled rule and render shaders
  pass `naga` parsing and validation. This runs in CI.

Manual verification: run the app, load each built-in, confirm it animates,
introduce a syntax error and confirm the panel shows it and the viewport keeps
running, fix it and confirm it applies.

## Dependencies

`eframe` (wgpu feature), `wgpu`, `naga`, `bytemuck`, `serde`, `toml`, `rand`,
`rand_chacha`, `rfd`, `anyhow`, `log` + `env_logger`. Versions pinned in
`Cargo.toml` to whatever eframe's current wgpu dependency requires so there is
a single wgpu in the tree.
