# WezTerm Default Colors Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add exact bounded-static support for `wezterm.color.get_default_colors()` in every Lua palette consumer already supported by R-SSH.

**Architecture:** Model the call as a distinct palette source, generate WezTerm commit `093bf6b`'s default palette from compact ANSI, color-cube, and grey-ramp constants, then feed that source through the shared ordered palette-mutation reducer. Reuse the existing static module/function alias normalization and lexical-cell identity checks; calls with arguments, expression tails, dynamic rebinding, or escaped identities fail closed.

**Tech Stack:** Rust 2024, the bounded Lua parser in `crates/rssh-app/src/window.rs`, the pinned upstream checkout under `refs/wezterm`, Cargo test, rustfmt.

---

### Task 1: Lock the upstream palette contract as a RED test

**Files:**
- Modify: `crates/rssh-app/src/window.rs` near `NativeResolvedPalette` and its test module

**Step 1: Add an authoritative palette test**

Add `wezterm_default_colors_palette_matches_pinned_upstream`. Have it call the
dedicated constructor that will be introduced in this slice and assert:

- foreground `#b2b2b2`;
- background and cursor foreground `#000000`;
- cursor background and border `#52ad70`;
- selection foreground `Some(None)`, R-SSH's explicit transparent-selection
  foreground representation;
- selection background `Color::Rgba(127, 102, 153, 127)`;
- scrollbar thumb `#222222` and split `#444444`;
- ANSI and bright arrays exactly equal the 16 colors in pinned
  `term/src/color.rs`;
- `indexed[0..16]` are unset and every entry in `indexed[16..256]` is set;
- all 216 cube entries equal the formula using
  `[0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff]`;
- all 24 grey entries equal `8 + 10 * (index - 232)`;
- explicit samples cover 16, 17, 21, 22, 51, 52, 88, 124, 160, 196, 231,
  232, 249, and 255;
- fields absent from WezTerm's converted `Palette` remain unset.

Use loop assertions for the full 240-entry indexed table, not only samples.

**Step 2: Verify RED**

Run:

```powershell
cargo test -p rssh-app window::tests::wezterm_default_colors_palette_matches_pinned_upstream -- --exact --nocapture
```

Expected: compilation fails because the dedicated constructor does not exist.

**Step 3: Commit the test only**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "test: lock WezTerm default color palette"
```

### Task 2: Generate the exact pinned default palette

**Files:**
- Modify: `crates/rssh-app/src/window.rs` near `NativeResolvedPalette::default`

**Step 1: Implement a distinct constructor**

Add `native_wezterm_default_colors_palette() -> NativeResolvedPalette`. Do not
reuse `NativeResolvedPalette::default()`: that value is R-SSH's own fallback.

Define the 16 fixed ANSI colors exactly as upstream. Build a fresh
`[Option<Color>; 256]`, populate 16 through 231 with the six-level cube, and
populate 232 through 255 with the grey formula. Assign the scalar and RGBA
fields explicitly and leave fields absent from WezTerm's converted `Palette`
unset.

Keep the generation arithmetic in `u8`-safe form and document the pinned
upstream files and commit beside the helper.

**Step 2: Make the authoritative test GREEN**

Run:

```powershell
cargo test -p rssh-app window::tests::wezterm_default_colors_palette_matches_pinned_upstream -- --exact --nocapture
cargo fmt --all -- --check
```

Expected: both commands pass.

**Step 3: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: generate WezTerm default color palette"
```

### Task 3: Recognize exact zero-argument calls and aliases

**Files:**
- Modify: `crates/rssh-app/src/window.rs` near the static `wezterm.color` call resolvers and tests

**Step 1: Add RED resolver tests**

Add `default_colors_call_resolver_accepts_static_zero_argument_forms` covering:

```lua
wezterm.color.get_default_colors()
require('wezterm').color.get_default_colors()
(require('wezterm')).color.get_default_colors()
local wt = require 'wezterm'; wt.color.get_default_colors()
local color = wezterm.color; color.get_default_colors()
local get_defaults = wezterm.color.get_default_colors; get_defaults()
local key = 'get_default_colors'; wezterm.color[key]()
```

Include supported comment-separated and parenthesized receiver forms.

Add `default_colors_call_resolver_rejects_dynamic_or_nonexact_forms` covering:

- one or more arguments;
- method-call syntax;
- missing or unmatched parentheses;
- `.field`, `[key]`, a second call, arithmetic, boolean, and concatenation
  tails; preserve the existing exact-value boundary handling for semicolons,
  table commas/closing braces, new statements, and labels;
- a dynamic field key;
- aliases rebound before the call;
- a shadowed or dynamically supplied module.

