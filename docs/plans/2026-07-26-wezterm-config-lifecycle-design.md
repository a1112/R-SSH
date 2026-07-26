# WezTerm File-Backed Configuration Lifecycle Design

## Goal and pinned evidence

Make the existing native WezTerm-compatible configuration surface reachable
from the real GUI startup path, and keep every managed window on one
last-known-good configuration generation across manual and automatic reloads.

The behavioral reference is WezTerm
`093bf6bf2b82b929ed80c04fd54ebc80464f715e`:

- `wezterm-gui/src/main.rs:68-87` defines global `--skip-config`,
  `--config-file`, and repeated `--config name=value` options.
- `config/src/config.rs:935-999` resolves the portable Windows executable
  directory, `WEZTERM_CONFIG_FILE`, `$HOME/.wezterm.lua`, and XDG config
  candidates, distinguishing required from optional paths.
- `config/src/config.rs:1013-1066` reads UTF-8, removes an initial BOM,
  evaluates the selected file, applies CLI overrides, validates the result,
  and publishes `WEZTERM_CONFIG_FILE` and `WEZTERM_CONFIG_DIR`.
- `config/src/lib.rs:478-602` watches the selected source, debounces changes
  for 200 ms, retains the previous configuration on failure, advances a
  generation only on success, and notifies subscribers.
- `wezterm-gui/src/termwindow/mod.rs:1641-1750` applies a new generation to
  each window and then emits `window-config-reloaded`.

R-SSH already has a large bounded-static WezTerm Lua extractor and a single
`NativeWindowApp::set_config_overrides` fan-out path, but `window::run` never
loads a file and `reload_configuration` only clears the key-table stack and
fires an event. The design connects those pieces without claiming that the
bounded-static extractor is a complete Lua VM.

## Scope

This slice includes:

- global GUI configuration CLI options;
- deterministic source discovery and required/optional missing-file behavior;
- startup loading before window creation and the first PTY spawn;
- a diagnostic result model for the bounded-static evaluator;
- repeated CLI configuration overrides with higher precedence than the file;
- one manager-owned last-known-good configuration and generation;
- successful manual reload across startup, materialized, and pending windows;
- automatic non-recursive file watching with 200 ms debounce;
- failure retention, visible diagnostics, and upstream-compatible
  reload-attempt notification;
- base-configuration versus per-window override layering;
- environment publication for the active successful source; and
- architecture/parity documentation updates.

This slice does not implement arbitrary Lua execution, `require` loading of
new modules, helper-file watch registration, or the complete WezTerm Lua API.
Those remain a subsequent configuration-runtime milestone. Dynamic or
otherwise unsupported source must return an explicit stable diagnostic; it
must never be treated as an empty successful configuration.

Named remote domains, mux startup, font shaping, and unrelated terminal or
mouse parity work are outside this slice.

## CLI model and precedence

Introduce `WindowConfigOptions` inside `WindowOptions`:

- `skip_config: bool`;
- `config_file: Option<PathBuf>`; and
- ordered `config_overrides: Vec<(String, String)>`.

`--skip-config`/`-n`, `--config-file PATH`, and repeated
`--config NAME=VALUE` are global options accepted before the GUI command,
matching WezTerm. They are not duplicated after `window`/`start`.
`--skip-config` conflicts with `--config-file`. `--config` remains valid with
either: it applies over the defaults when config loading is skipped.

The effective precedence, from lowest to highest, is:

1. native defaults;
2. the selected file;
3. ordered CLI `--config` values, where the last duplicate wins;
4. a window's runtime `set_config_overrides` layer.

An explicit CLI program/cwd remains above `default_prog`/`default_cwd`.
Existing `startup_uses_default_shell` and pre-spawn default-program logic
remain the authority for that decision. A reload never replaces a running
PTY; it affects current runtime-consumable settings and future pane/window
launches.

## Source resolution

Add a small `config_lifecycle` module with injected discovery inputs for
tests. It produces either `Disabled` or a `ConfigSource` containing a path and
whether the path is required.

Resolution order is:

1. `--skip-config` disables file discovery;
2. explicit `--config-file` is required;
3. `WEZTERM_CONFIG_FILE` is required;
4. on Windows, `wezterm.lua` beside the current executable is optional;
5. `$HOME/.wezterm.lua` is optional;
6. `$XDG_CONFIG_HOME/wezterm/wezterm.lua`, or
   `$HOME/.config/wezterm/wezterm.lua` when `XDG_CONFIG_HOME` is unset, is
   optional;
7. on Unix, each `XDG_CONFIG_DIRS` entry contributes an optional
   `wezterm/wezterm.lua`.

The first present candidate wins. A missing required path is a load error;
missing optional paths are skipped. If none exists, native defaults plus CLI
overrides form a successful generation. Discovery runs again on every manual
or automatic reload, as it does in the fixed upstream. This allows an optional
source that disappeared to fall through to a lower-priority candidate or
defaults and allows a later manual reload to select a newly created
higher-priority source. A required source never falls through.

After a successful file load, publish `WEZTERM_CONFIG_FILE` as the selected
path and `WEZTERM_CONFIG_DIR` as its parent to the bounded evaluator context
and future PTY command environment. A successful disabled/default load removes
them from those derived environments. A failed runtime reload keeps the
last-known-good publication because the effective source remains active.

R-SSH does not mutate the process-global environment after threads exist.
The workspace uses Rust 2024 with `unsafe_code = "forbid"`, while
`std::env::set_var/remove_var` are unsafe in this context. This is an explicit
safety adaptation from fixed upstream: loader state is authoritative inside
the app, and child commands receive equivalent values without introducing
unsound process-global mutation.

## Loading and bounded-static diagnostics

Replace the production-facing `Option<NativeConfigOverrides>` contract with:

```rust
enum NativeConfigLoadErrorKind {
    Io,
    InvalidUtf8,
    InvalidSyntax,
    UnsupportedDynamicLua,
    InvalidOverride,
}

struct NativeConfigLoadError {
    kind: NativeConfigLoadErrorKind,
    path: Option<PathBuf>,
    message: String,
}

struct EffectiveNativeConfig {
    source: Option<PathBuf>,
    overrides: NativeConfigOverrides,
    generation: u64,
}

struct NativeConfigLoadAttempt {
    preferred_source: Option<PathBuf>,
    resolved_source: Option<PathBuf>,
    result: Result<NativeConfigOverrides, NativeConfigLoadError>,
}
```

The current extractor remains available to its existing unit tests, while a
new production parser accepts one deliberately enumerable grammar and produces
an assignment IR:

```rust
struct StaticNativeConfigAssignment {
    field_path: String,
    value_source: String,
    location: SourceLocation,
}
```

The accepted file forms are:

```lua
return {
  field = STATIC_VALUE,
  nested = { ... },
}
```

or:

```lua
local wezterm = require 'wezterm'
local config = wezterm.config_builder()
config.field = STATIC_VALUE
config.other_field = STATIC_VALUE
return config
```

The production parser is a single pass over top-level statements and direct
config fields. Every byte other than whitespace/comments/separators belongs
to one recognized production. Values are parsed recursively into a
`StaticLuaValue` tree; consuming a balanced outer table is not sufficient.
A registry maps each production-supported top-level field to a strict
`Result` decoder that recursively validates every nested object key, array
element, union tag, and trailing token before producing its native assignment.
Fields without a strict decoder are explicitly unsupported even if the legacy
extractor can partially recognize them. Mixed composite values such as a
valid `colors` key plus an unknown key, or a valid key assignment plus an
unsupported array item, fail the whole document.

The legacy bounded-static extractor is only the final converter for a document
whose complete assignment IR has already passed those strict decoders; it is
not a validator. An unknown field, unsupported value, extra executable
statement, variable-derived value, `table.insert`, event callback, helper
function, alternate returned root, or malformed balanced construct fails the
complete document with `UnsupportedDynamicLua` or `InvalidSyntax`; partial
success is impossible. Existing complex-variable/event/config tests continue
to exercise the legacy extractor only and are not advertised as production
file support in this slice.

