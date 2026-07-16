# WezTerm Color Object Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the complete bounded-static WezTerm `ColorWrap` API from pinned commit `093bf6b` across every existing typed Lua consumer.

**Architecture:** Add a typed static expression evaluator backed by `wezterm-color-types = "=0.3.0"`. Preserve `SrgbaTuple` precision through constructor calls, method chains, aliases, and tuple bindings, then adapt scalar results into the existing color/number/bool/string parsing paths. Reuse the current WezTerm receiver, static-key, binding-offset, recursion-limit, and API-mutation checks so unprovable expressions fail closed.

**Tech Stack:** Rust 2024, `wezterm-color-types 0.3.0`, the bounded Lua parser in `crates/rssh-app/src/window.rs`, pinned upstream source in `refs/wezterm`, Cargo test, rustfmt.

---

### Task 1: Lock the typed constructor contract

**Files:**
- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `crates/rssh-app/src/window.rs:123300-123450`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Add the exact upstream color dependency**

Add:

```toml
wezterm-color-types = "=0.3.0"
```

Run:

```powershell
cargo check -p rssh-app
```

Expected: PASS and `Cargo.lock` records `wezterm-color-types 0.3.0`.

**Step 2: Write failing constructor tests**

Add tests that call the not-yet-implemented evaluator:

```rust
#[test]
fn static_wezterm_color_value_evaluator_parses_constructors() {
    use wezterm_color_types::SrgbaTuple;

    for (source, marker, expected) in [
        (
            "config.colors = { foreground = wezterm.color.parse('rgba(1,2,3,0.5)') }",
            "wezterm.color.parse",
            "rgba(0.39215686274509803% 0.7843137254901961% 1.1764705882352942% 49.80392156862745%)",
        ),
        (
            "config.colors = { foreground = wezterm.color.from_hsla(120, 1, 0.25, 0.5) }",
            "wezterm.color.from_hsla",
            SrgbaTuple::from_hsla(120.0, 1.0, 0.25, 0.5).to_string().as_str(),
        ),
    ] {
        let start = source.find(marker).unwrap();
        let value = super::lua_static_wezterm_color_value_from_query(
            super::LuaStaticSource {
                source,
                max_start: start,
            },
            &source[start..],
        )
        .expect("expected static Color");
        assert_eq!(value.as_color().unwrap().to_string(), expected);
    }
}
```

Use owned expected strings in the actual test to avoid borrowing a temporary.
Also cover:

- direct `require('wezterm')` and parenthesized receivers;
- `local wt = require 'wezterm'`;
- `local color = wt.color`;
- static keys for `color`, `parse`, and `from_hsla`;
- direct constructor aliases;
- top-level static string and finite number arguments.

**Step 3: Run the constructor test to verify RED**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color_value_evaluator_parses_constructors -- --exact --nocapture
```

Expected: compilation failure because `NativeStaticLuaColorValue` and
`lua_static_wezterm_color_value_from_query` do not exist.

**Step 4: Commit the RED contract**

```powershell
git add crates/rssh-app/Cargo.toml Cargo.lock crates/rssh-app/src/window.rs
git commit -m "test: lock WezTerm Color constructors"
```

### Task 2: Implement typed values and exact constructors

**Files:**
- Modify: `crates/rssh-app/src/window.rs:110500-111100`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Add the internal value type**

Add near the shared color parser:

```rust
#[derive(Debug, Clone, PartialEq)]
enum NativeStaticLuaColorValue {
    Color(wezterm_color_types::SrgbaTuple),
    Number(f64),
    Integer(u8),
    Bool(bool),
    String(String),
    Tuple(Vec<NativeStaticLuaColorValue>),
}

impl NativeStaticLuaColorValue {
    fn as_color(&self) -> Option<wezterm_color_types::SrgbaTuple> {
        match self {
            Self::Color(color) => Some(*color),
            _ => None,
        }
    }

    fn into_scalar(self) -> Option<Self> {
        (!matches!(self, Self::Tuple(_))).then_some(self)
    }
}
```

Add a depth constant or reuse `LUA_STATIC_LOAD_SCHEME_PATH_MAX_DEPTH`.

**Step 2: Add conversion helpers**

Implement:

```rust
fn native_static_lua_color_from_terminal_color(
    color: Color,
) -> Option<wezterm_color_types::SrgbaTuple>

