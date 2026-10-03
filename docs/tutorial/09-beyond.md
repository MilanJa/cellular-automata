# 9. Beyond

*Goal: a map of everything the first eight chapters left out, each with enough to get started,
and pointers for reading further.*

You can now write a rule, a render, a seed, and reason about binary, continuous and
one-dimensional systems. What follows is the rest of the app, in the order you are most likely
to want it. Each section names a built-in preset or template to open and read; they are all
commented.

## Other grids

Square cells are a convention, not a requirement. Two other tilings are supported, through
neighbourhood helpers in the rule and matching drawing helpers in the render.

**Hexagonal.** Rows are offset by half a cell (the *odd-r* layout) and every cell has six
neighbours: `neighbours_hex(x, y)`. In the render, `hex_cell(uv)` finds the cell under a
pixel, `hex_local(uv)` the position inside it and `hex_dist(p)` a hexagon-shaped distance from
its centre, so you can draw beads with gaps. Open the *Hex Life* built-in; B2/S34 is a classic
hex rule with gliders.

**Triangular.** Cell `(x, y)` points up when `x + y` is even and down otherwise
(`tri_is_up`). `neighbours_tri` counts the three edge neighbours, `neighbours_tri12` the
twelve that share at least a corner. `tri_cell(uv)` finds the triangle under a pixel. Open the
*Triangular grid* template.

The state texture is still a rectangle; only the interpretation of the coordinates changes.
That is a general trick: any neighbourhood you can express as a list of offsets, including
long-range or asymmetric ones, is a few `cell(x + dx, y + dy)` calls.

## Post-processing

The Post editor runs a third shader over the finished picture, at viewport resolution, after
the render shader:

    fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32>

`color` is the pixel the render produced, `scene(uv)` samples the picture anywhere with
linear filtering, `prev(uv)` is the *previous frame's* post output, and `scene_px()` is one
pixel in `uv` units. `prev` is the interesting one: it is feedback, and feedback gives trails,
smearing and persistence that no per-cell rule can. Blur by averaging `scene` at a few offsets,
bloom by adding a blurred copy, trails by mixing in `prev`. It starts as a pass-through and is
collapsed until you change it. *Neon Life* uses it for glow and trails; the *Post effects*
template is a CRT look with curvature, chromatic aberration, scanlines and a vignette.

## Blending two rules

The Rule B editor takes a second rule with the same signature. When it is present both run on
every cell and the **Blend A → B** slider mixes their results, per cell, per step. Blend Life
into HighLife and watch the replicators appear halfway; blend a diffusion rule into Life and
get something soft and strange. Helper functions the two rules define must have different
names, since they share one module. With modulation (below) on the blend slider, the rule
itself can oscillate.

## Layers

Layer B is a whole second preset running alongside the main one at the same grid size. Both
layers' shaders can read the other layer's previous state with `other(x, y)` and
`other_alive(x, y)`, so one automaton can gate, seed or colour another. The *Driven by layer
B* template is Life that can only be born where layer B's `.g` is high; load Gray-Scott as
layer B (Params panel, Layer B section) and Life grows only inside the reaction-diffusion
pattern. A layer B is saved as part of the preset and travels in share links. The *Slither*
built-in pushes this further: layer A is snakes as linked chains of cells that move, eat and
die, layer B is the scent they emit and steer by, and each layer reads the other every step.
Its rule is a worked example of agents in a synchronous grid, where a head and the cell it moves
into must reach the same decision from the same previous state.

## Animating parameters

Every numeric slider has a **~** button. It opens a panel where you pick a wave (sine,
triangle, square, saw or smooth noise), a frequency, an amount as a fraction of the slider's
range, and a phase. The slider becomes the centre and the live value is shown beside it. By
default the wave runs on the wall clock, so a paused simulation still animates; *follow
simulation* makes it pause with the simulation. The same panel offers the **microphone**'s
levels (overall, low, mid, high) as sources once the mic is enabled in the top bar, and
**Learn** binds a **MIDI** knob. Modulations and bindings are saved with the preset.

## Rewind, record, export, share

The timeline under the viewport holds snapshots every few steps within a memory budget; drag to
go back. **Image → Record animation…** captures a run as an animated PNG, holding the simulation
still while each frame is read back so frames are exactly evenly spaced. **Image → Export PNG**
saves the grid at one, two or four pixels per cell through the render and post shaders. **File →
Copy share link** compresses the whole preset into a URL that opens the scene in the web version,
nothing uploaded anywhere. Bundles (`*.capreset.toml`) are the same thing as a file.

## Presets on disk

A preset is a folder:

    preset.toml     name, grid mode and size, steps per frame, seed, init, slider values,
                    modulations, MIDI bindings
    rule.wgsl       the rule
    render.wgsl     the render shader
    post.wgsl       optional post shader
    rule_b.wgsl     optional second rule
    seed.wgsl       optional seed shader (init kind "code")
    layer_b/        optional nested preset

Folders under `presets/` appear in the dropdown on the desktop; the browser keeps presets in
local storage and exchanges bundles. The built-ins are the folders in this repository's
`presets/`, embedded into the binary, and the tutorial presets are `presets/tutorial/`. Reading
them is the best next step after this tutorial; *Neon Life* for what a render shader can do,
*Gray-Scott Discs* for a seed with sliders, *Glider Gun* for a coordinate-list seed.

## Ideas to build

- **Langton's ant.** Needs a single moving agent in a synchronous grid: store the ant's
  presence and heading in a channel, and let each cell decide whether the ant arrives from a
  neighbour. A good test of the per-cell mindset from chapter 6.
- **Wireworld.** Four states (empty, conductor, electron head, electron tail), simple rules,
  and you can build logic gates and clocks by painting.
- **Lenia.** A continuous generalisation of Life with a smooth, ring-shaped neighbourhood and
  a bell-shaped growth function. Big neighbourhoods mean many `cell()` reads per step; the GPU
  does not mind.
- **Sandpiles.** Cells hold grains; a cell with four or more topples one to each neighbour.
  Seeded with a huge pile in the centre it produces fractal patterns.
- **Smooth Life / Primordia.** Diffuse, then threshold, with hysteresis.
- **Audio-reactive anything.** Bind the microphone's low band to Gray-Scott's `feed`.

## Reading on

- The **LifeWiki**, <https://conwaylife.com/wiki>, catalogues thousands of Life patterns with
  their behaviour, and the Life-like rules.
- Stephen Wolfram, *A New Kind of Science* (2002), free online at
  <https://www.wolframscience.com/nks/>, is the long version of chapter 8.
- Karl Sims' reaction-diffusion tutorial, <https://www.karlsims.com/rd.html>, is the clearest
  short account of Gray-Scott, with a map of the feed/kill plane.
- *The Book of Shaders*, <https://thebookofshaders.com>, teaches fragment shaders in GLSL;
  the ideas carry straight over to the render and post shaders here.
- The **WGSL specification**, <https://www.w3.org/TR/WGSL/>, is readable once you know what to
  look for: the built-in functions section is the useful one.
- Bert Chan, *Lenia: Biology of Artificial Life* (2019), for where continuous automata go.

[← One dimension](08-one-dimensional.md) · [Back to the contents](README.md)
