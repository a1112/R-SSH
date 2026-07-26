# WezTerm File-Backed Configuration Lifecycle Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Load WezTerm-style configuration from the real GUI startup path and keep all managed windows on one last-known-good, manually and automatically reloadable configuration generation.

**Architecture:** Add global configuration CLI options and a `config_lifecycle` module that owns strict bounded-static parsing, source rediscovery, last-known-good state, diagnostics, and filesystem watching. `NativeWindowManager` owns the lifecycle and transactionally reapplies its base generation beneath each window's runtime override layer; watcher threads only enqueue debounced winit events.

**Tech Stack:** Rust 2024, existing bounded-static WezTerm parser, `notify` filesystem events, `winit` user events, `rssh-app` unit/integration-style tests.

**Pinned Reference:** WezTerm `093bf6bf2b82b929ed80c04fd54ebc80464f715e`, especially `wezterm-gui/src/main.rs:68-87`, `config/src/config.rs:935-1066`, `config/src/lib.rs:478-602`, and `wezterm-gui/src/termwindow/mod.rs:1641-1750`.

**Design:** `docs/plans/2026-07-26-wezterm-config-lifecycle-design.md`.

---

## Execution rules

- Execute Tasks 0-5 in order with one fresh implementation subagent per task.
- Every implementation subagent must use `superpowers:test-driven-development`:
  add one focused behavior test, run it and record the expected RED failure,
  implement only that behavior, run GREEN, then repeat.
- After each task commit, dispatch a fresh spec-compliance reviewer. Only after
  spec review is Ready with no critical or important findings, dispatch a
  fresh code-quality reviewer. The original implementer fixes findings and the
  same reviewer re-reviews.
- Do not run implementation tasks in parallel. Tasks 1-4 share
  `config_lifecycle.rs` and/or `window.rs`; parallel mutation would invalidate
  RED/GREEN evidence.
- The production parser must never use the legacy
  `Option<NativeConfigOverrides>` result as proof that all syntax was
  understood. It may call the legacy extractor only after recursive strict
  validation.
- A source-file error is recoverable and keeps generation 0/defaults or the
  last-known-good generation. An invalid CLI `--config` is fatal before a
  window or PTY exists.
- Reload discovery runs on every attempt. A failed reload does not increment
  generation but still performs the upstream-compatible per-window
  reload-attempt notification.
- Watcher callbacks never mutate app state. All lifecycle and window mutation
  happens on the winit event-loop thread.
- Each task ends with `cargo fmt --all -- --check`, `git diff --check`, and a
  task-scoped commit containing only the intended files.

## Production static-config acceptance for this slice

The strict registry must support, at minimum, the fields that prove every
startup/runtime consumer:

- scalar/string/path/number/bool: `term`, `default_cwd`, `initial_cols`,
  `initial_rows`, `automatically_reload_config`, `scrollback_lines`,
  `max_fps`, `enable_tab_bar`, `color_scheme`;
- simple arrays: `default_prog`, `default_gui_startup_args`;
- strict composite tables: `colors` (including ANSI/bright arrays and the
  existing cursor/selection/tab-bar keys), `set_environment_variables`, and
  one complete `keys` entry with a statically supported action;
- every additional field exposed by the production registry must have a
  recursive decoder and rejection tests. A legacy-only field remains
  explicitly unsupported rather than partially accepted.

This is a lifecycle slice, not a claim of arbitrary Lua support. The
architecture documentation must list the production grammar boundary.

### Task 0: Parse Global Configuration CLI Options

**Files:**

- Modify: `crates/rssh-app/src/cli.rs:223-287` (`WindowOptions`, default GUI command)
- Modify: `crates/rssh-app/src/cli.rs:288-380` (`parse_args`)
- Modify: `crates/rssh-app/src/cli.rs:1350-1445` (`parse_window`)
- Modify: `crates/rssh-app/src/cli.rs` help text and test module

**Step 1: Write failing CLI model tests**

Add exact tests:

```rust
#[test]
fn parses_global_wezterm_config_options_for_default_window() { /* ... */ }

#[test]
fn parses_repeated_global_config_overrides_in_order() { /* ... */ }

#[test]
fn parses_global_config_options_before_window_and_start() { /* ... */ }

#[test]
fn rejects_skip_config_with_config_file() { /* ... */ }

#[test]
fn rejects_malformed_global_config_override() { /* ... */ }

#[test]
fn rejects_global_config_options_for_non_gui_commands() { /* ... */ }
```

Assert the exact `PathBuf`, ordered `(name, value)` pairs, `skip_config`,
default/window/start command, and stable error text.

**Step 2: Run RED**

Run:

```powershell
cargo test -p rssh-app cli::tests::parses_global_wezterm_config_options_for_default_window -- --exact --nocapture
```

Expected: FAIL because `WindowOptions` has no configuration model and the
first global flag is treated as an unknown command.

**Step 3: Add the CLI data model**

Add:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowConfigOptions {
    pub skip_config: bool,
    pub config_file: Option<PathBuf>,
    pub config_overrides: Vec<(String, String)>,
}
```

Add `pub config: WindowConfigOptions` to `WindowOptions`. Ensure the no-command
default initializes it.

**Step 4: Parse only the global prefix**

Refactor `parse_args` to collect argv after argv0, consume
`-n`/`--skip-config`, `--config-file PATH`, and repeated
`--config NAME=VALUE` before command dispatch, then attach the result only to
the default/window/start GUI command. Split `NAME=VALUE` on the first `=`;
require non-empty trimmed name and non-empty value but preserve the value text.
Reject skip/file conflicts and global config flags used with console/profile
commands.

Do not accept these options after the explicit subcommand.

**Step 5: Update help and run GREEN**

Run:

```powershell
cargo test -p rssh-app cli::tests::parses_global_wezterm_config_options_for_default_window -- --exact --nocapture
cargo test -p rssh-app cli::tests::parses_repeated_global_config_overrides_in_order -- --exact --nocapture
cargo test -p rssh-app cli::tests::rejects_skip_config_with_config_file -- --exact --nocapture
cargo test -p rssh-app cli::tests
```

Expected: all CLI tests pass, including pre-existing command-separator and
startup compatibility tests.

**Step 6: Verify and commit**

Run:

```powershell
cargo fmt --all -- --check
git diff --check
git add crates/rssh-app/src/cli.rs
git commit -m "feat: parse wezterm config lifecycle options"
```

### Task 1: Build the Strict Production Static-Config Parser

**Files:**

- Create: `crates/rssh-app/src/config_lifecycle.rs`
- Modify: `crates/rssh-app/src/main.rs:1-16` (declare the module)
- Modify: `crates/rssh-app/src/window.rs:4038-4310` (crate-visible native override type and legacy conversion hook)
- Test: `crates/rssh-app/src/config_lifecycle.rs`

**Step 1: Write RED lexer/value tests**

Add tests for:

- `strict_parser_accepts_empty_direct_table`;
- `strict_parser_accepts_config_builder_direct_assignments`;
- `strict_parser_consumes_nested_tables_arrays_strings_and_comments`;
- `strict_parser_rejects_trailing_top_level_statement`;
- `strict_parser_rejects_dynamic_return_root`;
- `strict_parser_rejects_variable_derived_value`;
- `strict_parser_rejects_event_callback_and_table_insert`;
- `strict_parser_rejects_malformed_balanced_value`.

Use both CRLF and LF, long comments/strings, escaped quotes, trailing
separators, and a leading BOM.

**Step 2: Run RED**

Run:

```powershell
cargo test -p rssh-app config_lifecycle::tests::strict_parser_accepts_empty_direct_table -- --exact --nocapture
```

Expected: FAIL because `config_lifecycle` and its parser do not exist.

**Step 3: Implement source positions and recursive syntax**

Introduce:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceLocation {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StaticLuaValue {
    Nil,
    Bool(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Array(Vec<StaticLuaValue>),
    Table(Vec<(StaticLuaKey, StaticLuaValue)>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StaticNativeConfigAssignment {
    pub field_path: String,
    pub value: StaticLuaValue,
    pub value_source: String,
    pub location: SourceLocation,
}
```