fn terminal_color_from_native_static_lua_color(
    color: wezterm_color_types::SrgbaTuple,
) -> Color
```

Only `Rgb` and `Rgba` are valid Color object values. Convert with upstream
`to_srgb_u8`; preserve alpha as `Rgba` unless it is `255`.

**Step 3: Canonicalize constructors**

Replace the parse-only special case with a shared resolver:

```rust
enum LuaStaticWeztermColorConstructor {
    Parse,
    FromHsla,
}

fn lua_static_wezterm_color_constructor_call_from_query(
    static_source: LuaStaticSource<'_>,
    query: &str,
) -> Option<(LuaStaticWeztermColorConstructor, &str)>
```

Reuse:

- `lua_static_wezterm_receiver_rest_from_query_with_strict_aliases`;
- `lua_static_string_field_key_from_query`;
- `lua_static_builtin_scheme_binding_before_offset`;
- the existing parse alias scanner pattern.

Resolve direct aliases for both constructors at the original call offset.

**Step 4: Evaluate constructor arguments**

Implement:

```rust
fn lua_static_wezterm_color_value_from_query(
    static_source: LuaStaticSource<'_>,
    query: &str,
) -> Option<NativeStaticLuaColorValue>

fn lua_static_wezterm_color_value_from_query_with_depth(
    static_source: LuaStaticSource<'_>,
    query: &str,
    depth: usize,
) -> Option<NativeStaticLuaColorValue>
```

For `parse`, require one statically resolved string and parse it as
`SrgbaTuple`. For `from_hsla`, require four statically resolved finite numbers
and call `SrgbaTuple::from_hsla`.

Require an exact expression tail using the existing value-end rules.

**Step 5: Run constructor tests**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color_value_evaluator_parses_constructors -- --exact --nocapture
cargo test -p rssh-app color_parse
```

Expected: PASS.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: evaluate static WezTerm Color constructors"
```

### Task 3: Implement all color-returning methods and chains

**Files:**
- Modify: `crates/rssh-app/src/window.rs:110500-111400`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write a RED method matrix**

Add:

```rust
#[test]
fn static_wezterm_color_value_evaluator_matches_all_color_methods() {
    let base: wezterm_color_types::SrgbaTuple =
        "rgba(25% 50% 75% 80%)".parse().unwrap();
    let cases = [
        ("complement()", base.complement()),
        ("complement_ryb()", base.complement_ryb()),
        ("saturate(0.2)", base.saturate(0.2)),
        ("desaturate(0.2)", base.saturate(-0.2)),
        ("saturate_fixed(0.2)", base.saturate_fixed(0.2)),
        ("desaturate_fixed(0.2)", base.saturate_fixed(-0.2)),
        ("lighten(0.2)", base.lighten(0.2)),
        ("darken(0.2)", base.lighten(-0.2)),
        ("lighten_fixed(0.2)", base.lighten_fixed(0.2)),
        ("darken_fixed(0.2)", base.lighten_fixed(-0.2)),
        ("adjust_hue_fixed(45)", base.adjust_hue_fixed(45.0)),
        (
            "adjust_hue_fixed_ryb(45)",
            base.adjust_hue_fixed_ryb(45.0),
        ),
    ];

    for (method, expected) in cases {
        let expression =
            format!("wezterm.color.parse('rgba(25% 50% 75% 80%)'):{method}");
        assert_static_color_expression(&expression, expected);
    }
}
```

Add a chain test equivalent to:

```lua
local base = wezterm.color.parse('yellow')
local transformed = base:complement_ryb():darken(0.2):saturate_fixed(0.1)
```

Assert against the same chained upstream calls without intermediate u8
conversion.

**Step 2: Run the method matrix to verify RED**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color_value_evaluator_matches_all_color_methods -- --exact --nocapture
```

Expected: FAIL because method tails are not evaluated.

**Step 3: Implement exact colon-method parsing**

Add:

```rust
fn lua_static_wezterm_color_method_result(
    static_source: LuaStaticSource<'_>,
    receiver: wezterm_color_types::SrgbaTuple,
    method: &str,
    arguments: &[&str],
    depth: usize,
) -> Option<NativeStaticLuaColorValue>
```

Map methods exactly:

```rust
match (method, arguments) {
    ("complement", []) => Color(receiver.complement()),
    ("complement_ryb", []) => Color(receiver.complement_ryb()),
    ("saturate", [factor]) => Color(receiver.saturate(number(factor)?)),
    ("desaturate", [factor]) => Color(receiver.saturate(-number(factor)?)),
    ("saturate_fixed", [amount]) => Color(receiver.saturate_fixed(number(amount)?)),
    ("desaturate_fixed", [amount]) => {
        Color(receiver.saturate_fixed(-number(amount)?))
    }
    ("lighten", [factor]) => Color(receiver.lighten(number(factor)?)),
    ("darken", [factor]) => Color(receiver.lighten(-number(factor)?)),
    ("lighten_fixed", [amount]) => Color(receiver.lighten_fixed(number(amount)?)),
    ("darken_fixed", [amount]) => Color(receiver.lighten_fixed(-number(amount)?)),
    ("adjust_hue_fixed", [amount]) => {
        Color(receiver.adjust_hue_fixed(number(amount)?))
    }
    ("adjust_hue_fixed_ryb", [amount]) => {
        Color(receiver.adjust_hue_fixed_ryb(number(amount)?))
    }
    _ => return None,
}
```

All numeric values must be finite. Parse repeated method tails until the exact
value end and enforce the recursion limit.

**Step 4: Resolve color variables at binding time**

Use the shared binding scanner to accept:

```lua
local base = wezterm.color.parse('yellow')
local transformed = base:darken(0.2)
```

Evaluate the binding right-hand side at its assignment offset, not the later
reference offset. Reject unknown rebinds and cycles.

**Step 5: Run focused regressions**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color_value_evaluator_matches_all_color_methods -- --exact --nocapture
cargo test -p rssh-app color_parse
cargo test -p rssh-app gradient
```

Expected: PASS.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: evaluate WezTerm Color method chains"
```

### Task 4: Implement tuple methods and multi-target bindings

**Files:**
- Modify: `crates/rssh-app/src/window.rs:66200-66920`
- Modify: `crates/rssh-app/src/window.rs:110500-111500`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write RED tuple tests**

Cover:

```lua
local base = wezterm.color.parse('yellow')
local a, b = base:triad()
local c, d, e = base:square()
local r, g, blue, alpha = base:srgba_u8()
local lr, lg, lb, la = base:linear_rgba()
local h, s, l, ha = base:hsla()
local lab_l, lab_a, lab_b, lab_alpha = base:laba()
```

Assert:

- color tuple elements equal upstream `triad`/`square`;
- u8 tuple elements equal `to_srgb_u8`;
- linear tuple equals `to_linear`;
- HSL and Lab tuples equal upstream values within `1e-9`;
- later single-variable references resolve the correct tuple position.

**Step 2: Run tuple tests to verify RED**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color_value_evaluator_resolves_multi_target_results -- --exact --nocapture
```

Expected: FAIL because tuple methods and multi-target bindings are unsupported.

**Step 3: Implement tuple method results**

Extend the exact method match:

```rust
("triad", []) => {
    let (a, b) = receiver.triad();
    Tuple(vec![Color(a), Color(b)])
}
("square", []) => {
    let (a, b, c) = receiver.square();
    Tuple(vec![Color(a), Color(b), Color(c)])
}
("srgba_u8", []) => {
    let (r, g, b, a) = receiver.to_srgb_u8();
    Tuple(vec![Integer(r), Integer(g), Integer(b), Integer(a)])
}
("linear_rgba", []) => {
    let rgba = receiver.to_linear();
    Tuple(vec![
        Number(f64::from(rgba.0)),
        Number(f64::from(rgba.1)),
        Number(f64::from(rgba.2)),
        Number(f64::from(rgba.3)),
    ])
}
("hsla", []) => tuple_numbers(receiver.to_hsla()),
("laba", []) => tuple_numbers(receiver.to_laba()),
```

**Step 4: Add a typed binding lookup**

Implement:

```rust
fn lua_static_color_value_binding_before_offset(
    source: &str,
    variable: &str,
    max_start: usize,
    depth: usize,
) -> Option<NativeStaticLuaColorValue>
```

For each top-level assignment:

- split targets with `split_lua_top_level_arguments`;
- evaluate the complete right-hand side at the assignment start;
- for a scalar, accept only a single target;
- for a tuple, select the target's positional value;
- unknown or missing positions return `None`;
- later proven bindings replace earlier ones;
- later unknown bindings shadow the value.

**Step 5: Run tuple and existing binding tests**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color_value_evaluator_resolves_multi_target_results -- --exact --nocapture
cargo test -p rssh-app static_load_scheme_path
cargo test -p rssh-app default_colors_call_resolver
```

