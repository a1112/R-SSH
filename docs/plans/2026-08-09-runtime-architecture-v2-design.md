# Runtime Architecture V2 Design

**Status:** Approved on 2026-08-09
**Baseline commit:** `9e99ba755fbdbd8896d849bc8934e187ab9062b5`
**Scope:** Deep redesign of the native runtime API, window shell, configuration
projection, concurrency, tests, and performance gates.

## Summary

R-SSH will replace the monolithic native-window runtime with four explicit
layers:

1. a thin `rssh-app` composition root and CLI;
2. a `rssh-native` host/controller/presentation library;
3. a platform-neutral `rssh-runtime` session engine with bounded mailboxes;
4. an immutable, layered `rssh-config` model with typed patches.

The migration is incremental. The legacy and V2 runtime paths will be compared
in deterministic tests before production ownership moves to V2. Every cutover
must improve the matched baseline in its target category while keeping every
unrelated category at least neutral. The final merge is permitted only when the
complete performance, resource, build, correctness, stability, and
maintainability scorecard is better than the baseline.

## Evidence from the Current System

The current `crates/rssh-app/src/window.rs` is both a platform adapter and the
application's largest domain/runtime implementation:

- 272,115 total lines;
- 134,221 production lines and 137,894 lines in one test module;
- 3,425 `#[test]` declarations in `window.rs`;
- 41,747 lines in `builtin_color_scheme_toml`;
- 21,386 lines in one `NativeWindowApp` implementation block;
- 417 fields in `NativeWindowApp`;
- approximately 748 methods in the main `NativeWindowApp` implementation;
- 1,771 top-level functions in `window.rs`;
- `rustfmt::skip` is required to keep rustfmt away from the aggregate.

Configuration is projected repeatedly:

- `NativeEffectiveConfig`: 261 fields;
- `NativeConfigOverrides`: 268 fields;
- `NativeLuaWindowConfigOverrides`: 255 fields;
- many of those values are copied again into `NativeWindowApp`.

The dependency direction is also inverted. `config_lifecycle` imports
`NativeConfigOverrides` from `window`, while `window` imports and owns
`config_lifecycle`.

The current native PTY data path has no explicit end-to-end backpressure:

- the PTY reader allocates a new `Vec<u8>` for each 8 KiB read;
- every read becomes a `WindowUserEvent::Output` in winit's event queue;
- the pane input worker uses an unbounded `std::sync::mpsc::channel`;
- the UI thread parses terminal output synchronously;
- active-pane metadata is re-read after every chunk;
- user variables are cloned for inactive panes after every chunk;
- render snapshots may be rebuilt after every delivered chunk.

This shape makes queue depth, allocation count, input-to-present latency, and
shutdown ordering implicit rather than enforceable API properties.

## Measured Baseline

All local measurements below were taken from the exact baseline commit on
Windows x64 with Rust/Cargo 1.89.0. Timing and resource comparisons after the
refactor must run on the same machine, toolchain, power profile, and command
fingerprint.

### Runtime medians

Protocol: two discarded warmups, seven measured samples, 1 MiB input, 8 KiB
chunks, 30 offscreen render frames, and a 1,000 ms idle sample.

| Workload | Throughput B/s | Chunk p95 us | Render p95 us | RSS bytes | Elapsed ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| `ansi-scroll-query` | 4,444,069 | 2,321 | 328 | 52,297,728 | 235 |
| `plain-scroll` | 3,806,866 | 2,562 | 373 | 52,027,392 | 275 |
| `ansi-scroll` | 3,518,809 | 3,016 | 430 | 52,203,520 | 297 |

All three workloads reported zero surviving-cell clones and zero history-row
relocations. The query workload inspected 1,499,241 bytes for 1 MiB of input.

### Build and test baseline

| Metric | Current value |
| --- | ---: |
| Clean `cargo check --locked -p rssh-app --tests` | 51.692 s |
| Cache-hit check | 0.776 s |
| Rebuild after cleaning only `rssh-app` | 18.101 s |
| `cargo test --no-run -p rssh-app` after check | 94.736 s |
| Temporary test target artifacts | 7,252,825,489 bytes |
| `rssh-app` unit-test harness | 74,248,192 bytes |
| 4,258-test unit harness execution | 18.737 s |
| Release `rssh-app.exe` | 28,459,520 bytes |

