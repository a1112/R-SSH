# Static load_scheme Path Expressions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Resolve deterministic Lua string expressions used as `wezterm.color.load_scheme` paths across both `config.colors` and `config.color_schemes`.

**Architecture:** Normalize all supported direct/module/function alias calls into one canonical call parser while preserving the original source offset. Evaluate literal, variable, variable-chain, and static-concatenation arguments with a path-focused recursive resolver whose variable lookup uses assignment-time offsets and fails closed on dynamic shadowing.

**Tech Stack:** Rust, existing static Lua query helpers in `rssh-app`, TOML color-scheme loading, headless native-window config tests.

---

### Task 1: Add strict static path-expression evaluation

**Files:**
- Modify: `crates/rssh-app/src/window.rs:65461-65506`
- Modify: `crates/rssh-app/src/window.rs:67812-67844`
- Test: `crates/rssh-app/src/window.rs` tests near the existing static Lua parser tests

**Step 1: Write failing evaluator tests**

Add a test that calls a not-yet-defined
`lua_static_load_scheme_path_expression_value_from_query` helper. Use a single
source string and the byte offset of a final marker call:

```rust
#[test]
fn static_load_scheme_path_expressions_preserve_assignment_time_values() {
    let source = r#"
        local dir = '/first'
        local name = 'scheme'
        local path = dir .. '/' .. name
        dir = '/second'
        path = path .. '.toml'
        wezterm.color.load_scheme(path)
    "#;
    let call_start = source.find("wezterm.color.load_scheme").unwrap();

    assert_eq!(
        lua_static_load_scheme_path_expression_value_from_query(
            source,
            "path",
            call_start,
        )
        .as_deref(),
        Some("/first/scheme.toml")
    );
    assert_eq!(
        lua_static_load_scheme_path_expression_value_from_query(
            source,
            "dir .. '/direct.toml'",
            call_start,
        )
        .as_deref(),
        Some("/second/direct.toml")
    );
}
```

Add a second test for strict invalidation and recursion safety:

```rust
#[test]
fn static_load_scheme_path_expressions_reject_dynamic_shadowing_and_cycles() {
    let dynamic = r#"
        local path = '/static.toml'
        path = compute_path()
        wezterm.color.load_scheme(path)
    "#;
    let dynamic_call = dynamic.find("wezterm.color.load_scheme").unwrap();
    assert_eq!(
        lua_static_load_scheme_path_expression_value_from_query(
            dynamic,
            "path",
            dynamic_call,
        ),
        None
    );

    let uninitialized = r#"
        local path = '/static.toml'
        local path
        wezterm.color.load_scheme(path)
    "#;
    let uninitialized_call = uninitialized.find("wezterm.color.load_scheme").unwrap();
    assert_eq!(
        lua_static_load_scheme_path_expression_value_from_query(
            uninitialized,
            "path",
            uninitialized_call,
        ),
        None
    );

    let cycle = r#"
        local first = second
        local second = first
        wezterm.color.load_scheme(first)
    "#;
    let cycle_call = cycle.find("wezterm.color.load_scheme").unwrap();
    assert_eq!(
        lua_static_load_scheme_path_expression_value_from_query(
            cycle,
            "first",
            cycle_call,
        ),
        None
    );
}
```

**Step 2: Run the tests to verify RED**

Run:

```powershell
cargo test -p rssh-app static_load_scheme_path_expressions_ -- --nocapture
```

Expected: compilation fails because the new evaluator does not exist. Confirm
the failure names that helper rather than an unrelated syntax error.

**Step 3: Implement the strict evaluator**

Add a small depth limit and a binding lookup that returns the right-hand-side
slice plus the assignment's source offset. A matching later dynamic assignment
must replace the earlier candidate; a matching uninitialized local declaration
must clear it.

