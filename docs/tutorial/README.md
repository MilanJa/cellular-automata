# Shader cellular automata: a tutorial

This tutorial teaches two things at once: how to write GPU shaders in WGSL, and how to use them
to build cellular automata. It is written for programmers who are comfortable in some language
(Python, JavaScript, Rust, C#...) but have never written GPU code. Nothing about graphics
programming is assumed.

You will use this repository's app, the Cellular Automata Shader IDE, as the lab bench. It runs
on the desktop (`cargo run --release`) and in a WebGPU browser at
<https://milanja.github.io/cellular-automata/>. Chapters 3 to 8 each end in a complete, runnable
preset: open it with **File → New from template**, under the **Tutorial** heading, and the code
from the chapter is in the editors, ready to tweak. The same files live in
[`presets/tutorial/`](../../presets/tutorial/).

## Chapters

| | Chapter | You learn | Preset |
|---|---|---|---|
| 1 | [How it all works](01-how-it-works.md) | what a cellular automaton is, what a shader is, why they fit, and a tour of the app | |
| 2 | [WGSL essentials](02-wgsl-essentials.md) | the language: types, vectors, `let` and `var`, functions, the one rule that bites everyone, and how to read the compiler | |
| 3 | [First rules](03-first-rules.md) | the per-cell mindset, reading neighbours, wraparound, your first slider, your first render shader | Majority vote |
| 4 | [Life](04-life.md) | the Game of Life, birth and survival rules, using spare channels as memory, painting, rewind | Life |
| 5 | [Colour and sliders](05-colour-and-sliders.md) | the render shader in depth: colour helpers, glow, time, every kind of parameter | Colour and sliders |
| 6 | [Randomness and seeds](06-randomness-and-seeds.md) | hashing instead of `rand()`, probabilistic rules, reproducible seeds, writing the start state as code | Randomness and seeds |
| 7 | [Continuous states](07-continuous-states.md) | cells as amounts rather than bits, diffusion, stability, Gray-Scott reaction-diffusion | Reaction-diffusion |
| 8 | [One dimension](08-one-dimensional.md) | space-time diagrams, elementary automata and Wolfram's rule numbers, a continuous 1D system | One dimension |
| 9 | [Beyond](09-beyond.md) | hex and triangular grids, post-processing, blending rules, layers, animation, and where to read on | |

Read them in order the first time: each chapter leans on the one before. Afterwards, the
helper tables in chapters 3, 5, 6 and 8 and the README's reference tables are meant to be
looked up.

## How to use the chapters

Every chapter has a goal, the ideas behind it, code with line-by-line explanations, and a few
exercises marked **Try it**. Do the exercises: the fastest way to learn a shader language is to
break a working shader and read the error. Nothing you do in the editors can damage anything;
the previous working shader keeps running until the new one compiles, and Reset starts over.

Keyboard: **Ctrl+Enter** applies the editors, **Space** plays and pauses, **Ctrl+S** saves.

## Regenerating the pictures

The images in [`images/`](images/) are produced from the presets by

    cargo run --release --example render_docs

which runs each tutorial preset headlessly for a fixed number of steps and exports it the way
**Image → Export PNG** would. Run it after changing a tutorial preset.