The unit harness result was 4,256 passed, zero failed, and two ignored.

### Baseline exception discovered in the isolated worktree

The exact baseline commit builds after the same Web asset setup used by CI. A
full local `cargo test --locked --workspace --all-targets` run passed the 4,258
unit tests and the preceding native-window groups, but the focused
`native_window_e2e_uses_borderless_integrated_titlebar` probe failed after
receiving a non-zero HWND. The failure reproduces locally and predates V2.

GitHub's non-interactive runner may exit that probe with the documented
`RSSH_WINDOW_STYLE_UNOBSERVABLE` skip. Therefore a green hosted result does not
prove that the local client/window rectangles match. Gate 0 of this project
must make the probe deterministic and establish whether the production window
has a real native frame inset. The V2 implementation cannot hide, delete, or
weaken this assertion.

## Goals

### Correctness and compatibility

- Preserve all current CLI, terminal, PTY, SSH, rendering, configuration, and
  WezTerm-compatibility behavior unless a separately approved defect fix is
  documented.
- Preserve public protocol ordering, including terminal replies relative to
  user input.
- Preserve generation-based rejection of stale pane events.
- Preserve last-known-good configuration reload behavior.
- Preserve native GPU direct-text rendering and device-loss recovery.
- Preserve all current deterministic fixtures while converting repetitive
  test functions into table-driven cases where useful.

### Runtime performance

- Apply backpressure before input/output queues can grow without bound.
- Coalesce PTY chunks before terminal parsing and presentation publication.
- Rebuild at most one replaceable presentation snapshot per published batch.
- Move terminal parsing and PTY write ordering off the winit event loop.
- Publish metadata deltas rather than cloning all pane metadata after every
  chunk.
- Make queue depth, batch size, processing time, dropped/replaced frame count,
  and wake count observable.

### Developer performance and maintainability

- Make module and crate boundaries enforce dependency direction.
- Restore normal rustfmt operation for every handwritten Rust source file.
- Reduce incremental compilation and test-link cost.
- Keep platform APIs out of runtime/config domain code.
- Make runtime/controller tests deterministic without a real winit event loop.

## Non-Goals

- Replacing the terminal emulator or GPU renderer wholesale.
- Executing arbitrary untrusted Lua in-process.
- Changing public command-line syntax merely to simplify internals.
- Dropping compatibility cases to reduce test count.
- Claiming GPU-present or input-to-present improvements before a reproducible
  probe exists.
- Refreshing a baseline to excuse an unexplained regression.

## Target Crate Boundaries

```text
rssh-app
  CLI parsing, command dispatch, dependency construction
    |
    v
rssh-native
  WinitHost -> WindowController -> Presentation
      |               |                |
      |               v                v
      |          rssh-runtime     rssh-renderer
      |               |
      v               v
  rssh-config   rssh-core / rssh-terminal / rssh-pty / rssh-ssh
```

### `rssh-app`

The binary becomes a composition root. It owns argument parsing, selects a
transport/backend, creates configuration/runtime/native services, and maps
top-level failures to process exit codes. It does not own pane state, terminal
state, window state, or configuration fields.

### `rssh-config`

`rssh-config` owns platform-neutral native configuration:

- nested immutable value objects such as `FontConfig`, `TerminalConfig`,
  `InputConfig`, `WindowConfig`, `RenderConfig`, `DomainConfig`, and
  `LifecycleConfig`;
- `Patch<T>` with explicit inherit/clear/set semantics;
- TOML and bounded/static Lua parsing;
- merge, validation, diagnostics, and diff calculation;
- generated built-in color-scheme assets and their index.

It must not depend on winit or the native window host. Renderer/platform values
are represented by configuration-domain enums and converted at adapter edges.

The three flat 255-268-field structures are replaced with nested value/patch
trees. A window stores one `Arc<EffectiveConfig>` plus small mutable runtime
overrides. Reload computes one validated `ConfigDiff`; consumers receive only
the relevant sub-diff.