The positive helper should return true only when the whole value expression is
the exact zero-argument call.

**Step 2: Verify RED**

Run:

```powershell
cargo test -p rssh-app default_colors_call_resolver_ -- --nocapture
```

Expected: the positive test fails because no resolver exists; the negative
cases establish the fail-closed boundary.

**Step 3: Implement the resolver**

Add a boolean/value-shape resolver alongside
`lua_wezterm_color_load_scheme_path_from_query_with_static_source` and the
built-in-scheme call normalization. Reuse their existing direct-module,
static-module-alias, static-key, and direct-function-alias machinery rather than
matching text substrings.

After canonicalization, require:

1. the exact `wezterm.color.get_default_colors` callee;
2. a parenthesized argument list whose comment-normalized body is empty;
3. a tail accepted only by the existing exact-value/statement-end boundary
   rules.

Resolve aliases at the call offset and preserve the current lexical-cell and
local-attribute rules.

**Step 4: Make resolver tests GREEN**

Run:

```powershell
cargo test -p rssh-app default_colors_call_resolver_ -- --nocapture
cargo fmt --all -- --check
```

Expected: all pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: resolve static WezTerm default color calls"
```

### Task 4: Route the source through direct `config.colors`

**Files:**
- Modify: `crates/rssh-app/src/window.rs` around `NativeColorSchemeLuaSource`, `NativeConfigColorsLuaSource`, and config-colors source extraction

**Step 1: Add RED end-to-end tests**

Add `window_app_loads_wezterm_default_colors_directly` with:

```lua
local wezterm = require 'wezterm'
local config = wezterm.config_builder()
config.colors = wezterm.color.get_default_colors()
return config
```

Assert all scalar/RGBA and ANSI fields, all 240 indexed entries, and the absence
of unrelated optional palette fields in the parsed source before effective
fallback merging.

Add `window_app_loads_wezterm_default_colors_through_static_aliases` for module,
color-table, function, and static-key aliases. Add a table-return configuration
form as well as `config_builder()` assignment form.

**Step 2: Verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_loads_wezterm_default_colors_ -- --nocapture
```

Expected: direct and aliased sources are not selected yet.

**Step 3: Add the new source cases**

Extend `NativeColorSchemeLuaSource` with:

```rust
DefaultColors {
    variable: Option<NativeLoadSchemeVariableReference>,
    entry_mutation: Option<NativeColorSchemeEntryVariableReference>,
}
```

Extend `NativeConfigColorsLuaSource` with a corresponding config-level case.
Update every exhaustive match, including `with_entry_mutation`,
`color_scheme_lua_source_value_from_query`,
`lua_color_variable_source_before_offset`,
`lua_color_variable_known_binding_from_query`,
`lua_config_colors_source_value_from_query`, and
`lua_config_colors_variable_source_before_offset`.

**Step 4: Seed overrides without losing alpha or indexed entries**

Add a focused conversion/helper that writes the WezTerm palette into
`NativeConfigOverrides`:

- set every scalar supplied by upstream;
- preserve `selection_fg_color = Some(None)` semantics (explicit transparent
  foreground, distinct from an unspecified outer `None`);
- set the combined 16-color `ansi_palette`;
- set all 240 indexed slots in `indexed_palette`;
- leave fields absent from upstream unset.

In `apply_lua_color_scheme_source_overrides`, initialize the new source from
that exact palette and then replay variable and custom-entry mutations using the
same order as table, load-scheme, and built-in sources.

**Step 5: Make direct consumers GREEN**

Run:

```powershell
cargo test -p rssh-app window_app_loads_wezterm_default_colors_ -- --nocapture
cargo test -p rssh-app config_colors -- --nocapture
cargo fmt --all -- --check
```

