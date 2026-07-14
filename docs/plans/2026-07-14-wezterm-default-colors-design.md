# WezTerm Default Colors Design

## Goal

Add bounded static support for `wezterm.color.get_default_colors()` everywhere
R-SSH already accepts a Lua palette source: `config.colors`, inline and direct
`config.color_schemes` entries, and intermediate palette variables with ordered
mutations. The returned palette must match WezTerm commit `093bf6b`, including
alpha-bearing selection colors and every indexed entry from 16 through 255.

## Upstream contract

At `093bf6b`, `lua-api-crates/color-funcs/src/lib.rs` implements
`get_default_colors()` by converting `wezterm_term::color::ColorPalette::default()`
into `config::Palette`. `term/src/color.rs` defines that default, while
`config/src/color.rs` defines the conversion exposed to Lua.

The observable palette contains:

- foreground from palette index 249 (`#b2b2b2`);
- background and cursor foreground `#000000`;
- cursor background and border `#52ad70`;
- selection foreground `rgba(0, 0, 0, 0)`, represented by R-SSH as the
  explicit `Some(None)` selection-foreground override so rendering retains the
  current cell foreground;
- selection background `Color::Rgba(127, 102, 153, 127)` after WezTerm's
  truncating `SrgbaTuple::as_rgba_u8()` conversion;
- scrollbar thumb `#222222` and split `#444444`;
- the 16 WezTerm/XTerm ANSI colors declared by `ColorPalette::compute_default()`;
- indices 16..231 from the six-level XTerm ramp
  `[0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff]`;
- indices 232..255 from the 24-value grey ramp
  `[0x08, 0x12, ..., 0xee]`.

The Lua conversion inserts all 240 entries from 16 through 255 into
`Palette.indexed`. They are not optional sparse overrides.

## Considered approaches

### 1. A distinct default-palette source (selected)

Add a `DefaultColors` case alongside table, load-scheme, and built-in palette
sources. Construct a `NativeResolvedPalette` from the upstream algorithm and
then reuse the existing ordered mutation reducer and consumer plumbing.

This keeps the upstream default independent from R-SSH's own UI defaults,
preserves alpha, and prevents the full indexed palette from being dropped.

### 2. A synthetic TOML scheme

Generate or embed a TOML scheme and route it through the existing TOML parser.
This would reuse more parsing code, but it introduces a large duplicated data
blob, obscures the upstream generation algorithm, and makes transparent
selection semantics easier to flatten accidentally.

### 3. General Lua execution

Execute Lua and inspect the result table. This would accept more language
constructs, but expands the trust and side-effect surface far beyond the current
bounded static interpreter. It is not required to model this zero-argument API.

## Architecture

### Default palette construction

Add a dedicated constructor, separate from `NativeResolvedPalette::default()`.
The existing `Default` implementation represents R-SSH's effective fallback
configuration and intentionally has different foreground, background, ANSI,
cursor, selection, and indexed values. Reusing it would be incorrect.

The new constructor computes the 256-entry upstream palette from compact ramp
constants. It copies indices 0..15 into `ansi` and `brights`, then stores every
index 16..255 as `Some(Color)` in `NativeResolvedPalette.indexed`. Scalar and
alpha-bearing fields are assigned explicitly from the upstream contract. The
fully transparent selection foreground uses R-SSH's established `Some(None)`
framebuffer semantic; fields not present in WezTerm's converted `Palette`
remain unset/default.

### Lua source recognition

Recognize an exact, zero-argument call after applying the same bounded static
normalization already used for the other WezTerm color APIs:

- `wezterm.color.get_default_colors()`;
- static module aliases, including `local wt = require 'wezterm'`;
- static field keys and direct function aliases;
- comments and parenthesized receivers already supported by the shared call
  parser.

Arguments, method-call syntax, dynamic receivers, dynamically rebound aliases,
and expression tails fail closed. Recognition must evaluate aliases at the call
site rather than by textual substring matching.

### Source and reducer integration

Extend `NativeColorSchemeLuaSource` with `DefaultColors`, carrying the same
optional variable and custom-scheme-entry mutation references as the existing
sources. `apply_lua_color_scheme_source_overrides` starts from the dedicated
upstream palette, converts it to overrides without losing RGBA values or indexed
entries, and replays the existing `LuaPaletteMutationEvent` sequence in source
order.

This gives all existing consumers identical behavior:

- direct `config.colors = wezterm.color.get_default_colors()`;
- `local colors = ...; config.colors = colors`;
- inline and direct custom color-scheme entries;
- scalar, whole-table, indexed-slot, ANSI/bright-slot, tab-bar, and ColorSpec
  mutations supported by the ordered reducer;
- closure capture, alias, rebinding, and lexical-cell validation already used
  for other palette-returning calls.

### Error handling

The parser stays conservative. A syntactically recognizable call with arguments
or an unprovable alias is not treated as the default palette. A selected palette
variable that later escapes or is mutated dynamically invalidates that static
source, matching the current fail-closed identity rules. Unsupported unrelated
palette fields remain ignored consistently with existing table sources.

## Testing

Tests must first fail against the current implementation and then prove:

1. exact scalar and RGBA defaults;
2. all 16 ANSI/bright entries;
3. all 240 indexed entries, plus boundary and ramp samples;
4. direct and statically aliased zero-argument calls;
5. intermediate variables and ordered replacement/slot mutations;
6. inline and direct custom-scheme consumers;
7. rejection of arguments, dynamic aliases, expression tails, escapes, and
   unprovable rebinding;
8. no regression in built-in scheme, load-scheme, palette reducer, and complete
   workspace tests.

The final verification gate is `cargo test -p rssh-app`,
`cargo test --workspace`, `cargo fmt --all -- --check`, and `git diff --check`.

## Completion boundary

This slice completes the bounded static `get_default_colors()` API across all
existing palette consumers. It does not claim arbitrary Lua execution, dynamic
iteration, runtime-generated keys, or general color-object analysis.