Implement a cursor-based parser that fully consumes comments, strings,
numbers, booleans, nested arrays/tables, and delimiters. It must return
location-bearing `InvalidSyntax`/`UnsupportedDynamicLua`, not `Option`.

Accept only the two forms in the design. Every non-comment top-level statement
and direct table field must become an IR item or fail.

**Step 4: Run syntax GREEN**

Run:

```powershell
cargo test -p rssh-app config_lifecycle::tests::strict_parser_
```

Expected: all strict syntax tests pass.

**Step 5: Write RED semantic-consumption tests**

Add table-driven tests:

```rust
#[test]
fn strict_registry_accepts_lifecycle_consumer_fields() { /* minimum field matrix */ }

#[test]
fn strict_registry_rejects_unknown_top_level_field() { /* ... */ }

#[test]
fn strict_registry_rejects_mixed_known_and_unknown_colors_keys() { /* ... */ }

#[test]
fn strict_registry_rejects_mixed_valid_and_unsupported_key_entries() { /* ... */ }

#[test]
fn strict_registry_rejects_trailing_tokens_inside_composite_value() { /* ... */ }

#[test]
fn strict_cli_overrides_validate_and_last_duplicate_wins() { /* ... */ }
```

The positive matrix must cover every field listed in “Production
static-config acceptance for this slice”.

**Step 6: Run semantic RED**

Run:

```powershell
cargo test -p rssh-app config_lifecycle::tests::strict_registry_accepts_lifecycle_consumer_fields -- --exact --nocapture
```

Expected: FAIL because there is no strict field registry or native conversion.

**Step 7: Implement strict decoders and canonical conversion**

Make `NativeConfigOverrides` and the legacy extractor `pub(crate)`. Add a
registry whose entry owns the complete recursive decoder for its field.
Decoders must reject unknown nested keys and invalid array elements.

Add:

```rust
pub(crate) fn validate_cli_config_overrides(
    items: &[(String, String)],
) -> Result<Vec<StaticNativeConfigAssignment>, NativeConfigLoadError>;

pub(crate) fn parse_native_config_document(
    source: &str,
    cli: &[StaticNativeConfigAssignment],
) -> Result<NativeConfigOverrides, NativeConfigLoadError>;
```

After strict validation, emit one canonical builder document from ordered IR
and call `native_config_overrides_from_wezterm_lua_config`. An empty IR returns
`NativeConfigOverrides::default()`. A non-empty validated IR yielding `None`
is an internal validation error, never a successful default.

Do not mutate the legacy parser's broader unit-test contract.

**Step 8: Run GREEN and legacy parser regressions**

Run:

```powershell
cargo test -p rssh-app config_lifecycle::tests
cargo test -p rssh-app window::tests::window_app_parses_wezterm_lua_config
```

Expected: strict tests and the large existing legacy parser family pass.

**Step 9: Verify and commit**

Run:

```powershell
cargo fmt --all -- --check
git diff --check
git add crates/rssh-app/src/config_lifecycle.rs crates/rssh-app/src/main.rs crates/rssh-app/src/window.rs
git commit -m "feat: validate file-backed wezterm config"
```

### Task 2: Resolve, Load, and Publish Configuration Sources

**Files:**

- Modify: `crates/rssh-app/src/config_lifecycle.rs`
- Modify: `crates/rssh-app/src/window.rs:399-416` (`run`)
- Modify: `crates/rssh-app/src/window.rs:89417-89440` (startup construction)
- Test: `crates/rssh-app/src/config_lifecycle.rs`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write RED discovery tests**

Add deterministic tests using injected home, executable, environment, and XDG
inputs:

- explicit file beats environment and candidates;
- environment file beats portable/home/XDG;
- Windows portable file beats home and XDG;
- home beats XDG;
- XDG home and Unix XDG dirs retain order;
- missing required path is a failed attempt;
- missing optional path falls through;
- skip disables file discovery but retains validated CLI IR;
- every reload call rediscovers rather than pinning the previous source.

**Step 2: Run RED**

Run:

```powershell
cargo test -p rssh-app config_lifecycle::tests::explicit_file_beats_environment_and_candidates -- --exact --nocapture
```

Expected: FAIL because discovery/lifecycle state does not exist.

**Step 3: Implement discovery and load-attempt state**

Add:

```rust
pub(crate) struct ConfigDiscoveryInputs { /* injected paths/env/platform */ }
pub(crate) enum ResolvedConfigSource {
    Disabled,
    Defaults,
    File(ConfigSource),
}
pub(crate) struct EffectiveNativeConfig {
    pub source: Option<PathBuf>,
    pub overrides: NativeConfigOverrides,
    pub generation: u64,
}
pub(crate) struct NativeConfigLoadAttempt { /* preferred/resolved/result */ }
pub(crate) struct NativeConfigLifecycle { /* inputs, CLI IR, LKG, diagnostic */ }
```

`NativeConfigLifecycle::attempt_reload` must rerun discovery, read UTF-8,
remove one BOM, call the strict parser, and return a temporary attempt without
mutating the effective generation.

**Step 4: Run discovery GREEN**

Run:

```powershell
cargo test -p rssh-app config_lifecycle::tests:: -- --nocapture
```

Expected: all discovery, BOM, required/optional, and rediscovery tests pass.

**Step 5: Write RED startup and environment tests**

Add:

- `window_run_configures_app_before_first_spawn`;
- `initial_invalid_source_uses_generation_zero_defaults_and_diagnostic`;
- `invalid_cli_override_fails_before_app_construction`;
- `explicit_cli_program_and_cwd_beat_file_defaults`;
- `successful_source_publishes_wezterm_config_environment`;
- `successful_default_or_skip_clears_config_environment`.

Avoid launching a real GUI. Factor a pure
`configured_startup_app_for_test(options, discovery)` helper that returns the
app and lifecycle before materialization.

**Step 6: Run startup RED**

Run:

```powershell
cargo test -p rssh-app window::tests::window_run_configures_app_before_first_spawn -- --exact --nocapture
```

Expected: FAIL because `window::run` still constructs the app directly from
CLI options.

**Step 7: Apply generation 0/1 before materialization**

Validate CLI overrides before event-loop/window creation. Bootstrap the
lifecycle, print/store a source error while continuing with defaults, apply
the effective base through the existing config fan-out, then create
`NativeWindowManager`.

Represent `WEZTERM_CONFIG_FILE`/`WEZTERM_CONFIG_DIR` as derived lifecycle
publication, not process-global mutation: Rust 2024 makes concurrent
`std::env::set_var/remove_var` unsafe and the workspace forbids unsafe code.
Feed publication to the bounded evaluator context and future PTY command
environment, beneath user `set_environment_variables` precedence. Tests must
prove child commands see the correct values and failed reload retains the LKG
publication. Preserve `startup_uses_default_shell`, so file `default_prog`
only replaces the default shell and explicit CLI command/cwd wins.

**Step 8: Run startup GREEN**

Run:

```powershell
cargo test -p rssh-app window::tests::window_run_configures_app_before_first_spawn -- --exact --nocapture
cargo test -p rssh-app window::tests::initial_invalid_source_uses_generation_zero_defaults_and_diagnostic -- --exact --nocapture
cargo test -p rssh-app window::tests::explicit_cli_program_and_cwd_beat_file_defaults -- --exact --nocapture
cargo test -p rssh-app window::tests::window_app_applies_wezterm_lua_config_default_prog
cargo test -p rssh-app window::tests::window_app_applies_wezterm_lua_config_term
```

Expected: all pass and no PTY is spawned by pure startup tests.

**Step 9: Verify and commit**

Run:

```powershell
cargo fmt --all -- --check
git diff --check
git add crates/rssh-app/src/config_lifecycle.rs crates/rssh-app/src/window.rs
git commit -m "feat: load wezterm config before window startup"
```

### Task 3: Apply Manual Reloads Across All Managed Windows

**Files:**

- Modify: `crates/rssh-app/src/config_lifecycle.rs`
- Modify: `crates/rssh-app/src/window.rs:81095-81580` (`NativeWindowManager`, `WindowUserEvent`)
- Modify: `crates/rssh-app/src/window.rs:83881-84420` (detached/pending app inheritance)
- Modify: `crates/rssh-app/src/window.rs:95606-96220` (base/effective apply)
- Modify: `crates/rssh-app/src/window.rs:101127-101140` (reload request)
- Modify: `crates/rssh-app/src/window.rs:128210-128390` (manager user events)
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write RED generation and transaction tests**

Add:

- `window_manager_successful_reload_advances_one_generation_for_all_apps`;
- `window_manager_reload_rebuilds_input_runtime_renderer_and_future_launch_state`;
- `window_manager_failed_reload_keeps_lkg_generation_and_effective_state`;
- `window_manager_failed_reload_still_notifies_each_window_once`;
- `window_manager_reload_rediscovers_optional_fallback_and_required_failure`;
- `window_manager_successful_reload_clears_latest_diagnostic`.

Use startup, materialized-test, and pending apps with distinct callback
counters. Assert palette, term/runtime, key assignment, `default_prog`, and
generation, not only copied override structs.

**Step 2: Run RED**

Run:

```powershell
cargo test -p rssh-app window::tests::window_manager_successful_reload_advances_one_generation_for_all_apps -- --exact --nocapture
```

Expected: FAIL because reload remains window-local and does not read a source.

**Step 3: Move reload ownership to the manager**

Add non-pane variants:

```rust
WindowUserEvent::ReloadConfigurationRequested,
WindowUserEvent::ConfigFileChanged,
```

Match them in `NativeWindowManager::user_event` before pane-owner routing.
`WindowCommand::ReloadConfiguration` and shortcut handling enqueue
`ReloadConfigurationRequested` through the existing proxy.

Implement one manager transaction:

```rust
fn reload_configuration_attempt(&mut self) {
    let attempt = self.config_lifecycle.attempt_reload();
    // publish success or retain LKG, then notify every managed app once
}
```

Success increments once and applies the new base to startup, pending, and
materialized apps before callbacks. Failure retains the base/generation but
reapplies it and performs the upstream reload-attempt callback.

**Step 4: Run transaction GREEN**

Run:

```powershell
cargo test -p rssh-app window::tests::window_manager_successful_reload_advances_one_generation_for_all_apps -- --exact --nocapture
cargo test -p rssh-app window::tests::window_manager_failed_reload_keeps_lkg_generation_and_effective_state -- --exact --nocapture
cargo test -p rssh-app window::tests::window_manager_failed_reload_still_notifies_each_window_once -- --exact --nocapture
```

Expected: all pass.

**Step 5: Write RED layering/inheritance tests**

Add:

- `window_override_survives_base_reload_and_remains_highest_precedence`;
- `pending_window_is_refreshed_to_current_generation_before_spawn`;
- `detached_window_inherits_base_generation_and_window_layer`;
- `new_split_and_tab_launches_use_reloaded_defaults`;
- `reload_clears_key_table_and_leader_state_once_per_attempt`.

**Step 6: Run layering RED**

Run:

```powershell
cargo test -p rssh-app window::tests::window_override_survives_base_reload_and_remains_highest_precedence -- --exact --nocapture
```

Expected: FAIL because `config_overrides` currently conflates base and runtime
window overrides.

**Step 7: Separate base from per-window overrides**

Add base generation/source fields and retain a distinct
`NativeLuaWindowConfigOverrides` layer. Refactor the current
`set_config_overrides` fan-out into:

```rust
fn set_base_config(&mut self, config: &EffectiveNativeConfig, disposition: ReloadDisposition);
fn set_window_config_overrides(&mut self, overrides: Option<NativeLuaWindowConfigOverrides>);
fn apply_effective_config(&mut self, disposition: ReloadDisposition);
```

Composition order is defaults < base < window. `SilentStartup` does not emit
the event; `ReloadAttempt` clears transient key/leader state and emits exactly
one callback after application. Preserve the existing test-facing
`set_config_overrides` helper by treating it as base replacement.

Before materializing a pending app, install the manager's current base.
Detached apps copy their window layer but are also brought to the manager's
current generation.

**Step 8: Run layering GREEN and manager regressions**

Run:

```powershell
cargo test -p rssh-app window::tests::window_override_survives_base_reload_and_remains_highest_precedence -- --exact --nocapture
cargo test -p rssh-app window::tests::pending_window_is_refreshed_to_current_generation_before_spawn -- --exact --nocapture
cargo test -p rssh-app window::tests::window_manager_
cargo test -p rssh-app window::tests::window_app_reload_configuration
```

Expected: all new and old manager/reload tests pass.

**Step 9: Verify and commit**

Run:

```powershell
cargo fmt --all -- --check
git diff --check
git add crates/rssh-app/src/config_lifecycle.rs crates/rssh-app/src/window.rs
git commit -m "feat: reload wezterm config across windows"
```

### Task 4: Watch and Debounce Configuration Changes

**Files:**

- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/rssh-app/src/config_lifecycle.rs`
- Modify: `crates/rssh-app/src/window.rs` (`ConfigFileChanged` event handling)
- Test: `crates/rssh-app/src/config_lifecycle.rs`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write RED debounce-worker tests**

Add deterministic tests:

- `watcher_coalesces_modify_burst_into_one_reload_event`;
- `watcher_accepts_create_modify_remove_and_ignores_other_kinds`;
- `watcher_watches_attempted_invalid_source_and_parent`;
- `watcher_skips_home_parent_but_watches_home_file`;
- `watch_paths_accumulate_across_rediscovery`;
- `watcher_remains_after_later_config_disables_auto_reload`;
- `per_window_auto_reload_override_does_not_control_global_watcher`;
- `dropping_watcher_stops_and_joins_worker`.

Use an injected event sink/channel and configurable test debounce duration;
production remains exactly 200 ms.

**Step 2: Run RED**

Run:

```powershell
cargo test -p rssh-app config_lifecycle::tests::watcher_coalesces_modify_burst_into_one_reload_event -- --exact --nocapture
```

Expected: FAIL because there is no watcher.

**Step 3: Add `notify` and the owned worker**

Add `notify = "8"` to `rssh-app`. Implement a `NativeConfigWatcher` that owns
`notify::RecommendedWatcher`, the debounce channel, and a join handle. Its
worker accepts create/modify/remove, waits 200 ms, drains the burst, and sends
one winit event through an injected sink.

Register the attempted file and eligible parent whenever the manager-owned LKG
base file+CLI config enables automatic reload, even when the attempt failed.
Per-window overrides never control this global policy. Retain the watcher and
accumulated paths after later base disable; simply do not add new paths while
disabled. Match upstream's home exception: watch `$HOME/.wezterm.lua` but not
the home directory itself.

Drop order must close the event sender, stop the worker, and join it.

**Step 4: Run watcher GREEN**

Run:

```powershell
cargo test -p rssh-app config_lifecycle::tests::watcher_
```

Expected: all watcher ownership, filtering, accumulation, and debounce tests
pass without sleeps longer than the injected test duration.

**Step 5: Write RED end-to-end auto-reload tests**

Add:

- `initial_invalid_watched_config_recovers_to_generation_one`;
- `automatic_reload_updates_all_windows_once_after_burst`;
- `automatic_reload_recovers_after_invalid_intermediate_file`;
- `automatic_reload_atomic_replace_rediscovers_non_home_source`;
- `disabled_generation_does_not_add_new_watch_paths_but_existing_watch_remains`.

Use real temporary files and the same manager transaction; do not create a
native window or PTY.

**Step 6: Run end-to-end RED**

Run:

```powershell
cargo test -p rssh-app window::tests::initial_invalid_watched_config_recovers_to_generation_one -- --exact --nocapture
```

Expected: FAIL because watcher events are not connected to the manager.

**Step 7: Connect watcher events to manager reload**

Give the lifecycle the event proxy after event-loop creation. Route the
debounced `ConfigFileChanged` event through
`reload_configuration_attempt`, identical to manual reload. Refresh watched
paths after every attempt using the current LKG auto-reload flag and attempted
path.

**Step 8: Run end-to-end GREEN**

Run:

```powershell
cargo test -p rssh-app window::tests::initial_invalid_watched_config_recovers_to_generation_one -- --exact --nocapture
cargo test -p rssh-app window::tests::automatic_reload_
cargo test -p rssh-app config_lifecycle::tests
```

Expected: all pass with one generation/event per debounced burst.

**Step 9: Verify and commit**

Run:

```powershell
cargo fmt --all -- --check
git diff --check
git add Cargo.lock crates/rssh-app/Cargo.toml crates/rssh-app/src/config_lifecycle.rs crates/rssh-app/src/window.rs
git commit -m "feat: auto reload wezterm config files"
```

### Task 5: Align Documentation and Run Full Verification

**Files:**

- Modify: `docs/architecture.md`
- Modify: `docs/research/wezterm-parity-gap.md`
- Modify: `docs/plans/2026-07-26-wezterm-config-lifecycle-design.md` only if implementation-required wording changed

**Step 1: Write the evidence checklist before editing docs**

List the exact implemented tests/commits proving:

- global CLI and source precedence;
- strict production grammar and explicit unsupported diagnostics;
- generation-0 startup recovery versus invalid CLI failure;
- pre-spawn config consumption;
- success/failure manual reload semantics;
- base/window layering and pending/detached inheritance;
- watcher debounce, accumulation, and initial invalid-file recovery.

**Step 2: Update architecture and parity status**

Mark the file-backed lifecycle slice complete only for the strict production
grammar actually implemented. Keep arbitrary Lua VM/`require`/helper watch
paths and any registry-excluded config fields open. Remove the stale
inactive-pane wheel backlog entries contradicted by merge `c33f1789`.

Record the fixed upstream pin and exact semantic details: rediscovery each
attempt, generation retention on failure plus notification, watcher creation
from failed attempted sources, watcher retention after later disable,
base-only watcher policy, the home-parent exception, and safe child-environment
publication instead of unsafe process-global mutation.

**Step 3: Run focused and package verification**

Run:

```powershell
cargo test -p rssh-app config_lifecycle::tests
cargo test -p rssh-app window::tests::window_manager_
cargo test -p rssh-app window::tests::automatic_reload_
cargo test -p rssh-app
```

Expected: all pass with zero failures.

**Step 4: Run complete workspace verification**

Run:

```powershell
cargo test --workspace --all-targets -- --skip builtin_color_scheme_lookup_covers_pinned_wezterm_names_and_aliases --skip builtin_color_scheme_lookup_matches_all_pinned_wezterm_palette_data
cargo fmt --all -- --check
git diff --check
git status --short
```

Expected: exit 0 for tests/fmt/diff check; status contains only the intended
documentation changes before the task commit.

**Step 5: Commit**

Run:

```powershell
git add docs/architecture.md docs/research/wezterm-parity-gap.md docs/plans/2026-07-26-wezterm-config-lifecycle-design.md
git commit -m "docs: record wezterm config lifecycle parity"
```

After this task, dispatch one final whole-slice reviewer against the design and
all commits. Fix every critical/important finding through the responsible
original implementer and repeat both spec and quality review as applicable.
Then use `superpowers:finishing-a-development-branch`, rerun the complete
verification from a clean checkout state, and locally merge into
`codex/wezterm-parity-progress` without pushing.
