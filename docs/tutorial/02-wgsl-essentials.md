# 2. WGSL essentials

*Goal: learn enough of the WebGPU Shading Language to read and write a rule without fighting
the compiler. If you know Rust or C, most of this will look familiar; the sections on literals
and conversions are the ones to read carefully whatever your background.*

WGSL is a small, strict, statically typed language. There are no classes, no strings, no
exceptions, no heap, no recursion and no printing. What is left is numbers, vectors of numbers,
functions and control flow. That is all a cellular automaton needs.

## Scalars

| Type | Meaning | Literal |
|---|---|---|
| `bool` | true or false | `true`, `false` |
| `i32` | 32-bit signed integer | `3i` or just `3` |
| `u32` | 32-bit unsigned integer | `3u` |
| `f32` | 32-bit float | `3.0f`, `3.0`, `3e-2` |

Notice the suffixes. A literal with a suffix has that exact type: `3u` is a `u32`. A literal
*without* a suffix (`3`, `3.0`) is an *abstract* number that adapts to whatever type the
surrounding expression needs. This is convenient and it is also where most compile errors come
from, so hold that thought until the conversions section.

## Vectors

A vector is two, three or four scalars of the same type: `vec2<f32>`, `vec3<i32>`,
`vec4<f32>` and so on. Cells are `vec4<f32>`, positions are `vec2<u32>`, colours are
`vec3<f32>` or `vec4<f32>`.

    let p = vec2<i32>(3, -1);          // construct from components
    let c = vec4<f32>(1.0, 0.5, 0.0, 1.0);
    let z = vec3<f32>(0.0);            // all components the same
    let q = vec2<f32>(p);              // convert a whole vector: (3.0, -1.0)

Components are reached by name. `.x .y .z .w` and `.r .g .b .a` are the same four slots under
two names; use whichever reads better. Several components at once make a new vector
(*swizzling*):

    c.r          // 1.0
    c.rgb        // vec3<f32>(1.0, 0.5, 0.0)
    c.xy         // vec2<f32>(1.0, 0.5)
    p.yx         // vec2<i32>(-1, 3)

Arithmetic on vectors is componentwise, and a vector can be multiplied or divided by a scalar:

    let a = vec2<f32>(1.0, 2.0) + vec2<f32>(10.0, 20.0);   // (11.0, 22.0)
    let b = vec3<f32>(0.2, 0.5, 1.0) * 0.5;                 // (0.1, 0.25, 0.5)

## Declarations

    let n = neighbours(x, y);      // immutable: cannot be assigned again
    var count = 0u;                // mutable
    count = count + 1u;
    count += 1u;                   // compound assignment works too
    const WIDTH = 7;               // module-scope constant (outside any function)

Types are inferred from the initialiser, as above, or written out: `var v: f32 = 0.0;`. Prefer
`let`; reach for `var` only when you assign more than once, as in a loop or when building a
colour up in stages. Assigning to a `let` is a compile error with the message *invalid
left-hand side of assignment*.

## Functions

    fn count_cross(x: i32, y: i32) -> u32 {
        return u32(alive(x - 1, y)) + u32(alive(x + 1, y))
             + u32(alive(x, y - 1)) + u32(alive(x, y + 1));
    }

Parameters are typed, the return type comes after `->`, and every path through the function
must `return` a value of that type. Functions at module scope can be written in any order; the
app's helpers (`alive`, `cell`, `neighbours`...) are ordinary functions defined just above your
code. You can define as many helpers of your own as you like.

The two functions the app calls are:

    fn rule(pos: vec2<u32>) -> vec4<f32>    // the next value of the cell at pos
    fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32>   // the colour of a pixel

## Control flow

    if (n == 3u) {
        return on();
    } else if (n > 5u) {
        return off();
    }

    for (var i = 0; i < 8; i++) { ... }

    while (v > 1.0) { v -= 1.0; }

    loop {                       // an infinite loop with explicit break
        if (done) { break; }
    }

The parentheses around conditions are optional in WGSL; this tutorial writes them anyway.

Shaders dislike branches less than folklore says, but a branchless pick is often clearer
anyway. `select` chooses between two values:

    let age = select(0.0, old_age + 1.0, stays_alive);
    //              ^if false  ^if true   ^condition

Read it as "select the second argument if the condition holds, else the first". Both branches
are evaluated, so do not put anything expensive in a `select`.

## The one rule that bites everyone: no implicit conversions

WGSL never converts between concrete numeric types on its own. Not `i32` to `f32`, not `u32` to
`i32`, not `i32` to `u32`. Every one of these is an error:

    let x = pos.x - 1;           // pos.x is u32; the literal becomes u32; fine so far...
    let y = pos.y * 0.5;         // ERROR: u32 times a float
    if (n == 3) { ... }          // fine: 3 adapts to u32
    if (n == params.count) ...   // ERROR if params.count is i32 and n is u32
    let v = cell(x, y).r + 1;    // fine: 1 adapts to f32
    return vec4<f32>(n, 0, 0, 1) // ERROR: n is u32, the components must be f32