### `rssh-runtime`

`rssh-runtime` owns pane sessions and terminal progression. It defines a stable
transport-independent API and concrete local-PTY/native-SSH adapters.

Conceptual API:

```rust
pub struct RuntimeHub;
pub struct PaneHandle;

pub enum RuntimeCommand {
    Open(OpenPane),
    Input(InputBytes),
    Resize(TerminalSize),
    ApplyConfig(RuntimeConfigDiff),
    RequestSnapshot(SnapshotRequest),
    Restart,
    Close,
}

pub enum RuntimeNotice {
    PaneReady(PaneToken),
}

pub struct RuntimeBatch {
    pub pane: PaneToken,
    pub revision: u64,
    pub snapshot: Option<Arc<TerminalRenderSnapshot>>,
    pub damage: Vec<DamageRegion>,
    pub metadata: PaneMetadataDelta,
    pub effects: Vec<RuntimeEffect>,
    pub metrics: RuntimeBatchMetrics,
}

pub trait SessionTransport: Send + 'static {
    type Reader: Read + Send + 'static;
    type Writer: Write + Send + 'static;
    type Control: SessionControl + Send + 'static;

    fn split(self) -> SessionParts<Self::Reader, Self::Writer, Self::Control>;
}
```

Exact public names may change during implementation, but the ownership and
flow invariants below may not.

### `rssh-native`

`rssh-native` contains three distinct responsibilities:

- `WinitHost`: translates platform events and executes platform effects;
- `WindowController`: a deterministic reducer from intent plus runtime batch
  to state plus effects;
- `Presentation`: builds window/tab/pane/overlay render data from controller
  state and immutable runtime snapshots.

`WinitHost` owns window/GPU/clipboard/URL/notification ports. It does not own a
terminal or perform PTY reads/writes. `WindowController` does not call winit,
the filesystem, the clipboard, or the network directly.

## Runtime Ownership and Concurrency

### Pane worker

Each live pane has one worker that owns:

- `TerminalRuntime`;
- the ordered PTY/SSH writer;
- transport control and shutdown state;
- configuration relevant to terminal/session behavior;
- reusable parse, effect, damage, and batch buffers.

A blocking transport reader may remain a separate thread. The pane worker
replaces the current standalone writer thread, so the normal local-pane thread
count does not increase. The worker selects between bounded reader events and
bounded controller commands. A channel implementation with deterministic
selection and capacity semantics may be introduced after measuring its build
and runtime cost.

The split transport also yields a cloneable, thread-safe interrupt handle that
is not owned by the pane worker. Calling it is fast and idempotent and wakes
blocked reader and writer operations. This lets the hub or deadline path begin
shutdown even when the pane worker is blocked inside an ordered write; worker-
owned `SessionControl` remains responsible for resize, exit status, and normal
close progression after I/O is released.

### Bounded queues

All production queues have both item and byte budgets. Initial limits are
chosen from measured workloads and are configurable for tests:

- controller-to-pane command queue: bounded item count plus a one-MiB input
  byte reservation;
- reader-to-pane queue: enough for a short burst, with the blocked reader
  applying kernel/transport backpressure;
- lossless runtime effects: bounded and drained in order;
- presentation frame: a single replaceable latest-frame slot.

Interactive key input is never silently dropped. A full input queue returns an
explicit `Backpressured` result so paste can be chunked/retried and ordinary
keys can be scheduled on the next event-loop turn.

### Batching and wake coalescing

The pane worker coalesces consecutive output up to a byte or time budget. One
batch performs terminal parsing, metadata diffing, damage normalization, and
snapshot publication once.

The worker sets a wake-pending flag only on the empty-to-ready transition. The
winit event carries only `PaneToken`; bytes and effects remain in bounded
mailboxes. The host drains all currently ready batches within a per-turn time
budget. If work remains, it schedules one continuation wake.

### Terminal runtime delta

`TerminalRuntime` stops accumulating side effects in multiple internal vectors
that callers later drain. Its feed API writes into caller-owned reusable
buffers and returns a `RuntimeDelta` containing:

- responses that must be written back to the transport;
- visible/loggable output slices or owned chunks only when retention is needed;
- normalized damage;
- bells, notifications, clipboard operations, and progress changes;
- terminal identity and metadata change flags.

Metadata is emitted only when its source changed. This removes per-chunk scans
and clones of current working directory, user variables, badge format,
progress, and title state.

### Presentation snapshots

Presentation is lossy only in time, never in terminal state: a newer snapshot
may replace an unpresented older snapshot, but all input bytes are parsed in
order and all lossless effects are retained. Each snapshot carries a monotonic
revision. The host never presents a revision older than the latest accepted
revision for that pane generation.

## Controller and Effect Model

Window behavior is expressed as reducers and effects rather than hundreds of
methods mutating a single object.

Representative intents:

- platform window/keyboard/mouse/IME events;
- parsed `WindowCommand` actions;
- runtime batches and transport lifecycle changes;
- configuration reloads;
- timer ticks and redraw opportunities.

Representative effects:

- runtime command;
- request/resize/close/focus a native window;
- redraw or schedule a deadline;
- clipboard read/write;
- open URI or dispatch notification;
- persist frecency/recent state;
- create another window/controller.

Effects are executed by typed ports. Tests can assert exact effects without
creating windows or child processes.

## Error Handling and Shutdown

- Every runtime error includes window, pane, generation, phase, and source.
- Recoverable read/write/config/render failures become typed controller events.
- Stale-generation events are ignored and counted.
- Close is a state machine: stop accepting input, close writer, begin transport
  close, drain reader to EOF when required, join workers, then release control.
- A deadline crossing transfers ownership to the existing bounded reaper path;
  ownership is never detached silently.
- Shutdown tests assert zero live worker handles, zero retained child processes,
  and empty runtime mailboxes.
- Runtime metrics expose queue high-water marks, backpressure duration, batch
  size, wake compression ratio, parse p95, snapshot p95, and shutdown time.

## Built-In Schemes and Lua Compatibility

Built-in schemes become data rather than Rust syntax. A deterministic generator
will produce:

- a compact byte asset containing canonical scheme TOML;
- a sorted/indexed name-to-range table;
- a checksum and source-version manifest.

The generator output is verified, not hand-edited. Lookup remains allocation
free until the selected TOML is parsed.

The bounded Lua compatibility parser moves behind an AST/reducer boundary.
Migration first preserves the existing parser output through fixtures. Repeated
special-case recognizers are then replaced by generic syntax and evaluation
nodes only when equivalence tests prove the same accepted/rejected surface.
Arbitrary Lua execution remains out of scope.

## Test Architecture

### Equivalence harness

Recorded inputs run against legacy and V2 paths and compare:

- terminal snapshots and stable row identities;
- runtime effects and their ordering;
- metadata deltas and final metadata;
- app-shell actions and pane ownership;
- error and shutdown outcomes.

The legacy path is compiled only for equivalence tests during migration and is
deleted after cutover.

### Deterministic runtime tests

Fake transports and a virtual clock cover:

- burst output and bounded backpressure;
- input/terminal-response ordering;
- stale generation rejection;
- resize/config changes during output;
- reader, writer, parser, and close failures;
- frame replacement without effect loss;
- multi-pane fairness and shutdown.

### Test compilation

The 4,258-function binary harness is split by library/crate responsibility.
Large families of repetitive parser/compatibility assertions become
table-driven fixtures. Coverage and fixture inventories, not raw test-function
count, prove preservation.

## Acceptance Scorecard

### Correctness

- All existing workspace tests and E2E contracts pass after the Gate 0 probe is
  made deterministic.
- No ignored regression is added.
- Existing line coverage does not decrease from the exact baseline artifact.
- `rssh-runtime` and reducer modules reach at least 90% line coverage.
- Legacy/V2 equivalence fixtures have zero unexplained differences.

### Runtime and resource improvement

Using the same two-warmup/seven-sample protocol:

- query throughput is at least 110% of 4,444,069 B/s;
- plain throughput reaches at least 5,242,880 B/s;
- ANSI throughput is at least 110% of 3,518,809 B/s;
- every workload's chunk p95 is at most 90% of its baseline;
- every workload's offscreen render p95 is at most 95% of its baseline;
- every workload's RSS is at most 95% of its baseline;
- deterministic clone/relocation counters remain zero;
- inspected query bytes remain within the existing four-times-input budget.