Expected: PASS.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: resolve WezTerm Color tuple bindings"
```

### Task 5: Implement numeric comparisons, equality, and string conversion

**Files:**
- Modify: `crates/rssh-app/src/window.rs:66200-66920`
- Modify: `crates/rssh-app/src/window.rs:108200-108400`
- Modify: `crates/rssh-app/src/window.rs:110500-111600`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write RED scalar-result tests**

Cover:

```lua
local red = wezterm.color.parse('red')
local navy = wezterm.color.parse('navy')
local ratio = red:contrast_ratio(navy)
local distance = red:delta_e(navy)
local same = red == wezterm.color.parse('red')
local different = red ~= navy
local text = tostring(red:darken(0.2))
```

Assert ratio/distance against upstream values, equality booleans, and the
upstream `to_string` result. Also assign a numeric result into an existing
supported numeric config field and use `tostring(Color)` in a supported static
status callback.

**Step 2: Run scalar-result tests to verify RED**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color_value_evaluator_resolves_scalar_results -- --exact --nocapture
```

Expected: FAIL.

**Step 3: Implement comparison methods**

Add:

```rust
("contrast_ratio", [other]) => Number(
    receiver.contrast_ratio(&color_argument(other)?) as f64
),
("delta_e", [other]) => Number(
    f64::from(receiver.delta_e(&color_argument(other)?))
),
```

The argument must evaluate to exactly one Color value.

**Step 4: Implement exact `==`, `~=`, and `tostring`**

At top-level expression parsing:

- split one top-level `==` or `~=` only;
- require both operands to be proven Color scalars;
- use `SrgbaTuple` equality;
- recognize exact one-argument `tostring(...)` and return upstream
  `color.to_string()`.

Reject chained comparisons and expression tails.

**Step 5: Add typed scalar adapters**

Implement:

```rust
fn lua_static_color_number_from_query(
    static_source: LuaStaticSource<'_>,
    query: &str,
) -> Option<f64>

fn lua_static_color_bool_from_query(
    static_source: LuaStaticSource<'_>,
    query: &str,
) -> Option<bool>

fn lua_static_color_string_from_query(
    static_source: LuaStaticSource<'_>,
    query: &str,
) -> Option<String>
```

Wire them into `parse_maybe_static_query_f64`, static bool resolution, and the
existing static string/`tostring` path without allowing tuples in scalar
contexts.

**Step 6: Run focused scalar/status tests**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color_value_evaluator_resolves_scalar_results -- --exact --nocapture
cargo test -p rssh-app update_status
cargo test -p rssh-app effective_config
```

Expected: PASS.

**Step 7: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: expose WezTerm Color scalar results"
```

### Task 6: Route Color values through every shared color consumer

**Files:**
- Modify: `crates/rssh-app/src/window.rs:110500-111100`
- Test: `crates/rssh-app/src/window.rs:127000-129200`
- Test: `crates/rssh-app/src/window.rs:158700-159300`
- Test: `crates/rssh-app/src/window.rs:210300-215200`

**Step 1: Write RED end-to-end consumer tests**

Add a table-driven test whose values are transformed Color expressions:

```lua
local wezterm = require 'wezterm'
local base = wezterm.color.parse('yellow')
local accent = base:complement_ryb():darken(0.2)
local a, b = accent:triad()

return {
  colors = {
    foreground = base,
    background = accent,
    cursor_bg = a,
    selection_bg = b,
    quick_select_match_fg = { Color = accent:lighten(0.1) },
  },
  window_background_gradient = {
    colors = { base, accent },
  },
  background = {
    { source = { Color = accent } },
  },
  window_frame = {
    active_titlebar_bg = accent,
  },
  integrated_title_button_color = accent,
}
```

Add named custom scheme and post-binding mutation coverage. Assert effective
colors against direct upstream calculations converted once with
`to_srgb_u8`.