Loading performs these steps:

1. read bytes and decode UTF-8;
2. remove one leading UTF-8 BOM;
3. parse the complete document into ordered assignments;
4. append the already validated CLI assignment IR, so the last duplicate
   wins;
5. emit a canonical config-builder document from the IR and call the existing
   extractor once;
6. map an empty assignment list to valid default overrides and require a
   non-empty list to produce `Some`, otherwise return an internal validation
   error.

Before source discovery or lifecycle construction, parse every
`--config NAME=VALUE` through the same strict field registry. Each name must
have a dotted identifier path and each value must be completely decodable.
Unknown fields and dynamic expressions are `InvalidOverride`; they terminate
startup rather than falling back to generation 0. The validated ordered CLI
assignment IR is then reused unchanged by every load attempt. CLI source is
never spliced into the original Lua text.

This strict adapter is intentionally honest: it creates a useful production
configuration path now while preserving a clean boundary for replacing the
bounded-static evaluator with a real Lua runtime later.

## Lifecycle ownership

`NativeWindowManager`, not an individual window, owns:

- the discovery inputs and ordered CLI overrides;
- a three-state source selection (`Disabled`, `Defaults`, or `File`);
- the last-known-good `EffectiveNativeConfig`;
- the preferred and most recently resolved source from the latest attempt;
- the latest load diagnostic;
- the filesystem watcher; and
- the reload generation.

`window::run` first validates CLI overrides; an invalid override returns an
error before any lifecycle, window, or PTY exists. It then attempts the initial
file load before constructing `NativeWindowApp`. On success it installs
generation 1. On source discovery/read/evaluation failure it records the
preferred-source diagnostic while retaining generation 0 native defaults,
prints/shows that diagnostic, and continues GUI startup, matching the fixed
upstream. In either case the effective base is applied before `create_window`
or `spawn_pty`, then `NativeWindowManager` receives the lifecycle state.

Split the current overloaded application state into:

- `base_config_overrides`, supplied by the manager; and
- the existing per-window runtime override layer.

The effective override object is recomputed in precedence order and passed
through the existing application fan-out. A base reload does not erase the
per-window layer. Detached and newly requested windows copy both the current
base generation and their applicable window layer; pending windows receive
the manager's current base again before materialization so they cannot spawn
from a stale generation.

Refactor `set_config_overrides` into an apply primitive that does not
implicitly request another reload. Startup application uses a silent
disposition. A successful runtime generation applies all derived settings,
clears transient key-table/leader state, invalidates snapshots/render state as
the existing fan-out requires, and only then dispatches one
`window-config-reloaded` event per window.

## Manual reload flow

`WindowCommand::ReloadConfiguration` and the default shortcut send a
manager-level `WindowUserEvent::ReloadConfigurationRequested`. They do not
reload only the focused window.

The manager:

1. reruns discovery, then reads, validates, and evaluates the selected source
   and CLI overrides into a temporary result;
2. on failure, stores and prints one stable diagnostic, leaves the effective
   generation unchanged, reapplies the last-known-good base plus each
   per-window layer, and dispatches exactly one reload-attempt callback per
   app, matching upstream subscriber notification on failure;
3. on success, increments the generation, clears the diagnostic, updates
   environment publication, installs the base on
   startup/pending/materialized apps, and dispatches exactly one callback per
   app;
4. requests redraw only where the normal apply fan-out marks state dirty.

Since application of an already validated native override object is
infallible, parsing before mutation is the transaction boundary: no window can
observe a partially parsed generation.

## Automatic reload flow

Use one `notify::RecommendedWatcher`. After every attempt, successful or
failed, if the last-known-good effective configuration has
`automatically_reload_config=true`, add the attempted source path and, except
when the parent is the user's home directory, its parent directory. Watch
paths accumulate, matching the fixed upstream, so an initially missing or
invalid required source can recover automatically, and an earlier selected
source or atomic editor replacement can trigger rediscovery. Only create,
modify, and remove/rename events enter a dedicated debounce worker. The worker
waits 200 ms, drains the burst, and sends one
`WindowUserEvent::ConfigFileChanged` through the winit proxy.