The fix is always the same: convert explicitly with the type's name used as a function.

    let fx = f32(pos.x) * 0.5;
    if (n == u32(params.count)) { ... }
    return vec4<f32>(f32(n), 0.0, 0.0, 1.0);

Two more conversions you will use constantly:

- `u32(flag)` turns a `bool` into `1u` or `0u`, so booleans can be added up. `f32(flag)` gives
  `1.0` or `0.0`.
- `i32(pos.x)` turns the unsigned coordinate into a signed one, so that `x - 1` at the left
  edge is `-1` rather than wrapping around to four billion. The helpers take `i32` coordinates
  for exactly this reason, and every rule starts with these two lines:

      let x = i32(pos.x);
      let y = i32(pos.y);

The abstract literals are what keeps this bearable: `n == 3u` and `n == 3` both work because
`3` adapts, and `v * 0.5` works for any `f32` `v`. Only *two concrete values of different
types* refuse to meet.

## Useful built-in functions

Mathematics: `abs`, `min`, `max`, `clamp(v, lo, hi)`, `floor`, `ceil`, `fract` (the part after
the decimal point), `sqrt`, `pow`, `exp`, `log`, `sin`, `cos`, `atan2`. All of these work on
vectors componentwise.

Interpolation: `mix(a, b, t)` is `a + (b - a) * t`, the workhorse of colouring.
`smoothstep(lo, hi, v)` ramps from 0 to 1 as `v` goes from `lo` to `hi`, with soft ends.
`step(edge, v)` is 0 below `edge` and 1 at or above.

Geometry: `length(v)`, `distance(a, b)`, `dot(a, b)`, `normalize(v)`.

Integers: `<<`, `>>`, `&`, `|`, `^` work on `u32` and `i32`, and `%` is the remainder.

## The cell

A cell is a `vec4<f32>`. The convention the helpers assume for *binary* automata is that `.r`
is 1.0 for on and 0.0 for off, and `alive(x, y)` tests `.r > 0.5`. Three helpers build cells:

    on()          // vec4<f32>(1.0, 0.0, 0.0, 1.0)
    off()         // vec4<f32>(0.0, 0.0, 0.0, 1.0)
    on_if(flag)   // on() when flag is true, else off()

Nothing forces the convention on you. A cell may hold any four floats, and chapters 4, 5 and 7
use the other channels for age, embers and chemical concentrations. Keep `.a` at 1.0 unless you
have a reason not to; the render shader ignores it, but some exports do not.

## What a rule can see

Inside `rule` you have:

- `pos`, your cell's coordinates, `vec2<u32>`, with `(0, 0)` at the top left.
- The previous generation, through `cell(x, y)`, `alive(x, y)`, `neighbours(x, y)` and friends
  (chapter 3 lists them). Coordinates wrap around at the edges.
- `params.<name>` for every slider you declare (chapter 3 introduces them, chapter 5 covers
  them fully).
- `globals.size` (the grid size, `vec2<u32>`), `globals.frame` (the step counter),
  `globals.time` (seconds since Reset), `globals.seed` (the Grid's random seed) and
  `globals.mode` (`1u` in 1D mode).

What a rule cannot see is any other cell's *new* value, anything about the grid as a whole, or
anything from outside the GPU. That is the automaton's locality, enforced by the hardware.

## Reading the compiler

When something does not compile, the bottom panel lists each problem as

    rule.wgsl:12:18  no definition in scope for identifier: `neigbours`
       ↳ Check the spelling. Rule helpers: cell, alive, neighbours, ...

The line and column are in *your* text, not in the generated code around it. Click the entry to
put the cursor there. The second line is a hint added for the mistakes beginners make most; the
first line is the compiler's own message, which is precise but terse. The ones you will meet:

| Message contains | What it means |
|---|---|
| *automatic conversions cannot convert* or *can't work with* | you mixed two numeric types; convert one with `f32()`, `i32()` or `u32()` |
| *does not match the declared return type* | a path through the function returns the wrong type or nothing; `rule` must return `vec4<f32>` |
| *no definition in scope for identifier* | a typo, or a helper that does not exist in this editor (render helpers are not available in the rule and vice versa) |
| *expected `;`* | the previous statement is missing its semicolon |
| *expected `)`, found "="* | you wrote `=` where `==` was meant |
| *invalid left-hand side of assignment* | assigning to a `let`; make it a `var` |
| *Requires N arguments* | wrong number of arguments to a helper; the 2D helpers take `(x, y)` |

There is no debugger and no `print`. The debugging tool is the render shader: when a rule
misbehaves, write the suspicious quantity into an unused channel (`.b`, say) and colour by it.
Chapter 5 shows how.

## Things WGSL does not have

So you stop looking for them: no implicit conversions (said enough), no recursion, no
variable-length arrays, no strings, no `null`, no exceptions, no global mutable state shared
between cells, no reading your own cell's *new* value. Integer division by zero and out-of-range
array indexing do not crash; they produce an unspecified value, which can make bugs quiet. Float
overflow produces infinity, which propagates; `clamp` is your friend in continuous rules.

That is the language. Everything from here on is about what to say in it.

[← How it all works](01-how-it-works.md) · [Next: First rules →](03-first-rules.md)