A new native probe must record current and V2 input-to-present p95 on the same
machine. V2 must improve the paired median by at least 10% and remain below one
60 Hz frame budget when supported by the adapter/runner.

A 64 MiB burst test must prove bounded memory, no input/effect loss, and at
least 16:1 PTY-chunk-to-winit-wake compression under sustained output.

### Build and artifact improvement

- clean `rssh-app` tests check: at most 41.4 seconds;
- package-only rebuild: at most 12.7 seconds;
- test no-run: at most 66.3 seconds;
- unit harness execution: at most 15.0 seconds;
- total comparable test target bytes: at most 75% of baseline;
- largest individual app test harness: at most 55.7 MB;
- release executable: at most 25.62 MB.

The comparison protocol must isolate caches in a fresh target directory, record
the machine/toolchain fingerprint, and clean its exact temporary directory
afterward.

### Structural improvement

- zero `rustfmt::skip` in handwritten code;
- no handwritten Rust source above 8,000 lines;
- no state type above 64 direct fields;
- no implementation block above 2,000 lines;
- no function above 300 lines without an approved generated-code exemption;
- zero unbounded channels in production app/native/runtime code;
- `config_lifecycle` has no dependency on a window module;
- `rssh-app/src/main.rs` is a thin composition root;
- architecture checks run in CI and report exact violating files/items.

### Stability improvement

- the existing required 100-attempt PTY lifecycle probe remains zero-retry and
  zero-survivor, and completes at least 10% faster on the same machine;
- pane/window/application shutdown leaves no worker threads, queued ownership,
  or child processes;
- device-loss and surface-recovery tests keep direct GPU rendering;
- queue saturation is observable and never becomes silent data loss.

## Migration Plan

### Gate 0: Baseline integrity

- Make the Windows decoration probe deterministic without weakening it.
- Add reproducible native input-to-present and queue-depth probes.
- Record the exact baseline evidence and comparison scripts in the repository.

### Phase 1: New seams

- Add `rssh-config`, `rssh-runtime`, and `rssh-native` crates.
- Define transport, command, batch, controller intent/effect, and config patch
  contracts with fake implementations.
- Add architecture and bounded-channel contract tests.

### Phase 2: Runtime engine in shadow tests

- Implement the pane worker, bounded mailboxes, batching, delta emission, and
  deterministic shutdown.
- Run legacy/V2 transcript equivalence without selecting V2 in production.
- Optimize until the runtime and resource gates pass.

### Phase 3: Production runtime cutover

- Move one local pane, then multi-pane, then native SSH sessions to V2.
- Keep a short-lived internal fallback only until each cutover's equivalence
  and E2E gates pass.
- Remove the old path immediately after the corresponding gate.

### Phase 4: Controller and configuration cutover

- Move `AppShell` coordination into `WindowController` reducers.
- Move platform side effects into `WinitHost` ports.
- Replace flat configuration projections with immutable nested snapshots and
  diffs.
- Move color schemes to generated assets and split the Lua parser boundary.

### Phase 5: Aggregate removal and final optimization

- Split/migrate tests and delete `window.rs` compatibility aggregate.
- Remove `rustfmt::skip` and obsolete allowances.
- Update architecture/status documentation.
- Run the complete scorecard locally and in GitHub CI/fixed performance jobs.

## Commit and Rollback Discipline

- Each commit establishes one new boundary or migrates one behavior family.
- Every migration commit has focused RED/GREEN tests and equivalence evidence.
- Old and new production paths are not kept indefinitely.
- If a cutover misses any gate, revert that cutover while retaining the tested
  new seam and diagnostics.
- Performance baselines are not updated until the old baseline has accepted the
  new implementation on the same protected runner.

## Documentation

`docs/architecture.md` will become a concise stable architecture description.
Feature/parity inventories move to dedicated status ledgers. Verification
documents record exact commits and runner fingerprints rather than embedding
stale architecture claims.
