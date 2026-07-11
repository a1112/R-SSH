# Static Built-in Color Scheme Lookup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Resolve statically keyed `wezterm.color.get_builtin_schemes()` and legacy `wezterm.get_builtin_color_schemes()` lookups as mutable palette sources for `config.colors` and `config.color_schemes`.

**Architecture:** Normalize supported direct/module/require/static-key/function-alias expressions into one canonical zero-argument keyed lookup while preserving the original source offset. Represent the selected built-in scheme as a new palette source that reuses the pinned TOML corpus and existing palette-variable/custom-entry mutation passes.

**Tech Stack:** Rust, existing static Lua query helpers in `rssh-app`, pinned built-in TOML schemes, native effective-config color tests.

---

### Task 1: Add the strict canonical built-in lookup resolver

**Files:**
- Modify: `crates/rssh-app/src/window.rs:68235-68500`
- Test: `crates/rssh-app/src/window.rs` beside the focused `load_scheme_call_resolver_` tests

**Step 1: Write failing resolver tests**

Add a focused test that calls a not-yet-defined central helper such as
`lua_wezterm_builtin_color_scheme_name_from_query_with_static_source`.
Use table-driven sources for:

- `wezterm.color.get_builtin_schemes()['Gruvbox Light']`;
- `wezterm.get_builtin_color_schemes()['Gruvbox Light']`;
- a direct or parenthesized `require('wezterm')` receiver;
- a top-level module alias;
- static-key `color`/`get_builtin_schemes` fields;
- a top-level function alias;
- a top-level static scheme-name variable.

Put a different scheme-name binding after each lookup and expect the name that
was visible at the original lookup offset.

Add a rejection matrix for:

- one or more function arguments;
- a missing closing parenthesis or bracket;
- a missing key lookup;
- an unknown built-in name;
- a dynamically shadowed key variable;
- an additional `[key]`, `.field`, call, operator, or comment/newline-obscured
  continuation after the selected palette;
- storing or using the whole returned map without a keyed selection.

Add positive tail cases for EOF, semicolon, table comma/close, a real next
statement, and a Lua label.

**Step 2: Run the tests to verify RED**

Run:

```powershell
cargo test -p rssh-app builtin_scheme_lookup_resolver_ -- --nocapture
```

Expected: compilation fails because the central resolver does not exist.
Confirm the failures are the intended missing helper/unsupported lookup rather
than test syntax errors.

**Step 3: Implement the canonical parser and normalization**

Add a central resolver that computes the original borrowed query offset once.
It should try:

1. a canonical modern call;
2. a supported module/`require`/static-key receiver normalized to the canonical
   modern call;
3. a supported function alias normalized to the same call.

The canonical parser must:

- parse `wezterm.color.get_builtin_schemes` exactly;
- require a balanced, empty argument list;
- require exactly one following bracket-key lookup;
- accept a quoted/long-bracket literal key or an exact top-level identifier
  whose value resolves statically at the original lookup offset;
- use strict latest-binding resolution for the key so a later dynamic binding
  before the lookup fails closed rather than using an older literal;
- verify the result through `builtin_color_scheme_toml`;
- reject expression continuations after the closing bracket.

Normalize the legacy `wezterm.get_builtin_color_schemes` name to the canonical
modern call. The receiver normalizer must preserve the complete call/key/tail.
The function-alias lookup must clear its candidate on a later unsupported
rebinding, matching the strict alias behavior already used by `load_scheme`.

Extract or rename the current `load_scheme` call-tail predicate into a generic
static value-tail helper and use it for both features; do not duplicate the
continuation grammar.

Never call `lua_source_slice_start_offset` on an owned normalized string.

**Step 4: Run focused and nearby resolver tests**

Run:

```powershell
cargo test -p rssh-app builtin_scheme_lookup_resolver_ -- --nocapture
cargo test -p rssh-app load_scheme_call_resolver_ -- --nocapture
cargo test -p rssh-app static_load_scheme_path_expressions_ -- --nocapture
```

Expected: all pass.

**Step 5: Format, inspect, and commit**

```powershell
cargo fmt --all
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "feat: resolve static built-in scheme lookups"
```

### Task 2: Apply built-in lookups through `config.colors`

**Files:**
- Modify: `crates/rssh-app/src/window.rs:4690-4815`
- Modify: `crates/rssh-app/src/window.rs:66920-67220`
- Modify: `crates/rssh-app/src/window.rs:19180-19200`
- Test: `crates/rssh-app/src/window.rs` near existing built-in and `config.colors` tests

**Step 1: Write failing `config.colors` tests**

Add native effective-config tests for:

```lua
config.colors = wezterm.color.get_builtin_schemes()['Gruvbox Light']
```

and:

```lua
local scheme_name = 'Gruvbox Light'
local scheme = wezterm.color.get_builtin_schemes()[scheme_name]
scheme.background = '#010203'
config.colors = scheme
```

Assert several known base fields or palette slots from `Gruvbox Light` and the
mutated background. Add a later supported built-in reassignment before the
consumer and a later reassignment after the consumer to prove source ordering
and the existing consumer boundary.

**Step 2: Run the tests to verify RED**

Run the two new exact test names. Expected: no built-in source is applied.

**Step 3: Add the `config.colors` source variant**