```rust
const LUA_STATIC_LOAD_SCHEME_PATH_MAX_DEPTH: usize = 8;

fn lua_static_load_scheme_path_expression_value_from_query(
    source: &str,
    query: &str,
    max_start: usize,
) -> Option<String> {
    lua_static_load_scheme_path_expression_value_from_query_with_depth(
        source,
        query,
        max_start,
        0,
    )
}

fn lua_static_load_scheme_path_expression_value_from_query_with_depth(
    source: &str,
    query: &str,
    max_start: usize,
    depth: usize,
) -> Option<String> {
    if depth > LUA_STATIC_LOAD_SCHEME_PATH_MAX_DEPTH {
        return None;
    }

    let query = lua_trim_start_comments(query.trim())?;
    if let Some((literal, literal_len)) = lua_inline_string_literal_value_and_len(query) {
        return lua_trim_start_comments(query.get(literal_len..)?)?
            .trim()
            .is_empty()
            .then_some(literal);
    }

    if query.contains("..") {
        let mut value = String::new();
        for segment in split_lua_string_concat_segments(query)? {
            value.push_str(
                &lua_static_load_scheme_path_expression_value_from_query_with_depth(
                    source,
                    segment,
                    max_start,
                    depth + 1,
                )?,
            );
        }
        return (!value.is_empty()).then_some(value);
    }

    let variable = lua_identifier_literal_from_query(query)?;
    let rest = query.get(variable.len()..)?;
    if !lua_static_identifier_value_rest_is_statement_end(rest) {
        return None;
    }
    let (value, binding_start) =
        lua_static_load_scheme_path_binding_before_offset(source, variable, max_start)?;
    lua_static_load_scheme_path_expression_value_from_query_with_depth(
        source,
        value,
        binding_start,
        depth + 1,
    )
}

fn lua_static_load_scheme_path_binding_before_offset<'a>(
    source: &'a str,
    variable: &str,
    max_start: usize,
) -> Option<(&'a str, usize)> {
    let mut selected = None;

    for start in lua_top_level_statement_start_indices_before_offset(source, max_start)? {
        let is_local = lua_source_keyword_at(source, start, "local");
        let rest = if is_local {
            lua_trim_start_comments(source.get(start + "local".len()..)?)?
        } else {
            source.get(start..)?
        };
        let Some(rest) = rest.strip_prefix(variable) else {
            continue;
        };
        if rest.chars().next().is_some_and(is_lua_identifier_character) {
            continue;
        }
        let rest = lua_trim_start_comments(rest)?;
        if let Some(value) = rest.strip_prefix('=') {
            selected = lua_top_level_statement_value_from_query(value)
                .map(|value| (value, start));
        } else if is_local
            && (lua_static_identifier_value_rest_is_statement_end(rest)
                || rest.trim_start().starts_with(','))
        {
            selected = None;
        }
    }

    selected
}
```

During implementation, preserve field mutations such as `path.field = ...` as
non-bindings; they must not clear a path variable. Use `apply_patch`, and adjust
the exact local-declaration boundary check if an existing helper provides a
safer equivalent.

**Step 4: Run the evaluator tests to verify GREEN**

Run the focused filter again. Expected: 2 passed, 0 failed.

Run nearby static string/expression tests if the compiler identifies a shared
helper interaction. Expected: all pass.

**Step 5: Format, inspect, and commit**

```powershell
cargo fmt --all
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "feat: evaluate static load scheme path expressions"
```

### Task 2: Route every load_scheme call through the expression evaluator

**Files:**
- Modify: `crates/rssh-app/src/window.rs:67180-67210`
- Modify: `crates/rssh-app/src/window.rs:67812-67887`
- Test: `crates/rssh-app/src/window.rs` beside Task 1 parser tests

**Step 1: Write failing canonical-call tests**

Add a test for a direct call and supported function alias, both using static
expressions:

```rust
#[test]
fn load_scheme_call_resolver_accepts_static_path_expressions_at_call_offset() {
    let source = r#"
        local wezterm = require 'wezterm'
        local load_scheme = wezterm.color.load_scheme
        local dir = '/schemes'
        local file = 'project.toml'
        config.colors = load_scheme(dir .. '/' .. file)
    "#;
    let query_start = source.find("load_scheme(dir").unwrap();
    let query = &source[query_start..];

    assert_eq!(
        lua_wezterm_color_load_scheme_path_from_query_with_static_source(source, query)
            .as_deref(),
        Some("/schemes/project.toml")
    );
}
```

Add rejection cases:

```rust
#[test]
fn load_scheme_call_resolver_rejects_invalid_argument_shapes() {
    for source in [
        "local path = 'one'; config.colors = wezterm.color.load_scheme(path, 'two')",
        "local path = 'one'; config.colors = wezterm.color.load_scheme path",
        "local path = 'one'; config.colors = wezterm.color.load_scheme(path).colors",
    ] {
        let start = source.find("wezterm.color.load_scheme").unwrap();
        assert_eq!(
            lua_wezterm_color_load_scheme_path_from_query_with_static_source(
                source,
                &source[start..],
            ),
            None,
            "source was {source:?}"
        );
    }
}
```

Add a focused fallback-helper assertion using
`lua_config_load_scheme_colors_assignment_from_query` and a direct
`config.colors = wezterm.color.load_scheme(path)` source. It must expect the
resolved path, proving the legacy helper is no longer literal-only.

**Step 2: Run the tests to verify RED**

```powershell
cargo test -p rssh-app load_scheme_call_resolver_ -- --nocapture
```

Expected: the accepted-expression test fails with `None`; the rejection test
may partially pass. Run the fallback test separately and confirm it also fails
to resolve the variable.

**Step 3: Add the canonical call parser**

Replace the literal-only terminal parsing with a helper shaped like:

```rust
fn lua_wezterm_color_load_scheme_path_from_call_query(
    source: &str,
    query: &str,
    call_max_start: usize,
) -> Option<String> {
    let rest = lua_function_name_rest_from_query(
        query.trim_start(),
        "wezterm.color.load_scheme",
    )?;
    let rest = lua_trim_start_comments(rest)?;

    if let Some(arguments) = rest.strip_prefix('(') {
        let (arguments, tail) = lua_parenthesized_argument_list_prefix_from_query(arguments)?;
        let arguments = split_lua_top_level_arguments(arguments)?;
        let [argument] = arguments.as_slice() else {
            return None;
        };
        if !lua_load_scheme_call_tail_is_value_end(tail) {
            return None;
        }
        return lua_static_load_scheme_path_expression_value_from_query(
            source,
            argument,
            call_max_start,
        );
    }

    let (literal, literal_len) = lua_inline_string_literal_value_and_len(rest)?;
    lua_load_scheme_call_tail_is_value_end(rest.get(literal_len)?)
        .then_some(literal)
}
```

Define `lua_load_scheme_call_tail_is_value_end` to accept an empty tail,
newline/comment/semicolon statement end, or the table-field terminators already
accepted by current callers, while rejecting `.`, `[`, `(`, `..`, and other
expression continuations.

Refactor `lua_wezterm_color_load_scheme_path_from_query_with_static_source` so
it computes `call_max_start` once from the original source slice and passes that
same offset to the canonical helper for:

- the original canonical call;
- a normalized module/`require` call;
- a normalized function-alias call.

Never call `lua_source_slice_start_offset` on the owned normalized `String`.

Update `lua_config_load_scheme_colors_assignment_from_query` to extract the
complete supported assignment value and send it through the central resolver,
then retain the existing colors-result-variable fallback.

**Step 4: Run call and legacy regression tests**

Run the new call/fallback tests. Expected: all pass.

Then run:

```powershell
cargo test -p rssh-app window_app_loads_wezterm_lua_colors_from_load_scheme_ -- --nocapture
cargo test -p rssh-app window_app_parses_wezterm_lua_custom_color_scheme_from_load_scheme -- --nocapture
```

