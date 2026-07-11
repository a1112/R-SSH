# Built-in Scheme Map Consumers Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Resolve statically indexed built-in scheme maps through every existing config palette consumer while preserving palette mutation and source-order semantics.

**Architecture:** Generalize the existing whole-map lookup helper in `window.rs` so it is shared by config colors, custom color-scheme entries, and top-level palette-variable bindings. Continue producing the existing Builtin source variants, recording a palette variable only after a map entry has been selected so existing mutation passes apply to the clone rather than the map.

**Tech Stack:** Rust 2024, Cargo test, existing bounded Lua static parser.

---

### Task 1: Cover custom color-scheme consumers

**Files:**
- Modify: `crates/rssh-app/src/window.rs` test module near existing built-in lookup tests

**Step 1: Write failing tests**

Add end-to-end tests for both forms:

```lua
local schemes = wezterm.color.get_builtin_schemes()
config.color_schemes = { ['Mine'] = schemes['Gruvbox Light'] }
config.color_scheme = 'Mine'
```

```lua
local schemes = wezterm.color.get_builtin_schemes()
config.color_schemes['Mine'] = schemes['Gruvbox Light']
config.color_scheme = 'Mine'
```

Assert Gruvbox foreground, background, and representative ANSI/bright slots.

**Step 2: Verify RED**

Run each exact test with `cargo test -p rssh-app window::tests::<test-name> -- --exact`.

Expected: FAIL because `color_scheme_lua_source_value_from_query` does not route whole-map indexed values to `NativeColorSchemeLuaSource::Builtin`.

**Step 3: Implement the shared direct consumer path**

Call the whole-map indexed resolver from `color_scheme_lua_source_value_from_query` before parsing a plain variable. Return `NativeColorSchemeLuaSource::Builtin { name, variable: None, entry_mutation: None }`.

**Step 4: Verify GREEN**

Run both exact tests and the existing `window_app_parses_wezterm_lua_builtin_scheme_from_whole_map_variable` test.

Expected: PASS.

### Task 2: Preserve intermediate palette-variable mutation semantics

**Files:**
- Modify: `crates/rssh-app/src/window.rs` parser and tests

**Step 1: Write a failing mutation test**

Use:

```lua
local schemes = wezterm.color.get_builtin_schemes()
local scheme = schemes['Gruvbox Light']
scheme.background = '#010203'
config.color_schemes = { ['Mine'] = scheme }
config.color_scheme = 'Mine'
```

Assert the built-in foreground/ANSI palette plus the mutated background.

**Step 2: Verify RED**

Run the exact test. Expected: FAIL because the palette-variable binding resolver only recognizes direct built-in lookup assignments.

**Step 3: Implement binding support**

Teach `lua_builtin_color_scheme_assignment_from_query` and the matching before-offset path to recognize a whole-map indexed right-hand side using the assignment/lookup position. Return the selected built-in name while retaining the selected palette variable in the existing source variant, so existing mutation-range logic applies.

**Step 4: Add binding and rejection coverage**

Cover the legacy API/module or function alias, a static key variable captured at lookup time, a later key rebind that does not change the selected palette, and rejection of dynamic keys or a dynamically rebound map before lookup.

**Step 5: Verify GREEN and regressions**

Run the new tests plus existing direct built-in palette-variable mutation tests.

Expected: PASS with mutation ordering unchanged.

### Task 3: Record parity and verify the workspace

**Files:**
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Update the bounded parity claim**

Record support for whole-map bindings across config colors, custom scheme entries, and intermediate palette variables. Keep dynamic selection, iteration, and arbitrary Lua explicitly open.

**Step 2: Run verification**

Run:

- `$env:RUST_TEST_THREADS='1'; cargo test -p rssh-app`
- `cargo test --workspace`
- `git diff --check`

Expected: all non-ignored tests pass and no whitespace errors are reported.

**Step 3: Commit**

Stage `crates/rssh-app/src/window.rs` and `docs/research/wezterm-parity-gap.md`, then commit with `feat: complete builtin scheme map consumers`.