Expected: all pass.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: apply WezTerm default colors to config"
```

### Task 5: Preserve ordered variable mutation semantics

**Files:**
- Modify: `crates/rssh-app/src/window.rs` near palette mutation reducer tests and default-source binding recognition

**Step 1: Add RED variable and mutation tests**

Add `window_app_reduces_wezterm_default_color_mutations_in_source_order` with a
single default palette variable and an interleaved sequence proving:

- scalar replacement;
- `ansi` and `brights` whole replacements followed by slot patches;
- `indexed = { ... }` clearing inherited slots followed by indexed slot patches;
- selection RGBA replacement;
- tab-bar and ColorSpec nested updates;
- a whole replacement with a fresh `get_default_colors()` call, followed by
  later mutations.

Add `window_app_uses_latest_wezterm_default_color_binding_before_reference` to
prove assignment-time ordering and lexical-cell tracking.

**Step 2: Add RED fail-closed identity tests**

Add `window_app_rejects_dynamic_wezterm_default_color_identity` covering:

- closure capture followed by rebinding;
- function/identity alias escape;
- dynamic whole replacement or indexed key;
- reassignment through an unproven value;
- mutation through an aliased table identity;
- invocation with arguments or an expression tail.

Assert the complete static source returns `None`, not a partially applied
default palette.

**Step 3: Verify RED**

Run:

```powershell
cargo test -p rssh-app wezterm_default_color_mutation -- --nocapture
cargo test -p rssh-app wezterm_default_color_binding -- --nocapture
cargo test -p rssh-app dynamic_wezterm_default_color_identity -- --nocapture
```

Expected: the new source is not yet recognized as a known binding in all paths.

**Step 4: Complete reducer integration**

Teach the known-binding parser and source wrapper to retain the default source
through `LuaPaletteMutationEvent` collection. Replay the already-committed
logical-statement reducer; do not add a second mutation scanner. Ensure a fresh
whole replacement resets the base to a new upstream default palette at the
correct statement position.

**Step 5: Make ordered semantics GREEN**

Run:

```powershell
cargo test -p rssh-app wezterm_default_color_ -- --nocapture
cargo test -p rssh-app palette_mutation -- --nocapture
cargo fmt --all -- --check
```

Expected: all pass.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: reduce WezTerm default color mutations"
```

### Task 6: Cover every custom color-scheme consumer

**Files:**
- Modify: `crates/rssh-app/src/window.rs` near custom color-scheme parsing tests

**Step 1: Add RED inline and direct-entry tests**

Add `window_app_parses_wezterm_default_colors_in_inline_custom_schemes` for:

```lua
config.color_schemes = {
  ['Default Copy'] = wezterm.color.get_default_colors(),
}
```

Add `window_app_parses_wezterm_default_colors_in_direct_custom_scheme_entries`
for:

```lua
config.color_schemes['Default Copy'] = wezterm.color.get_default_colors()
```

Cover intermediate variables and mutations both before and after insertion,
including static aliases. Assert the resulting named palette has the full
indexed table and exact alpha-bearing selection colors.

**Step 2: Verify RED**

Run:

```powershell
cargo test -p rssh-app default_colors_in_ -- --nocapture
```

Expected: any still-unrouted custom-scheme form fails.

**Step 3: Finish source routing**

Update only the common `color_scheme_lua_source_value_from_query` and entry
mutation plumbing required by the failures. Keep the new source flowing through
`native_color_scheme_palette_from_lua_source`; do not duplicate consumer-specific
palette construction.

**Step 4: Make all consumers GREEN**

Run:

```powershell
cargo test -p rssh-app default_colors_in_ -- --nocapture
cargo test -p rssh-app custom_color_scheme -- --nocapture
cargo test -p rssh-app color_scheme -- --nocapture
cargo fmt --all -- --check
```

Expected: all pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "test: cover default colors in custom schemes"
```

### Task 7: Update parity documentation and run the full gate

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/mvp-6-app-shell-v1.md`
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Update the documented boundary**

State that bounded-static `wezterm.color.get_default_colors()` is supported for
direct colors, named custom schemes, static aliases, intermediate variables,
and ordered palette mutations. Remove it from the explicit unsupported list.
Keep arbitrary Lua execution, dynamic iteration/keys, escaped aliases, runtime
color objects, and unprovable rebinding documented as unsupported.

**Step 2: Run focused regression groups**

```powershell
cargo test -p rssh-app default_colors -- --nocapture
cargo test -p rssh-app load_scheme -- --nocapture
cargo test -p rssh-app builtin_scheme -- --nocapture
cargo test -p rssh-app palette_mutation -- --nocapture
cargo test -p rssh-app color_spec -- --nocapture
```

Expected: all pass.

**Step 3: Run the complete verification gate**

```powershell
cargo test -p rssh-app
cargo test --workspace
cargo fmt --all -- --check
git diff --check
git status --short
```

Expected: both test suites pass with zero failures, formatting and whitespace
checks pass, and status lists only the intentional documentation edits before
the final commit.

**Step 4: Commit**

```powershell
git add docs/architecture.md docs/mvp-6-app-shell-v1.md docs/research/wezterm-parity-gap.md
git commit -m "docs: record WezTerm default colors parity"
```

**Step 5: Re-run final evidence after the commit**

```powershell
cargo test -p rssh-app
cargo test --workspace
cargo fmt --all -- --check
git diff --check
git status --short --branch
```

Expected: all verification remains green and the worktree is clean.