Add a small `NativeBuiltinColorSchemeAssignment` carrying the resolved scheme
name and optional palette-variable mutation reference. Add a corresponding
`NativeConfigColorsLuaSource` variant.

Extend `lua_config_colors_source_value_from_query` to recognize a direct lookup
before falling back to a variable. Extend
`lua_config_colors_variable_source_before_offset` so a supported top-level
palette assignment can select the built-in source and retain the consumer's
mutation boundary.

In the effective-config application path:

1. call `apply_builtin_color_scheme_overrides` for the resolved name;
2. apply existing palette-variable mutations to both the effective override and
   `colors` snapshot override;
3. populate `overrides.colors` through the unchanged palette snapshot path.

Unknown names or unsupported assignments must not select a built-in source.

**Step 4: Run `config.colors` and built-in regressions**

Run:

```powershell
cargo test -p rssh-app window_app_applies_wezterm_lua_builtin_scheme_lookup_to_config_colors -- --nocapture
cargo test -p rssh-app window_app_applies_wezterm_lua_builtin_scheme_variable_mutations_to_config_colors -- --nocapture
cargo test -p rssh-app builtin_color_scheme -- --nocapture
cargo test -p rssh-app load_scheme -- --nocapture
```

Expected: all pass.

**Step 5: Format, inspect, and commit**

```powershell
cargo fmt --all
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "feat: apply built-in schemes to config colors"
```

### Task 3: Reuse built-in lookups in custom color schemes

**Files:**
- Modify: `crates/rssh-app/src/window.rs:18125-18230`
- Modify: `crates/rssh-app/src/window.rs:18820-19165`
- Test: `crates/rssh-app/src/window.rs` near existing custom built-in and `load_scheme` scheme tests

**Step 1: Write failing custom-scheme tests**

Port the documented WezTerm customization shape:

```lua
local wezterm = require 'wezterm'
local scheme = wezterm.color.get_builtin_schemes()['Gruvbox Light']
scheme.background = '#010203'

return {
  color_schemes = {
    ['Gruvbox Light'] = scheme,
    ['Gruvbox Custom'] = scheme,
  },
  color_scheme = 'Gruvbox Light',
}
```

Assert the selected background mutation plus multiple unchanged base fields and
an ANSI slot. Also add focused coverage for:

- an inline lookup as a `config.color_schemes` table value;
- a direct `config.color_schemes['Name'] = <lookup>` assignment;
- a palette variable followed by existing palette mutations;
- existing selected-entry mutations after the assignment;
- the legacy API and a supported function alias through this consumer.

**Step 2: Run the tests to verify RED**

Run the new exact tests. Expected: the custom scheme source is not found or its
base palette is missing.

**Step 3: Add the custom-scheme source variant**

Add a built-in source to `NativeColorSchemeLuaSource` with:

- resolved built-in name;
- optional palette-variable mutation reference;
- optional selected-entry mutation reference.

Update `with_entry_mutation` and `apply_lua_color_scheme_source_overrides` so
the new variant applies the pinned built-in TOML first, then the same palette
and entry mutation passes used by table/`load_scheme` sources.

Extend `color_scheme_lua_source_value_from_query` for direct lookups and extend
its palette-variable fallback to find a supported built-in lookup assignment.
Ensure `color_scheme_lua_source_value_end_from_query` records the complete keyed
lookup rather than only a call prefix so later entry mutations start at the
correct source offset.

**Step 4: Run custom-scheme and palette mutation regressions**

Run:

```powershell
cargo test -p rssh-app builtin_scheme_lookup -- --nocapture
cargo test -p rssh-app custom_color_scheme -- --nocapture
cargo test -p rssh-app load_scheme -- --nocapture
```

Expected: all pass, including the documented customization test and existing
inline/load-scheme mutation coverage.

**Step 5: Format, inspect, and commit**

```powershell
cargo fmt --all
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "feat: customize built-in color schemes"
```

### Task 4: Update parity records and verify the workspace

**Files:**
- Modify: `docs/research/wezterm-parity-gap.md`
- Modify: `docs/architecture.md`
- Modify: `docs/mvp-6-app-shell-v1.md`

**Step 1: Synchronize the three documentation records**

State that statically keyed modern and legacy built-in scheme lookups can feed
`config.colors` and `config.color_schemes`, directly or through supported
palette variables, while reusing existing palette and selected-entry
mutations.

Keep the boundary explicit: whole-map variables, iteration, dynamic keys,
random/conditional selection, `get_default_colors`, color-object analysis, and
arbitrary Lua remain open.

**Step 2: Verify formatting and scope**

```powershell
cargo fmt --all -- --check
git diff --check
git diff --stat
```

Expected: implementation changes are limited to `window.rs` and documentation
changes are limited to the three named Markdown files.

**Step 3: Run focused, app, and workspace suites**

```powershell
cargo test -p rssh-app builtin_scheme_lookup -- --nocapture
cargo test -p rssh-app load_scheme -- --nocapture
cargo test -p rssh-app
cargo test --workspace
```

Expected: all test targets and doc-tests pass; existing intentionally ignored
real-PTY tests remain ignored.

**Step 4: Commit the documentation**

```powershell
git add docs/research/wezterm-parity-gap.md docs/architecture.md docs/mvp-6-app-shell-v1.md
git commit -m "docs: record static built-in scheme parity"
```
