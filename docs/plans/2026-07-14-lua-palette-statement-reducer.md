# Lua Palette Statement Reducer Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace divergent palette-variable scanners with one exact, source-ordered Lua statement event stream used by both identity validation and mutation replay.

**Architecture:** A logical-statement lexer produces stable source ranges and distinguishes Lua long strings from long-string index keys. Each relevant statement is parsed once into a typed palette event with an exactly consumed RHS; identity tracking and a sequential reducer consume the same events. Unsupported or partial statements fail closed, while whole replacements, slot patches, and nested patches preserve Lua source order.

**Tech Stack:** Rust 2024, existing bounded Lua static parser in `crates/rssh-app/src/window.rs`, Cargo test, rustfmt.

---

### Task 1: Lock the reviewer counterexamples as RED tests

**Files:**
- Modify: `crates/rssh-app/src/window.rs` test module near the existing built-in scheme-map and Lua splitter tests

**Step 1: Add the closure-alias regression**

Add `window_app_rejects_aliased_palette_capture_after_known_rebind` using:

```lua
local schemes = wezterm.color.get_builtin_schemes()
local scheme = schemes['Gruvbox Light']
local function mutate_original()
  scheme.background = '#010203'
end
local mutate = mutate_original
scheme = schemes['Builtin Solarized Dark']
mutate()
config.colors = scheme
```

Assert `native_config_overrides_from_wezterm_lua_config(...)` returns `None`.

**Step 2: Add the exact-RHS regression**

Add `window_app_rejects_palette_boolean_literal_expression_tail` with
`scheme.tab_bar.active_tab.italic = false or true`. Assert fail closed rather
than accepting the `false` prefix.

**Step 3: Add whole-replacement regressions**

Add end-to-end tests for:

```lua
scheme.indexed = {}
scheme.indexed[137] = '#040506'
```

Assert the inherited `indexed[136]` is absent and 137 is present. Add negative
tests for unfinished `ansi = {}`, `brights = {}`, and ColorSpec `{}`. Add a
tab-bar whole replacement test proving absent fields are cleared.

**Step 4: Add lexer regressions**

Add unit tests proving both forms remain distinct:

```lua
local text = [[[foo]], 'after'
scheme[[[background]]] = '#010203'
```

The first contains an ordinary long string whose content begins with `[`. The
second is an outer index whose key is a long string.

**Step 5: Verify RED**

Run each exact test:

```powershell
cargo test -p rssh-app window::tests::<test-name> -- --exact
```

Expected: every reviewer counterexample fails for the documented current
behavior, while the existing long-string index positive test remains green.

**Step 6: Commit the tests only**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "test: cover Lua palette reducer edge cases"
```

Do not include production fixes in this commit. If existing uncommitted
production edits overlap, use patch staging and verify the staged diff contains
only tests.

### Task 2: Introduce one logical-statement lexer

**Files:**
- Modify: `crates/rssh-app/src/window.rs` around `lua_top_level_statement_start_indices_before_offset`, assignment splitting, and long-bracket helpers

**Step 1: Define the lexical item**

Introduce:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LuaLogicalStatement {
    start: usize,
    end: usize,
}
```

Add `lua_top_level_logical_statements_before_offset(source, max_start)` that
performs one forward lexical scan and returns exact statement ranges.

**Step 2: Make bracket handling context-aware**

At `[`:

- parse a long string when the current grammar position can start a value;
- parse an outer index when it follows an assignable expression and its inner
  key is a quoted or long-string literal;
- require the outer closing `]` after the inner long-string closing delimiter;
- otherwise use ordinary balanced bracket depth.

Delete `lua_bracket_starts_outer_index_around_long_string`; do not replace it
with another prefix-only heuristic.

**Step 3: Use the lexer in both proof and replay paths**

Replace the separate raw-start and logical-range callers in palette binding and
mutation code with the same `Vec<LuaLogicalStatement>`.

