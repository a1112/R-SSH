# Built-in Scheme Map Consumers Design

## Goal

Complete static support for palettes selected from the map returned by
`wezterm.color.get_builtin_schemes()` and the legacy
`wezterm.get_builtin_color_schemes()` API across every existing R-SSH palette
consumer.

## Supported forms

The bounded Lua interpreter will support a top-level whole-map binding followed
by a static keyed lookup in:

- `config.colors` assignments;
- inline and direct `config.color_schemes` entries;
- a top-level intermediate palette variable whose existing supported mutations
  are later consumed by either path.

Module, static field, and function aliases already accepted by the direct
built-in lookup path remain supported. Scheme keys may be quoted, long-bracket
literals, or top-level static string variables resolved at the lookup point.

## Architecture

Extract the whole-map keyed lookup logic into a shared resolver that returns the
selected built-in name plus the original lookup position. Both config-colors
and custom-scheme source resolvers route through it before falling back to plain
palette variables.

For an intermediate palette assignment such as
`local scheme = schemes['Gruvbox Light']`, the existing built-in palette source
records `scheme` as its palette variable and uses the consumer position as the
mutation boundary. Mutations apply only to that selected palette clone. The map
variable itself is never treated as a palette and no global built-in palette is
modified.

Binding resolution retains the existing source-order rules: the latest
provable top-level binding before consumption wins, and a lookup captures the
map and key at its own assignment position.

## Rejected forms

Dynamic keys, iteration, helper-local map bindings, conditional map selection,
non-zero-argument calls, dynamically reassigned maps or keys, and arbitrary Lua
execution remain unsupported and fail closed. Rebinding after a proven lookup
does not retroactively change the selected palette.

## Testing

Use TDD to cover inline and direct custom-scheme entries, an intermediate
palette variable with mutation, modern and legacy API aliases, static key
variables, binding-time capture, and negative dynamic/rebinding cases. Retain
the existing direct `config.colors` whole-map regression, then run the focused
tests, the serial `rssh-app` suite, workspace tests, and `git diff --check`.

Update the parity tracker so whole-map variables are no longer listed as wholly
open; iteration and dynamic selection remain open boundaries.