Expected: all existing literal, direct/parenthesized `require`, static-key,
function-alias, result-variable, and mutation cases remain green.

**Step 5: Format, inspect, and commit**

```powershell
cargo fmt --all
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "feat: resolve static load scheme call paths"
```

### Task 3: Prove both colors consumers with real TOML schemes

**Files:**
- Test: `crates/rssh-app/src/window.rs:125840-126060`
- Test: `crates/rssh-app/src/window.rs:149855-150505`

**Step 1: Add the config.colors binding-timing integration test**

Follow the existing three-temp-file pattern from
`window_app_uses_latest_wezterm_lua_load_scheme_variable_assignment_before_config_colors`.
Create three TOML schemes with distinct foreground/background colors, then use:

```lua
local first_path = '<first>'
local second_path = '<second>'
local third_path = '<third>'
local scheme_path = first_path
scheme_path = second_path
local colors = wezterm.color.load_scheme(scheme_path)
scheme_path = third_path
config.colors = colors
```

Assert the effective colors come from the second file. This proves the latest
binding before the call wins and a later rebinding cannot change the already
loaded colors.

**Step 2: Add the config.color_schemes alias/concatenation integration test**

Create one temporary TOML file. Split its normalized path into a directory
prefix (including the trailing slash) and filename, then configure:

```lua
local wt = require 'wezterm'
local load_scheme = wt.color.load_scheme
local scheme_dir = '<normalized-parent>/'
local scheme_name = '<filename>'
local scheme_path = scheme_dir .. scheme_name

config.color_scheme = 'Project Scheme'
config.color_schemes = {
  ['Project Scheme'] = load_scheme(scheme_path),
}
```

Assert foreground, background, cursor color, and one palette slot from the TOML
file. This proves the function alias and the second consumer share the static
expression resolver.

**Step 3: Temporarily verify the tests fail against the pre-feature base if needed**

The parser/call feature was already introduced in Tasks 1-2, so these
integration tests should pass immediately. Do not weaken them to manufacture a
failure. The RED evidence for their behavior is provided by the focused Task 2
call tests before production changes.

Run each integration test by exact name. Expected on current HEAD: PASS.

**Step 4: Run the complete load_scheme regression group**

```powershell
cargo test -p rssh-app load_scheme -- --nocapture
```

Expected: all matching tests pass, including literal calls, aliases, result
variables, mutations, built-in scheme loading, and the two new consumers.

**Step 5: Format, inspect, and commit**

```powershell
cargo fmt --all
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "test: cover static load scheme path expressions"
```

### Task 4: Update parity records and verify the workspace

**Files:**
- Modify: `docs/research/wezterm-parity-gap.md:3501-3522`
- Modify: `docs/architecture.md:1656-1665`
- Modify: `docs/mvp-6-app-shell-v1.md:1474-1484`

**Step 1: Synchronize the three documentation records**

State that supported `load_scheme` paths may be inline literals or statically
evaluable top-level string expressions. Document literal/long-bracket values,
variable chains, pure `..` concatenation, latest binding at each evaluation
point, assignment-time capture, and later-call rebinding isolation.

Keep the boundary explicit: dynamic helpers/branches/environment-backed paths,
`wezterm.config_dir`, and downstream metadata consumption remain open. Do not
claim arbitrary Lua expression parity.

**Step 2: Verify formatting and scope**

```powershell
cargo fmt --all -- --check
git diff --check
git diff --stat
```

Expected: exit 0; implementation changes are limited to `window.rs`, and the
documentation commit is limited to the three named Markdown files.

**Step 3: Run the app crate suite**

```powershell
cargo test -p rssh-app
```

Expected: all app tests pass.

**Step 4: Run the full workspace suite**

```powershell
cargo test --workspace
```

Expected: all workspace tests pass.

**Step 5: Commit the documentation**

```powershell
git add docs/research/wezterm-parity-gap.md docs/architecture.md docs/mvp-6-app-shell-v1.md
git commit -m "docs: record static load scheme path parity"
```