**Step 4: Verify GREEN and compatibility**

Run:

```powershell
cargo test -p rssh-app split_lua_top_level_arguments
cargo test -p rssh-app long_bracket
cargo test -p rssh-app dotted_comment
```

Expected: ordinary long strings, long-string index keys, and dotted load-scheme
continuations all pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "refactor: unify Lua logical statement lexing"
```

### Task 3: Parse exact typed palette events once

**Files:**
- Modify: `crates/rssh-app/src/window.rs` near `NativeColorSchemeLuaSource` and the current palette mutation helpers

**Step 1: Add typed events**

Introduce equivalent types with source ranges:

```rust
#[derive(Clone, Debug, PartialEq)]
enum LuaPaletteEvent {
    KnownBinding(LuaPaletteBinding),
    UnknownBinding,
    Mutation(LuaPaletteMutation),
    FunctionDefinition(LuaPaletteFunctionDefinition),
    FunctionAlias { alias: String, target: String },
    FunctionCall { name: String },
    Escape,
    Irrelevant,
}

#[derive(Clone, Debug, PartialEq)]
enum LuaPaletteMutation {
    ScalarReplace { field: String, value: NativePaletteValue },
    CompositeReplace { path: Vec<String>, value: NativeCompositeValue },
    SlotPatch { field: PaletteArrayField, index: usize, color: Color },
    NestedPatch { path: Vec<String>, value: NativePaletteValue },
}
```

Keep concrete value types aligned with existing `NativeConfigOverrides` fields;
do not introduce a second generic color/table representation when an existing
typed value can be reused.

**Step 2: Require complete RHS consumption**

Create exact wrappers for every supported static RHS parser. Each returns the
value plus the end offset. Accept only when the remainder is comments and
whitespace.

Cover strings, long strings, booleans, numeric indexes, tables, ColorSpec,
ANSI/brights arrays, indexed tables, and tab-bar tables. Remove
`lua_color_variable_mutation_rhs_is_single_static_expression` after callers use
the exact wrappers.

**Step 3: Parse each logical statement once**

Implement:

```rust
fn lua_palette_event_from_statement(
    source: &str,
    statement: LuaLogicalStatement,
    variable: &str,
) -> Option<LuaPaletteEvent>
```

If a statement references the tracked variable but cannot be fully classified,
return `Escape`; never return `Irrelevant` for a partial palette use.

**Step 4: Verify exact-RHS GREEN**

Run the new boolean-tail test, the earlier quoted/table literal-tail tests, and
the complete `color_spec` group.

Expected: partial expressions fail closed and all exact static values remain
supported.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "refactor: parse typed Lua palette events"
```

### Task 4: Track palette identity and closure aliases from events

**Files:**
- Modify: `crates/rssh-app/src/window.rs` around `lua_color_variable_source_before_offset`

**Step 1: Add lexical binding state**

Track whether the palette identifier is a currently declared local, its latest
known source, and the binding event offset. A function definition captures only
the variable binding visible at its definition site.

**Step 2: Track closure objects and aliases**

Represent each captured function definition with a stable internal identity.
Map function names and aliases to that identity. Redefining the original name
updates only that name; an existing alias continues to reference the older
closure. Alias chains resolve transitively and cycles fail closed.

**Step 3: Apply call and escape rules**

A call to any name mapped to a closure that captures the active palette cell
invalidates the candidate unless that closure body is itself exactly replayed
(not required in this slice). Unknown aliases, receiver calls, argument escape,
and dynamic function rebinding invalidate the candidate.

**Step 4: Verify identity tests**

Run exact tests for:

- direct captured closure after known rebind;
- alias call after known rebind;
- alias retaining an old closure after original-name redefinition;
- function declared before a later local palette binding (must not be treated as
  capturing that later local);
- unknown rebind, call argument, call receiver, and palette alias escape.