**Step 2: Run end-to-end tests to verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_routes_wezterm_color_objects_through_shared_consumers -- --exact --nocapture
```

Expected: FAIL because shared color adapters still only resolve the older
parse-only query form.

**Step 3: Replace the parse-only adapter**

Change:

```rust
fn lua_color_from_query_with_static_source(...)
```

to try the typed evaluator first:

```rust
if let Some(color) =
    lua_static_wezterm_color_value_from_query(static_source, value)?.as_color()
{
    return Some(terminal_color_from_native_static_lua_color(color));
}
```

Then retain literal parsing as the fallback. Update opaque, selection, multiple
static-source, palette-array, tab-bar, ColorSpec, gradient, background-layer,
window-frame, and integrated-title adapters only where they bypass this shared
function; consolidate bypasses onto the shared adapter instead of duplicating
method parsing.

**Step 4: Run the consumer matrix**

Run:

```powershell
cargo test -p rssh-app window_app_routes_wezterm_color_objects_through_shared_consumers -- --exact --nocapture
cargo test -p rssh-app color_parse
cargo test -p rssh-app color_scheme
cargo test -p rssh-app window_background_gradient
cargo test -p rssh-app background_layer
cargo test -p rssh-app window_frame
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: route WezTerm Color objects to color consumers"
```

### Task 7: Lock fail-closed identity and arity behavior

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write negative tests**

Add a table covering:

- wrong constructor and method arity;
- non-finite `from_hsla` and method arguments;
- dot calls and extracted/bound method identities;
- tuple in scalar color/number/bool/string context;
- dynamic method arguments or color arguments;
- constructor/module/color namespace/API mutation;
- color variable unknown rebind before use;
- identity escape through another variable followed by mutation;
- multi-target ambiguity and missing tuple positions;
- expression tails and chained comparisons;
- cycles and recursion depth;
- dynamic control-flow bindings.

Each case must assert the typed evaluator returns `None`; complete config cases
must assert `native_config_overrides_from_wezterm_lua_config` returns `None`
when the unprovable value is required.

**Step 2: Run the negative matrix**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color_value_evaluator_rejects_unprovable_forms -- --exact --nocapture
```

Expected before any fix: at least one case fails if the evaluator is too
permissive.

**Step 3: Tighten at the common resolver**

Apply minimal fixes in the typed evaluator and shared binding/API mutation
checks. Do not add consumer-specific rejection logic.

**Step 4: Run all helper regressions**

Run:

```powershell
cargo test -p rssh-app static_wezterm_color
cargo test -p rssh-app color_parse
cargo test -p rssh-app default_colors
cargo test -p rssh-app builtin_scheme
cargo test -p rssh-app load_scheme
cargo test -p rssh-app gradient
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "fix: reject unprovable WezTerm Color expressions"
```

### Task 8: Document the boundary and run the final gate

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/mvp-6-app-shell-v1.md`
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Update parity documentation**

Document:

- both constructors;
- all 20 pinned Color methods;
- method chains and retained floating precision;
- constructor/module/static-key aliases;
- color variables and multi-target tuple bindings;
- numeric comparison, equality, and `tostring` results;
- all shared color consumers;
- explicit bounded-static exclusions.

Remove Color-object analysis from the relevant open-gap list, but keep
arbitrary Lua userdata behavior, method extraction, tuple table expansion,
dynamic control flow, and image color extraction open.

**Step 2: Run the complete verification gate**

Run:

```powershell
$env:RUST_TEST_THREADS='1'
cargo test -p rssh-app
Remove-Item Env:RUST_TEST_THREADS -ErrorAction SilentlyContinue
cargo test --workspace
cargo fmt --all -- --check
git diff --check
git status --short --branch
```

Expected:

- all `rssh-app` tests pass, with only the existing integration ignores;
- all workspace tests pass;
- formatting and diff checks return exit code 0;
- only the intended documentation changes remain before the final commit.

**Step 3: Commit documentation**

```powershell
git add docs/architecture.md docs/mvp-6-app-shell-v1.md docs/research/wezterm-parity-gap.md
git commit -m "docs: record WezTerm Color object parity"
```

**Step 4: Re-run final evidence after the commit**

Run:

```powershell
cargo test --workspace
cargo fmt --all -- --check
git diff --check
git status --short --branch
```

Expected: PASS and a clean feature worktree.

**Step 5: Finish the branch**

Use `superpowers:verification-before-completion`, then
`superpowers:finishing-a-development-branch`. Present the verified local merge,
PR, keep, and discard options without pushing or deleting work unless selected.