The manager handles that event on the event-loop thread and runs the same
transaction as manual reload. The watcher callback never mutates window or
configuration state.

The watcher policy is derived only from the manager-owned base file plus CLI
configuration; a per-window runtime override cannot enable or disable this
global resource.

The watcher is first created whenever an attempt identifies a source path and
the last-known-good base configuration has automatic reload enabled;
attempt success is not required. Once created it is retained even if a later
generation disables automatic reload, matching the fixed upstream; only newly
attempted paths stop being added while the flag is false. Dropping the
lifecycle cleanly stops and joins the debounce worker.

When the selected file is directly under the user's home directory, fixed
upstream watches the file but deliberately does not watch the noisy home
parent. Modify events remain supported, but a delete followed by later
recreation is not guaranteed to be observed for that one source location.
Atomic replace/remove/recreate recovery tests therefore use non-home config
directories.

## Error behavior and observability

- Invalid CLI overrides terminate before lifecycle creation. Initial source
  failure from an explicit/environment-required or
  present-but-invalid source records and displays a diagnostic but continues
  with generation 0 native defaults.
- Runtime failure preserves the last-known-good configuration, PTYs, window
  generation, watcher, and environment publication. It still performs the
  upstream reload-attempt notification, which clears transient key-table
  state, reapplies the same effective base plus per-window layer, and emits one
  config-reloaded callback per window.
- Diagnostics include source path, stable category, and actionable detail.
- A successful reload clears the last diagnostic.
- Every reload rediscovers candidates. Removing an optional source can select
  a lower-priority source or defaults successfully; removing a required source
  is a failure and retains the last-known-good effective configuration.
- Duplicate watcher bursts coalesce to one load and one event per window.

## Verification matrix

Focused tests must prove:

- global CLI parsing, conflicts, repeated override ordering, help text, and
  interaction with the default GUI command plus explicit `window`/`start`;
- every source precedence case, Windows portable ordering, XDG fallback,
  required versus optional missing paths, skip behavior, BOM removal, and
  environment publication;
- valid empty config succeeds while malformed, unknown-field, unsupported
  dynamic, partially understood, and invalid override inputs fail visibly;
- every unconsumed statement/field is rejected by table-driven production
  grammar tests; mixed valid/unknown nested object keys and mixed
  valid/unsupported array items are rejected; the legacy extractor's broader
  test-only acceptance remains unchanged;
- startup file values for `default_prog`, `default_cwd`, `term`, initial
  rows/columns, palette, and key bindings apply before the first spawn, while
  explicit CLI command/cwd wins;
- an invalid initial required or optional-present config continues with
  generation 0 defaults and a visible diagnostic;
- an invalid CLI override aborts before a window or PTY is created;
- successful manual reload changes effective config/input/renderer/runtime
  state across multiple windows, clears transient key state, advances one
  generation, and fires one event per window;
- failed manual reload retains the previous effective objects, generation,
  PTYs, and environment, but clears transient key state and fires the one
  upstream reload-attempt event per window;
- each reload rediscovers sources, including optional removal fallback,
  required removal failure, and later higher-priority source selection;
- per-window overrides survive a base reload and continue to win;
- pending/detached/new windows use the current generation;
- automatic reload can create the watcher from an enabled failed attempt,
  repairs an initially invalid generation-0 source into generation 1, retains
  an existing watcher after a later disable, accumulates attempted paths,
  ignores per-window automatic-reload overrides, coalesces a burst, handles
  atomic replace/remove/recreate outside the home-root exception, and recovers
  after an invalid intermediate file;
- watcher shutdown does not leak a worker; and
- existing static-parser, app-shell, PTY-launch, renderer-palette, key-table,
  manager-routing, and complete workspace suites remain green.

Run focused RED/GREEN tests for each task, then `cargo test -p rssh-app`, the
full workspace all-targets suite, `cargo fmt --all -- --check`, and
`git diff --check`.
