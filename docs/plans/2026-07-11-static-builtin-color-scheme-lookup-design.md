# Static Built-in Color Scheme Lookup Design

## Goal

Match WezTerm's documented static color-customization workflow by recognizing
statically keyed lookups from `wezterm.color.get_builtin_schemes()` and its
legacy `wezterm.get_builtin_color_schemes()` alias.

The supported result can feed `config.colors` or a selected
`config.color_schemes` entry directly or through a top-level palette variable,
then reuse the color mutation support R-SSH already applies to inline tables and
`load_scheme` results.

## Upstream behavior

Pinned WezTerm registers both APIs as zero-argument Lua functions that return a
clone of the built-in color-scheme map. The map is keyed by scheme name and each
value is a mutable palette table. The documented customization example obtains
`['Gruvbox Light']`, changes its background, and stores the result in
`config.color_schemes` under the original or a new name.

R-SSH already carries the pinned built-in TOML corpus and resolves more than one
thousand canonical names and aliases through `builtin_color_scheme_toml`. It
also already applies supported scalar, ANSI/bright/indexed, tab-bar, and
ColorSpec mutations to palette variables and selected custom scheme entries.
The missing link is static recognition of the Lua lookup expression.

## Chosen scope

Support expressions shaped as:

```lua
wezterm.color.get_builtin_schemes()['Gruvbox Light']
wezterm.color.get_builtin_schemes()[scheme_name]
wezterm.get_builtin_color_schemes()['Gruvbox Light']
```

Also recognize the receiver and function indirections already supported by the
static Lua compatibility layer:

- direct or parenthesized `require('wezterm')` receivers;
- top-level module aliases;
- static-key access to `color`, `get_builtin_schemes`, and the legacy function;
- top-level function aliases that still resolve to one of the two APIs.

The function call must have zero arguments. The selected scheme key may be a
quoted or long-bracket literal or an exact top-level static string variable at
the lookup offset. The selected name must exist in R-SSH's pinned built-in
scheme corpus.

Keep these forms outside this slice:

- iterating the returned map with `pairs`, sorting, random selection, or other
  general Lua table operations;
- storing the whole returned scheme map and indexing it later;
- dynamic keys, conditional key selection, helper calls, or arbitrary Lua;
- `wezterm.color.get_default_colors()`;
- color-object analysis such as `wezterm.color.parse(...):hsla()`;
- metadata enumeration or mutation of the global built-in map.

## Architecture

### Canonical keyed-lookup parser

Add one canonical parser for a normalized
`wezterm.color.get_builtin_schemes()[key]` expression. It must:

1. parse an exact supported function name;
2. parse a balanced parenthesized argument list and require zero arguments;
3. parse one bracket lookup whose key is a literal or top-level static string
   variable;
4. reject additional indexing, field access, calls, operators, or other
   expression continuations after the selected palette;
5. verify the resolved name through `builtin_color_scheme_toml`.

The outer resolver records the original expression's source offset once.
Direct/module/static-key/function-alias normalization must pass that same offset
to the canonical parser so a scheme-name variable resolves at the lookup point,
not at an alias declaration or an owned normalized string.

The modern and legacy API names normalize to the same canonical form. The
normalizers remain thin: they identify a supported receiver/function path and
retain the complete call/key/tail for the canonical parser.

### Palette source variants

Add a built-in scheme source to both existing palette-source paths:

- `NativeConfigColorsLuaSource` for `config.colors`;
- `NativeColorSchemeLuaSource` for custom/selected `config.color_schemes`
  entries.

The source stores the resolved built-in name plus the same optional palette
variable and custom-entry mutation references used by table and `load_scheme`
sources. Applying the source calls `apply_builtin_color_scheme_overrides`, then
applies supported palette-variable mutations and selected-entry mutations in
their existing source-order ranges.

No built-in palette is mutated globally. Every consumer starts from the pinned
TOML scheme again, matching WezTerm's cloned-map behavior for the supported
static subset.

### Variable and consumer routing

Extend the central palette value resolvers rather than adding consumer-specific
parsers:

- direct `config.colors = <lookup>` resolves immediately;
- a top-level palette assignment such as `local scheme = <lookup>` is available
  to the existing `config.colors = scheme` and
  `config.color_schemes['Name'] = scheme` flows;
- inline `config.color_schemes = { ['Name'] = <lookup> }` and direct entry
  assignments use the same source variant;
- supported palette and custom-entry mutations retain their existing ordering
  and evaluation boundaries.

Unsupported or unknown lookups return no static palette and do not fall back to
a different built-in name.

## Data flow

1. A colors consumer encounters a direct lookup or traces a palette variable to
   a supported assignment.
2. The keyed-lookup resolver records the original source offset.
3. Supported receiver or function aliases normalize to the canonical modern
   call without changing that offset.
4. The canonical parser validates zero arguments, resolves the static key, and
   verifies the built-in name.
5. The consumer applies the pinned built-in TOML colors.
6. Existing palette-variable and selected-entry mutation passes apply on top.
7. The resulting overrides enter the existing effective palette path.

## Error handling

An unknown name, dynamic key, nonzero argument list, incomplete lookup, or
continued expression returns no static source. This is a bounded interpreter;
it must not choose an arbitrary scheme or use a stale key when a lookup cannot
be proven.

The built-in TOML parser and existing color override code remain the single
source of truth for palette fields and parse failures.

## Testing

Use TDD with four layers:

1. Focused resolver tests cover canonical modern and legacy calls, module and
   function aliases, static-key fields, a static scheme-name variable, original
   lookup offsets, unknown names, dynamic keys, invalid arguments, and
   continuation tails.
2. Source-routing tests cover direct and palette-variable forms for both
   `config.colors` and `config.color_schemes`.
3. A real effective-config test ports the documented Gruvbox customization:
   load `Gruvbox Light`, mutate the background, publish it as a custom scheme,
   select it, and assert the mutation plus unchanged base colors/palette slots.
4. Existing built-in selection, inline color tables, `load_scheme`, palette
   variable mutation, and selected custom-scheme mutation regressions remain
   green before the full app and workspace suites run.

Update the parity tracker, architecture record, and MVP record with the same
bounded claim, keeping map iteration, dynamic selection, default colors, and
color analysis explicitly open.