Expected: unsafe cases fail closed; the declared-before-binding safe case keeps
its proven palette source.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "fix: track Lua palette closure aliases"
```

### Task 5: Reduce palette mutations once in source order

**Files:**
- Modify: `crates/rssh-app/src/window.rs` around `apply_lua_color_variable_mutation_overrides`

**Step 1: Add the sequential reducer**

Implement one loop over the already parsed events:

```rust
fn apply_lua_palette_events(
    events: &[SpannedLuaPaletteEvent],
    overrides: &mut NativeConfigOverrides,
) -> Option<bool> {
    let mut parsed = false;
    for event in events {
        if let LuaPaletteEvent::Mutation(mutation) = &event.event {
            apply_lua_palette_mutation(mutation, overrides)?;
            parsed = true;
        }
    }
    Some(parsed)
}
```

Do not call any helper that rescans `source[..event.end]`.

**Step 2: Implement replacement versus patch semantics**

- indexed whole table, including `{}`, replaces the full indexed array;
- indexed slot changes one entry in the current array;
- ANSI/brights whole arrays must contain exactly eight values; incomplete or
  empty arrays fail closed;
- ANSI/brights slot changes one current entry;
- tab-bar/item whole tables replace and therefore clear omitted fields;
- ColorSpec whole replacement replaces the variant; an unfinished `{}` fails
  closed;
- nested patches operate on the value produced by all earlier events;
- later whole replacement discards earlier patches.

**Step 3: Verify reducer tests**

Run exact tests for repeated scalar writes, indexed slot/whole in both orders,
ANSI/brights slot/whole in both orders, ColorSpec nested/whole in both orders,
empty indexed replacement, tab-bar clearing, and unfinished composites.

Expected: last source event wins according to Lua table mutation semantics.

**Step 4: Remove superseded scanners**

Delete the old multi-pass mutation-table, palette-slot, indexed-slot, tab-bar,
and ColorSpec source rescans once every caller uses the event reducer. Confirm
there is no loop over statements whose body calls another full-source scan.

**Step 5: Run focused regression groups**

```powershell
cargo test -p rssh-app scheme_map
cargo test -p rssh-app whole_map
cargo test -p rssh-app load_scheme
cargo test -p rssh-app palette_mutation
cargo test -p rssh-app color_spec
```

Expected: all focused tests pass with no warnings.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "refactor: reduce Lua palette mutations in source order"
```

### Task 6: Complete the built-in map slice and establish the next baseline

**Files:**
- Modify: `docs/research/wezterm-parity-gap.md`
- Modify: `refs/README.md`
- Verify: `crates/rssh-app/src/window.rs`

**Step 1: Update parity evidence**

Record that whole built-in scheme-map bindings are supported through
`config.colors`, inline/direct custom schemes, and intermediate palette
variables with ordered mutations. Keep dynamic keys, iteration, arbitrary Lua,
and unproven result-table aliases explicitly open.

**Step 2: Synchronize the pinned source note**

Change the WezTerm revision in `refs/README.md` from the stale `577474d` to the
authoritative `093bf6b` already recorded by `refs/sources.json` and checked out
in `refs/wezterm`.

**Step 3: Run complete verification**

```powershell
$env:RUST_TEST_THREADS='1'
cargo test -p rssh-app
cargo test --workspace
cargo fmt --all -- --check
git diff --check
git status --short
```

Expected: all non-ignored tests pass, formatting and whitespace checks are
clean, and only intended files are modified.

**Step 4: Commit**

```powershell
git add crates/rssh-app/src/window.rs docs/research/wezterm-parity-gap.md refs/README.md
git commit -m "feat: complete ordered builtin scheme map consumers"
```

**Step 5: Begin the next WezTerm slice**

Use the committed event reducer for `wezterm.color.get_default_colors()`.
Implement a distinct WezTerm default palette, including all 240 indexed entries
from 16 through 255; do not reuse `NativeResolvedPalette::default()`.
