# 1. How it all works

*Goal: understand what a cellular automaton is, what a shader is, why the two fit together so
well, and find your way around the app.*

## Cellular automata in one page

A cellular automaton is a grid of cells. Each cell is in some state: on or off in the simplest
case, a number or a few numbers in general. Time moves in steps. At every step, every cell
computes its next state from one thing only: the current states of the cells near it, its
*neighbourhood*. All cells do this at the same time, using the same rule.

That is the whole definition, and it has three consequences worth holding on to:

- **Locality.** No cell knows anything about the grid as a whole. There is no "centre", no
  "population count", no "step 100 happens here". Whatever large-scale structure appears has to
  emerge from cells talking to their neighbours.
- **Synchrony.** The next generation is computed entirely from the previous one. A cell that
  has already decided its new value does not influence a neighbour that is still deciding. In
  code this means you never update the grid in place: you read from the old grid and write to a
  new one.
- **Uniformity.** The same rule everywhere. The interesting differences between automata are
  all in the rule, the neighbourhood and the state space.

The most famous automaton is Conway's Game of Life (chapter 4): two states, eight neighbours,
and a rule you can say in one sentence, yet it produces gliders, guns, oscillators and, with
enough patience, a working computer. Elementary automata (chapter 8) are even smaller: one
dimension, two states, three neighbours, and Rule 30 is still a decent random number generator.
Reaction-diffusion systems (chapter 7) use continuous states and produce the spots and stripes
you see on animals.

## Shaders in one page

A shader is a small program that a GPU runs many times in parallel, once per *element*. For the
graphics card's usual job the elements are the pixels of a triangle, and the shader decides
their colours. A *compute shader* is the same idea without the triangles: you say "run this
function for every (x, y) in a 512 by 512 grid" and the GPU does, thousands at a time.

Shaders are written in dedicated languages. This app uses WGSL, the WebGPU Shading Language,
which looks like Rust or C and which chapter 2 covers. A shader function cannot do everything a
normal function can. It cannot allocate memory, print, read a file or call back into your
program. Every invocation gets the same inputs (the previous grid, a few global numbers, your
slider values) plus its own coordinates, and produces one output. That is exactly a cellular
automaton's rule.

The fit is close enough that the whole simulation is one compute shader:

    for every cell (x, y), in parallel:
        new_grid[x, y] = rule(x, y, old_grid)

The GPU can run that for a million cells per step, hundreds of steps per second. The same
machine that struggles to update a million cells in a Python loop at one frame per second has
no trouble here, because there is no loop: all the cells are computed at once.

## How this app puts them together

The grid lives in a GPU **texture**: a 2D array where every element, a *texel*, holds four
32-bit floats. The four are named `r`, `g`, `b`, `a` after red, green, blue and alpha, but they
are just four numbers and you may store whatever you like in them. A binary automaton uses `r`
as 0 or 1 and ignores the rest. Life in this tutorial keeps a cell's age in `g`. Gray-Scott
keeps two chemical concentrations in `r` and `g`.

There are two such textures, A and B. A step reads A and writes B; the next step reads B and
writes A. This is called **ping-pong**, and it is how the synchrony rule is honoured: while any
cell's new value is being computed, the old values are all still intact in the other texture.
You never see this; the helpers you call (`cell(x, y)`, `alive(x, y)`, `neighbours(x, y)`)
always read the previous generation.

    step n:      read A ──rule──> write B
    step n + 1:  read B ──rule──> write A
    step n + 2:  read A ──rule──> write B   ...

Three or four shaders cooperate:

1. The **rule** shader, a compute shader, is your automaton: `fn rule(pos) -> vec4<f32>`
   returns the cell's next value. It runs once per cell per step, *steps per frame* times a
   frame.
2. The **render** shader, a fragment shader, turns cells into colours: `fn shade(uv, cell) ->
   vec4<f32>` runs once per *pixel of the viewport*, is handed the cell under that pixel, and
   returns a colour. It never changes the state.
3. An optional **post** shader runs over the finished picture for effects like glow and trails.
4. An optional **seed** shader computes the starting state when you press Reset, instead of the
   random or blank fill.

Everything else in the app is there to feed these shaders and look at their output.

## A tour of the app

Start it with `cargo run --release` or open the web version. Game of Life loads.

**The viewport** in the middle is the grid, drawn by the render shader. Drag with the left mouse
button to paint cells, right button to erase; a small brush toolbar appears while you hover.
Below it, a thin timeline lets you rewind to earlier snapshots.

**The top bar**, left to right:

- *File*: new presets from templates (the tutorial's presets are here), save, export and import
  single-file bundles, copy a share link.
- *Image*: export a PNG, seed the grid from an image, record an animation.
- *View*: the rule explorer, a window of random Life-like rules running live.
- The **preset picker**: built-in examples and your saved presets.
- **Transport**: play/pause, Step (one step while paused), steps per frame, Reset, and the
  **Grid** popover with the grid's mode (2D or 1D), size, initial pattern and random seed. Grid
  settings take effect when you press Reset or the popover's *Apply & reset*.
- *mic* and *MIDI*: live inputs that can drive sliders (chapter 9).
- The status readout: steps per second and the step counter.

**The left panel** holds the editors: Rule, the optional Rule B, Seed, Render and Post, each a
collapsible section with its own Apply button. Pressing **Ctrl+Enter** anywhere applies all of
them. Below the editors, **Params** shows a slider for every `@param` your shaders declare, and
the Layer B section.

**The bottom panel** shows compile errors with the file, line and column, and often a
plain-language hint. Click an error to jump to that line. When everything compiles it shows a
tick and some live statistics instead.

Two habits to pick up now. First, the previous working shader keeps running until the new one
compiles, so apply often and fearlessly. Second, Reset re-initialises from the Grid settings,
and nothing else does; applying a new rule does not clear the grid, which is usually what you
want while iterating.

## What happens in a frame

For completeness, this is one frame of the app, start to finish:

1. Slider values (with any modulation applied) and the clock are uploaded to the GPU.
2. The rule shader runs *steps per frame* times, ping-ponging between the two textures. In 1D
   mode each run writes one new row instead of the whole grid.
3. The render shader draws the current texture into a picture the size of the viewport.
4. The post shader, if any, processes that picture.
5. The picture is shown, and statistics (population, change) are read back for the status line.

You now know enough to read a rule. Chapter 2 is about writing one.

[Next: WGSL essentials →](02-wgsl-essentials.md)
