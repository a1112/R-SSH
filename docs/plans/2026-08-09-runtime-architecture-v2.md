# Runtime Architecture V2 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the monolithic native-window runtime with bounded, independently testable config, runtime, and native-controller layers while preserving behavior and exceeding the approved runtime, build, resource, stability, and maintainability baselines.

**Architecture:** Keep `rssh-app` as a thin composition root. Move immutable configuration and typed patches into `rssh-config`, pane/session ownership and bounded message flow into `rssh-runtime`, and deterministic window-controller/presentation logic into `rssh-native`. Migrate through a legacy/V2 transcript-equivalence harness, only cut production ownership over after every matched contract passes, and enforce the final scorecard in scripts and CI.

**Tech Stack:** Rust 1.89 / edition 2024, Cargo workspace, winit 0.30, existing terminal/PTY/SSH/renderer crates, Windows Win32 probes, PowerShell and Python CI helpers, GitHub Actions, cargo-llvm-cov.

---

## Ground Rules

- Work from `codex/runtime-architecture-v2` in the isolated worktree.
- Run each task's red test before implementation and record the expected failure.
- Do not change a behavioral assertion merely to make the new path pass.
- Keep legacy and V2 implementations side-by-side until equivalence is green.
- Commit after each green task or coherent pair of small tasks.
- Re-run the affected benchmark at every production cutover.
- If any unrelated scorecard metric regresses, keep the legacy path active and
  optimize before continuing.
- Generated assets may exceed structural source limits; handwritten Rust may
  not.

## Approved Baseline and Final Gates

The authoritative values and full protocol live in
`docs/plans/2026-08-09-runtime-architecture-v2-design.md`. The implementation
must enforce at least these final limits:

| Category | Gate |
| --- | --- |
| Query throughput | `>= 4,888,476 B/s` |
| Plain throughput | `>= 5,242,880 B/s` |
| ANSI throughput | `>= 3,870,690 B/s` |
| Chunk p95 | `<= 90%` of each matched baseline |
| Render p95 | `<= 95%` of each matched baseline |
| RSS | `<= 95%` of each matched baseline |
| Clean app-tests check | `<= 41.4 s` |
| Package-only rebuild | `<= 12.7 s` |
| App tests no-run | `<= 66.3 s` |
| Unit-harness execution | `<= 15.0 s` |
| Comparable test target size | `<= 5,439,619,116 bytes` |
| Largest app test harness | `<= 55.7 MB` |
| Release executable | `<= 25.62 MB` |
| Runtime/controller coverage | `>= 90%` line coverage |
| Native input-to-present p95 | `>= 10%` faster and `< 16.67 ms` when supported |
| Burst wake compression | `>= 16:1` for 64 MiB |
| PTY lifecycle | zero retry/survivor and `>= 10%` faster |

## Gate 0: Trustworthy Baselines

### Task 1: Replace the flaky Windows decoration observation boundary

**Files:**

- Create: `crates/rssh-test-support/src/windows/window_probe.rs`
- Modify: `crates/rssh-test-support/src/lib.rs`
- Modify: `crates/rssh-app/tests/native_window_e2e.rs`

**Step 1: Write the failing probe test**

Add a Windows-only helper contract that takes a process ID and deadline, finds
the owned visible top-level HWND, and returns every Win32 observation in one
typed result instead of executing nested PowerShell:

```rust
#[cfg(target_os = "windows")]
#[derive(Debug)]
pub struct WindowFrameObservation {
    pub hwnd: isize,
    pub style: isize,
    pub ex_style: isize,
    pub window_rect: Rect,
    pub client_rect: Rect,
    pub client_origin: Point,
}

#[cfg(target_os = "windows")]
pub fn wait_for_owned_window_frame(
    process_id: u32,
    deadline: Instant,
) -> Result<WindowFrameObservation, WindowProbeError>;
```

First assert that an invalid process ID times out with a diagnostic containing
the PID and last enumeration result. Then migrate the existing integrated
titlebar E2E to use this API and leave its frame assertion intact.

**Step 2: Run the red test**

Run:

```powershell
cargo test --locked -p rssh-app --test native_window_e2e `
  native_window_e2e_uses_borderless_integrated_titlebar -- --nocapture
```

Expected: compile failure because the probe module/API does not exist.

**Step 3: Implement a deterministic Win32 probe**

Use direct `windows-sys` calls in test support: `EnumWindows`,
`GetWindowThreadProcessId`, `IsWindowVisible`, `GetWindowRect`,
`GetClientRect`, `ClientToScreen`, and `GetWindowLongPtrW`. Enumerate all
candidate HWNDs on every poll. Return partial call errors in the timeout
diagnostic. Keep all `unsafe` code inside the test-support crate and document
each safety invariant; if the workspace `unsafe_code = "forbid"` prevents
this, put the FFI in the smallest Windows-only build-script-generated helper
executable and invoke it as a subprocess.

**Step 4: Establish the real window contract**

Compare the observation with winit's documented undecorated-shadow behavior.
The accepted geometry may account only for the measured/documented shadow
line; it must still reject WS_CAPTION and a normal 8/30-pixel native frame.
Include style, ex-style, rectangles, DPI, and candidate HWNDs in failures.

**Step 5: Verify repeatability**

Run the focused E2E ten times:

```powershell
1..10 | ForEach-Object {
  cargo test --locked -p rssh-app --test native_window_e2e `
    native_window_e2e_uses_borderless_integrated_titlebar -- --nocapture
  if ($LASTEXITCODE -ne 0) { throw "iteration $_ failed" }
}
```

Expected: 10/10 pass locally with no skip and no surviving `rssh-app` process.

**Step 6: Commit**

```powershell
git add crates/rssh-test-support crates/rssh-app/tests/native_window_e2e.rs
git commit -m "test: make Windows frame probing deterministic"
```

### Task 2: Version the performance baseline schema and runner

**Files:**

- Create: `scripts/perf/runtime-scorecard.ps1`
- Create: `scripts/perf/build-scorecard.ps1`
- Create: `scripts/perf/scorecard.schema.json`
- Create: `scripts/perf/baselines/windows-x64-rust-1.89.json`
- Create: `crates/rssh-app/tests/performance_scorecard_contract.rs`
- Modify: `.gitignore`

**Step 1: Write schema/contract tests**

Test that the checked-in baseline includes commit, OS, CPU, Rust/Cargo
versions, power-profile note, command fingerprints, warmup/sample counts, all
approved raw values, and all derived gates. Reject missing or non-finite
values.

**Step 2: Run the red contract**

```powershell
cargo test --locked -p rssh-app --test performance_scorecard_contract
```

Expected: failure because the schema and baseline files are absent.

**Step 3: Implement the runtime runner**

Run two warmups plus seven alternating samples for all three workloads. Emit a
single JSON document with raw samples, medians, percentiles, counters, binary
hash, and machine fingerprint. Do not compare timings from different
fingerprints unless `-AllowDifferentMachine` is explicitly supplied for an
informational run.

**Step 4: Implement the build runner**

Create a fresh exact temporary target directory, build Web assets first, time
the four approved build/test phases, measure artifacts, and delete only the
resolved temporary directory in `finally`. Refuse cleanup if the resolved path
is not below the system temporary directory.

**Step 5: Populate and verify the baseline**

Copy the approved measurements without refreshing them. Run both scripts in
validation-only mode and validate JSON against the contract test.

**Step 6: Commit**

```powershell
git add scripts/perf crates/rssh-app/tests/performance_scorecard_contract.rs .gitignore
git commit -m "test: codify runtime and build scorecards"
```

### Task 3: Add architecture-policy tests before new crates

**Files:**

- Create: `scripts/ci/check-rust-architecture.py`
- Create: `scripts/ci/tests/test_check_rust_architecture.py`
- Create: `scripts/ci/architecture-policy.json`
- Modify: `.github/workflows/ci.yml`

**Step 1: Create failing fixtures**

Cover every approved rule: handwritten file length, state field count, impl
length, function length, rustfmt skip, unbounded production channels, and
forbidden config-to-window/native dependencies. Tests must print exact file,
item, observed value, and limit.

**Step 2: Run the red tests**

```powershell
python -m unittest scripts.ci.tests.test_check_rust_architecture
```

Expected: failure because the checker is absent.

**Step 3: Implement syntax-aware-enough checks**

Use lexical masking for comments/strings and brace matching rather than raw
line regexes. Support explicit generated-file exemptions in the policy file;
do not add an exemption for `window.rs`.

**Step 4: Add migration budgets**

The initial policy records current violations as a monotonically decreasing
budget. Every extraction task lowers the relevant budget. The final task sets
all budgets to the approved target, so CI never permits a regression while the
migration is incomplete.

**Step 5: Wire CI and commit**

```powershell
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
git add scripts/ci .github/workflows/ci.yml
git commit -m "ci: enforce shrinking architecture budgets"
```

## Phase 1: New Domain Seams

### Task 4: Create `rssh-config` with immutable nested values

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/rssh-config/Cargo.toml`
- Create: `crates/rssh-config/src/lib.rs`
- Create: `crates/rssh-config/src/model.rs`
- Create: `crates/rssh-config/src/patch.rs`
- Create: `crates/rssh-config/tests/patch_merge.rs`

**Step 1: Write patch truth-table tests**

Cover inherit, set, and clear across default, user file, CLI, runtime, and
per-window layers. Include nested font, terminal, input, window, render,
domain, and lifecycle objects.

```rust
pub enum Patch<T> {
    Inherit,
    Clear,
    Set(T),
}

pub struct EffectiveConfig {
    pub font: FontConfig,
    pub terminal: TerminalConfig,
    pub input: InputConfig,
    pub window: WindowConfig,
    pub render: RenderConfig,
    pub domain: DomainConfig,
    pub lifecycle: LifecycleConfig,
}
```

**Step 2: Run red**

```powershell
cargo test --locked -p rssh-config --test patch_merge
```

Expected: package not found.

**Step 3: Implement the smallest green model**

Keep values immutable after validation. Return `Arc<EffectiveConfig>` from the
merge boundary and never expose partial invalid state.

**Step 4: Verify and commit**

```powershell
cargo test --locked -p rssh-config
cargo clippy --locked -p rssh-config --all-targets -- -D warnings
git add Cargo.toml Cargo.lock crates/rssh-config
git commit -m "feat: add immutable configuration domain"
```

### Task 5: Extract validation, diagnostics, and typed config diffs

**Files:**

- Create: `crates/rssh-config/src/diagnostic.rs`
- Create: `crates/rssh-config/src/diff.rs`
- Create: `crates/rssh-config/src/validate.rs`
- Create: `crates/rssh-config/tests/validation.rs`
- Modify: `crates/rssh-config/src/lib.rs`

**Step 1: Write failing tests**

Assert that invalid combinations preserve the last-known-good snapshot and
return path-qualified diagnostics. Assert that a font-only edit produces only
`ConfigDiff::font`, not a 261-field copy or unrelated runtime diff.

**Step 2: Implement typed diffs**

```rust
pub struct ConfigDiff {
    pub font: Option<FontConfigDiff>,
    pub terminal: Option<TerminalConfigDiff>,
    pub input: Option<InputConfigDiff>,
    pub window: Option<WindowConfigDiff>,
    pub render: Option<RenderConfigDiff>,
    pub domain: Option<DomainConfigDiff>,
    pub lifecycle: Option<LifecycleConfigDiff>,
}
```

Use equality/field-aware comparison at reload time only. Consumers receive
only their sub-diff.

**Step 3: Verify and commit**

```powershell
cargo test --locked -p rssh-config
git add crates/rssh-config
git commit -m "feat: add validated configuration diffs"
```

### Task 6: Turn built-in color schemes into generated data

**Files:**

- Create: `crates/rssh-config/assets/color-schemes/`
- Create: `crates/rssh-config/src/schemes.rs`
- Create: `crates/rssh-config/build.rs`
- Create: `crates/rssh-config/tests/scheme_equivalence.rs`
- Modify: `crates/rssh-app/src/window.rs`

**Step 1: Capture legacy equivalence**

Generate a fixture containing every legacy scheme name, canonical TOML hash,
and parsed semantic value. The test compares the new index and byte ranges to
that fixture.

**Step 2: Run red, then implement generator**

The build script sorts names, validates duplicate-free UTF-8 TOML, writes one
compact byte asset plus name/range/checksum metadata to `OUT_DIR`, and marks
all source assets with `rerun-if-changed`.

**Step 3: Remove the 41,747-line literal**

Route existing callers through `rssh_config::schemes`. Confirm lookup performs
no allocation before the selected TOML is parsed.

**Step 4: Verify structure and commit**

```powershell
cargo test --locked -p rssh-config --test scheme_equivalence
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
git add crates/rssh-config crates/rssh-app/src/window.rs scripts/ci/architecture-policy.json
git commit -m "refactor: generate built-in color scheme assets"
```

### Task 7: Move config lifecycle out of the window dependency cycle

**Files:**

- Create: `crates/rssh-config/src/lifecycle.rs`
- Create: `crates/rssh-config/src/source.rs`
- Create: `crates/rssh-config/tests/reload.rs`
- Modify: `crates/rssh-app/src/config_lifecycle.rs`
- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/Cargo.toml`

**Step 1: Port lifecycle fixtures as failing tests**

Cover missing file, valid reload, invalid reload, debounce, source precedence,
last-known-good retention, and diagnostics.

**Step 2: Implement config-owned lifecycle**

`rssh-config` returns snapshots/diffs/events and has no dependency on winit or
any `window` module. The app adapter owns file watching and translates notify
events into lifecycle inputs.

**Step 3: Delete the reverse import and verify**

```powershell
rg "crate::window|rssh_native|winit" crates/rssh-config
cargo test --locked -p rssh-config -p rssh-app config
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
```

Expected: `rg` returns no dependency violation; all ported tests pass.

**Step 4: Commit**

```powershell
git add crates/rssh-config crates/rssh-app scripts/ci/architecture-policy.json Cargo.lock
git commit -m "refactor: invert configuration lifecycle dependency"
```

### Task 8: Create the transport-neutral runtime API

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/rssh-runtime/Cargo.toml`
- Create: `crates/rssh-runtime/src/lib.rs`
- Create: `crates/rssh-runtime/src/api.rs`
- Create: `crates/rssh-runtime/src/transport.rs`
- Create: `crates/rssh-runtime/src/metrics.rs`
- Create: `crates/rssh-runtime/tests/api_contract.rs`

**Step 1: Write compile-time and behavior contracts**

Assert transport independence, monotonic pane generation/revision, explicit
backpressure, and lossless effect ordering.

```rust
pub enum SubmitResult {
    Accepted,
    Backpressured { retry_after: Duration },
    Closed,
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
```

**Step 2: Run red and implement minimal types**

```powershell
cargo test --locked -p rssh-runtime --test api_contract
```

Keep platform and winit types out of the public domain API.

**Step 3: Verify and commit**

```powershell
cargo test --locked -p rssh-runtime
cargo clippy --locked -p rssh-runtime --all-targets -- -D warnings
git add Cargo.toml Cargo.lock crates/rssh-runtime
git commit -m "feat: define transport-neutral runtime API"
```

### Task 9: Implement byte-budgeted bounded mailboxes

**Files:**

- Create: `crates/rssh-runtime/src/mailbox.rs`
- Create: `crates/rssh-runtime/tests/mailbox.rs`
- Modify: `crates/rssh-runtime/src/lib.rs`

**Step 1: Write concurrency tests with a virtual budget**

Cover item limit, byte reservation, multi-producer accounting, FIFO order,
explicit full/closed results, reservation release, and close-unblocks-waiters.
Prove the mailbox never exceeds either configured high-water mark.

**Step 2: Run red**

```powershell
cargo test --locked -p rssh-runtime --test mailbox
```

**Step 3: Implement without an unbounded escape path**

Use a small dependency only if benchmarks show it is superior to a
`Mutex<VecDeque<T>> + Condvar` implementation. Record items, bytes, blocked
duration, and high-water marks.

**Step 4: Verify and commit**

```powershell
cargo test --locked -p rssh-runtime mailbox
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
git add crates/rssh-runtime scripts/ci/architecture-policy.json Cargo.lock
git commit -m "feat: add byte-budgeted runtime mailboxes"
```

### Task 10: Extract caller-owned terminal deltas

**Files:**

- Create: `crates/rssh-runtime/src/terminal.rs`
- Create: `crates/rssh-runtime/src/delta.rs`
- Create: `crates/rssh-runtime/tests/terminal_delta.rs`
- Modify: `crates/rssh-app/src/terminal_runtime.rs`
- Modify: `crates/rssh-app/src/terminal_queries.rs`
- Modify: `crates/rssh-app/src/terminal_query_dcs.rs`
- Modify: `crates/rssh-app/src/visible_output.rs`

**Step 1: Record legacy transcripts**

For every existing parser/query/visible-output fixture, capture ordered
responses, damage, visible bytes, title/progress/user-var flags, bells,
clipboard effects, and final terminal snapshot.

**Step 2: Write failing V2 comparison tests**

```rust
let mut buffers = RuntimeBuffers::default();
let delta = runtime.feed_into(input, &mut buffers)?;
assert_eq!(delta.effects(), legacy.effects());
assert_eq!(runtime.snapshot(), legacy.snapshot());
```

**Step 3: Move the runtime and reuse buffers**

The feed call writes into caller-owned buffers and returns borrowed ranges or
small descriptors. Metadata deltas are emitted only when their sources change.
Do not clone the whole terminal or metadata set per chunk.

**Step 4: Run equivalence and benchmark**

```powershell
cargo test --locked -p rssh-runtime --test terminal_delta
./scripts/perf/runtime-scorecard.ps1 -Candidate current -Workload ansi-scroll-query
```

Required at this checkpoint: query throughput and p95 do not regress; clone
and relocation counters remain zero.

**Step 5: Commit**

```powershell
git add crates/rssh-runtime crates/rssh-app Cargo.lock
git commit -m "refactor: return reusable terminal runtime deltas"
```

## Phase 2: Runtime Engine in Shadow Mode

### Task 11: Add fake transport and virtual clock test infrastructure

**Files:**

- Create: `crates/rssh-runtime/src/clock.rs`
- Create: `crates/rssh-runtime/src/testing.rs`
- Create: `crates/rssh-runtime/tests/fake_transport.rs`
- Modify: `crates/rssh-runtime/src/transport.rs`

**Step 1: Write failing scripted-transport tests**

Script partial reads/writes, read/write errors, delayed EOF, blocked writer,
resize/control calls, and close. The virtual clock must advance without sleep.
Retain a cloneable interrupt handle outside the reader, writer, and worker-owned
control plane; prove that it wakes blocked read and write operations and that
repeated interrupt/close calls are idempotent.

**Step 2: Implement the narrow test ports**

Keep `testing` behind `cfg(any(test, feature = "test-support"))` and ensure
production code depends only on `Clock` and `SessionTransport` traits.
`SessionTransport::split` returns reader, writer, control, and an independently
owned `SessionInterrupt`; the interrupt path must not wait for the pane worker.

**Step 3: Verify and commit**

```powershell
cargo test --locked -p rssh-runtime --test fake_transport
git add crates/rssh-runtime
git commit -m "test: add deterministic runtime transports and clock"
```

### Task 12: Implement one-owner pane workers and ordered writes

**Files:**

- Create: `crates/rssh-runtime/src/hub.rs`
- Create: `crates/rssh-runtime/src/pane.rs`
- Create: `crates/rssh-runtime/src/shutdown.rs`
- Create: `crates/rssh-runtime/tests/pane_worker.rs`
- Modify: `crates/rssh-runtime/src/lib.rs`

**Step 1: Write lifecycle/order tests**

Cover open-ready, user input interleaved with terminal replies, resize during
output, restart generation, stale event rejection, close, deadline/reaper
handoff, and zero live handles after shutdown.

**Step 2: Implement the pane state machine**

One worker owns terminal progression, ordered writer, transport control,
reusable buffers, and shutdown state. A separate blocking reader may feed the
bounded reader mailbox. No production channel is unbounded.

**Step 3: Verify race-focused repetitions**

```powershell
1..100 | ForEach-Object {
  cargo test --locked -p rssh-runtime --test pane_worker --quiet
  if ($LASTEXITCODE -ne 0) { throw "iteration $_ failed" }
}
```

**Step 4: Commit**

```powershell
git add crates/rssh-runtime
git commit -m "feat: add owned pane worker lifecycle"
```

### Task 13: Add batch coalescing and one-slot presentation publication

**Files:**

- Create: `crates/rssh-runtime/src/batch.rs`
- Create: `crates/rssh-runtime/src/latest.rs`
- Create: `crates/rssh-runtime/tests/batching.rs`
- Create: `crates/rssh-runtime/tests/burst.rs`

**Step 1: Write failing batching contracts**

Assert byte/time budget boundaries, one parse publication per batch, one wake
on empty-to-ready, continuation wake when work remains, monotonic revisions,
latest-frame replacement, and lossless effects.

**Step 2: Add the 64 MiB stress test**

Feed 8 KiB reader chunks. Assert bounded queue/RSS growth, identical final
terminal state, no effect loss, and at least 16 PTY chunks per host wake.

**Step 3: Implement and instrument**

Publish snapshots through an atomic/mutex latest slot. Keep effects in a
bounded ordered queue. Record batch bytes/items, parse/snapshot duration,
replaced frames, wakes, and high-water marks.

**Step 4: Verify and commit**

```powershell
cargo test --locked -p rssh-runtime --test batching --test burst
git add crates/rssh-runtime
git commit -m "feat: batch runtime output and coalesce wakes"
```

### Task 14: Add local PTY and SSH transport adapters

**Files:**

- Create: `crates/rssh-runtime/src/transport/local.rs`
- Create: `crates/rssh-runtime/src/transport/ssh.rs`
- Create: `crates/rssh-runtime/tests/local_transport.rs`
- Create: `crates/rssh-runtime/tests/ssh_transport.rs`
- Modify: `crates/rssh-runtime/Cargo.toml`
- Modify: `crates/rssh-app/src/local.rs`
- Modify: `crates/rssh-app/src/ssh.rs`

**Step 1: Port adapter contracts as red tests**

Cover spawn/connect, partial I/O, resize, exit status, disconnect, error
context, and close ordering. Reuse existing loopback and process support.

**Step 2: Implement thin adapters**

Adapters only translate `rssh-pty` / `rssh-ssh` handles into
`SessionTransport`; terminal and controller decisions stay in runtime/native.

**Step 3: Run focused real transports**

```powershell
cargo test --locked -p rssh-runtime --test local_transport
cargo test --locked -p rssh-runtime --test ssh_transport
cargo test --locked -p rssh-app --test local_pty
cargo test --locked -p rssh-app --test openssh_loopback
```

**Step 4: Commit**

```powershell
git add crates/rssh-runtime crates/rssh-app Cargo.lock
git commit -m "feat: adapt PTY and SSH sessions to runtime v2"
```

### Task 15: Build the legacy/V2 transcript-equivalence harness

**Files:**

- Create: `crates/rssh-runtime/tests/equivalence.rs`
- Create: `crates/rssh-runtime/tests/fixtures/transcripts/`
- Create: `crates/rssh-app/src/legacy_runtime.rs`
- Modify: `crates/rssh-app/Cargo.toml`

**Step 1: Define canonical transcript serialization**

Normalize only nondeterministic identifiers/timestamps. Preserve byte order,
effect order, snapshot cells/row identities, metadata, damage, errors, and
shutdown outcome.

**Step 2: Populate representative fixtures**

Include plain/ANSI/query workloads, alternate screen, scrollback, resize,
mouse/IME input, OSC/DCS, clipboard, title/progress/user vars, multi-pane,
restart, stale generation, local exit, and SSH disconnect.

**Step 3: Run red, then close every difference**

```powershell
cargo test --locked -p rssh-runtime --test equivalence -- --nocapture
```

Expected before fixes: explicit structured differences. Expected before
cutover: zero unexplained differences.

**Step 4: Commit**

```powershell
git add crates/rssh-runtime crates/rssh-app
git commit -m "test: prove legacy and runtime v2 equivalence"
```

## Phase 3: Deterministic Native Controller

### Task 16: Create `rssh-native` intent/effect contracts

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/rssh-native/Cargo.toml`
- Create: `crates/rssh-native/src/lib.rs`
- Create: `crates/rssh-native/src/intent.rs`
- Create: `crates/rssh-native/src/effect.rs`
- Create: `crates/rssh-native/src/state.rs`
- Create: `crates/rssh-native/src/controller.rs`
- Create: `crates/rssh-native/tests/controller.rs`

**Step 1: Write pure reducer tests**

Test platform intent, parsed command, runtime batch, config diff, timer,
redraw, close, restart, stale revision, and stale generation. Assert exact
state/effect output without opening a real window.

```rust
pub fn reduce(
    state: &mut WindowState,
    intent: WindowIntent,
    effects: &mut Vec<WindowEffect>,
);
```

**Step 2: Implement the smallest reducer**

Keep `WindowState` under 64 direct fields by grouping cohesive nested states.
Effects identify typed ports for runtime, window, renderer, clipboard, URI,
notification, persistence, and spawning.

**Step 3: Verify and commit**

```powershell
cargo test --locked -p rssh-native --test controller
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
git add Cargo.toml Cargo.lock crates/rssh-native scripts/ci/architecture-policy.json
git commit -m "feat: add deterministic native window controller"
```

### Task 17: Extract presentation building from platform hosting

**Files:**

- Create: `crates/rssh-native/src/presentation.rs`
- Create: `crates/rssh-native/src/layout.rs`
- Create: `crates/rssh-native/tests/presentation.rs`
- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/src/window_gpu.rs`

**Step 1: Port snapshot/layout fixtures as red tests**

Cover tabs, panes, overlays, titlebar, scale/DPI, damage, cursor, selection,
scrollbar, search, command palette, and frame revision selection.

**Step 2: Implement immutable presentation input**

Build presentation data from controller state, config snapshot, and latest
runtime snapshots. Do not access PTY, SSH, filesystem, clipboard, or winit.

**Step 3: Verify rendering parity and commit**

```powershell
cargo test --locked -p rssh-native --test presentation
cargo test --locked -p rssh-app window_gpu
git add crates/rssh-native crates/rssh-app scripts/ci/architecture-policy.json
git commit -m "refactor: extract native presentation model"
```

### Task 18: Extract WinitHost and typed ports

**Files:**

- Create: `crates/rssh-native/src/host.rs`
- Create: `crates/rssh-native/src/ports.rs`
- Create: `crates/rssh-native/src/platform/`
- Create: `crates/rssh-native/tests/host_effects.rs`
- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/src/window_gpu.rs`

**Step 1: Write fake-port effect tests**

Assert ordering and error translation for window creation/resize/focus/close,
redraw/deadline, runtime submission/backpressure, clipboard, URI,
notification, persistence, GPU surface/device recovery, and new windows.

**Step 2: Implement host translation**

Winit user events carry pane/window tokens only. Host drains bounded runtime
batches within a per-turn time budget and schedules at most one continuation
wake. It executes effects but owns no terminal/config model.

**Step 3: Verify real-window contracts**

```powershell
cargo test --locked -p rssh-native --test host_effects
cargo test --locked -p rssh-app --test native_window_debug
cargo test --locked -p rssh-app --test native_window_e2e
```

**Step 4: Commit**

```powershell
git add crates/rssh-native crates/rssh-app Cargo.lock scripts/ci/architecture-policy.json
git commit -m "refactor: isolate winit host and native ports"
```

### Task 19: Cut over a single local pane behind a runtime selector

**Files:**

- Create: `crates/rssh-app/src/runtime_selection.rs`
- Modify: `crates/rssh-app/src/main.rs`
- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/tests/local_pty.rs`
- Modify: `crates/rssh-app/tests/native_window_e2e.rs`

**Step 1: Add selector and dual-path E2E**

Use an internal environment/test selector only; public CLI stays unchanged.
Run the exact local-pane scenario on legacy and V2 and compare transcript,
window behavior, process cleanup, and exit status.

**Step 2: Route V2 through composition root**

Construct config, transport, runtime hub, controller, host, and renderer in
`main.rs`. Do not duplicate domain logic in the selector.

**Step 3: Enforce checkpoint metrics**

Run the runtime scorecard and focused build scorecard. Require matched runtime
throughput/p95/RSS to be better than baseline and unrelated metrics no worse
before making V2 the test default.

**Step 4: Commit**

```powershell
git add crates/rssh-app
git commit -m "feat: run local panes through runtime v2"
```

### Task 20: Migrate multi-pane, restart, and SSH ownership

**Files:**

- Modify: `crates/rssh-native/src/controller.rs`
- Modify: `crates/rssh-runtime/src/hub.rs`
- Modify: `crates/rssh-app/src/window_restart_pane_tests.rs`
- Modify: `crates/rssh-app/src/window_inspect_pane_tests.rs`
- Modify: `crates/rssh-app/tests/openssh_loopback.rs`
- Modify: `crates/rssh-app/src/window.rs`

**Step 1: Port red behavior matrices**

Cover split/new/close/activate pane, fair draining, inactive metadata, restart
generation, stale events, SSH disconnect/reconnect, and window/application
shutdown.

**Step 2: Implement hub/controller ownership**

The runtime hub owns pane workers; the controller owns pane/window layout and
selection. No pane terminal/transport remains in the host.

**Step 3: Run deterministic and real transport tests**

```powershell
cargo test --locked -p rssh-runtime
cargo test --locked -p rssh-native
cargo test --locked -p rssh-app --test local_pty --test openssh_loopback
```

**Step 4: Commit**

```powershell
git add crates/rssh-runtime crates/rssh-native crates/rssh-app
git commit -m "feat: migrate pane and SSH ownership to runtime v2"
```

### Task 21: Replace flattened window config copies

**Files:**

- Modify: `crates/rssh-native/src/state.rs`
- Modify: `crates/rssh-native/src/controller.rs`
- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/src/config_lifecycle.rs`
- Modify: `crates/rssh-app/src/main.rs`

**Step 1: Add allocation/diff tests**

Assert each window stores one `Arc<EffectiveConfig>`, runtime overrides stay
small, reload shares unchanged subtrees, and consumers receive only relevant
diffs.

**Step 2: Route all access through nested config**

Delete `NativeEffectiveConfig`, `NativeConfigOverrides`, and
`NativeLuaWindowConfigOverrides` only after fixture parity proves the same
precedence and accepted/rejected syntax.

**Step 3: Verify and commit**

```powershell
cargo test --locked -p rssh-config -p rssh-native
cargo test --locked -p rssh-app config
git add crates/rssh-config crates/rssh-native crates/rssh-app
git commit -m "refactor: share immutable native configuration"
```

## Phase 4: Remove the Aggregate and Improve Build/Test Shape

### Task 22: Move remaining window domains into bounded modules

**Files:**

- Create/modify: `crates/rssh-native/src/commands/`
- Create/modify: `crates/rssh-native/src/input/`
- Create/modify: `crates/rssh-native/src/tabs/`
- Create/modify: `crates/rssh-native/src/panes/`
- Create/modify: `crates/rssh-native/src/overlays/`
- Create/modify: `crates/rssh-native/src/persistence/`
- Create/modify: `crates/rssh-native/src/accessibility/`
- Modify: `crates/rssh-app/src/window.rs`

**Step 1: Extract one cohesive behavior family at a time**

For each family: move its existing tests first, run them red against the new
module boundary, move the smallest implementation, run green, then lower the
architecture budget. Do not combine unrelated behavior in one module or impl.

**Step 2: Keep platform effects at ports**

Filesystem, shell, clipboard, URL, notifications, and native-window calls must
be emitted as typed effects and exercised with fake ports.

**Step 3: Verify after every family**

```powershell
cargo test --locked -p rssh-native
cargo test --locked -p rssh-app --lib
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
```

**Step 4: Commit each cohesive extraction**

Use messages such as:

```powershell
git commit -m "refactor: extract native input controller"
git commit -m "refactor: extract native pane controller"
```

### Task 23: Split and table-drive the 4,258-test monolith

**Files:**

- Create: `crates/rssh-config/tests/fixtures/`
- Create: `crates/rssh-runtime/tests/fixtures/`
- Create: `crates/rssh-native/tests/fixtures/`
- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/src/window_inspect_pane_tests.rs`
- Modify: `crates/rssh-app/src/window_restart_pane_tests.rs`

**Step 1: Inventory tests by behavior ID**

Create a checked test manifest mapping every legacy test/fixture to its new
crate, module, and behavior ID. The manifest must fail on missing or duplicate
IDs.

**Step 2: Convert repetitive families to tables**

Keep distinct failure diagnostics per case. Preserve coverage and semantic
assertions rather than raw function count.

**Step 3: Measure compile/link improvements**

Run the build scorecard after each large test-family move. Keep changes only
when largest harness, no-run time, and target bytes improve without reducing
coverage.

**Step 4: Commit**

```powershell
git add crates/rssh-config crates/rssh-runtime crates/rssh-native crates/rssh-app scripts/perf
git commit -m "test: partition native behavior fixtures by domain"
```

### Task 24: Delete the legacy path and `window.rs` aggregate

**Files:**

- Delete: `crates/rssh-app/src/legacy_runtime.rs`
- Delete: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/src/main.rs`
- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `scripts/ci/architecture-policy.json`

**Step 1: Prove cutover readiness**

Run the entire equivalence corpus, workspace tests, native E2E, and the full
runtime/build scorecards. Zero unexplained difference is required.

**Step 2: Remove legacy selection and aggregate**

Delete the test-only legacy compiler path, remove `#[rustfmt::skip]`, make V2
the sole production path, and reduce architecture budgets to the final hard
limits.

**Step 3: Verify the thin app root**

`main.rs` may parse/dispatch/construct/map errors but may not own pane,
terminal, window, or config field state.

**Step 4: Commit**

```powershell
git add -A crates/rssh-app scripts/ci/architecture-policy.json
git commit -m "refactor: complete runtime v2 cutover"
```

## Phase 5: Final Evidence and CI Enforcement

### Task 25: Add native latency, saturation, and shutdown probes

**Files:**

- Create: `crates/rssh-app/tests/native_runtime_performance.rs`
- Create: `scripts/perf/native-latency.ps1`
- Modify: `scripts/perf/runtime-scorecard.ps1`
- Modify: `scripts/ci/process-harness.ps1`

**Step 1: Write probe contracts**

Record input receipt, worker processing, snapshot publication, redraw request,
and present completion using one monotonic clock. Record queue depth,
backpressure, wake compression, replaced frames, and shutdown duration.

**Step 2: Add paired baseline/candidate protocol**

Alternate samples to reduce drift. Require at least 10% median p95 improvement
and below 16.67 ms where the adapter reports present timing. Unsupported
present timing must be explicit, not silently passed.

**Step 3: Extend lifecycle stress**

Run 100 PTY attempts with zero retry and zero survivor, assert all runtime
workers/mailboxes/reaper ownership are empty, and require 10% faster completion
than the approved baseline on the same fingerprint.

**Step 4: Commit**

```powershell
git add crates/rssh-app/tests scripts/perf scripts/ci/process-harness.ps1
git commit -m "test: gate native latency and runtime shutdown"
```

### Task 26: Enforce the final scorecard in CI

**Files:**

- Modify: `.github/workflows/ci.yml`
- Modify: `scripts/perf/runtime-scorecard.ps1`
- Modify: `scripts/perf/build-scorecard.ps1`
- Modify: `scripts/ci/write-evidence-manifest.py`
- Modify: `docs/production-parity-verification.md`

**Step 1: Add deterministic contract gates**

Run architecture policy, bounded 64 MiB burst, equivalence corpus, coverage,
and deterministic counters on hosted runners. Upload structured evidence.

**Step 2: Separate machine-bound trend evidence**

Hosted CI must never compare raw time/RSS values with the local fingerprint.
It validates invariants and its own pinned-runner baseline. The authoritative
local scorecard remains a required release artifact.

**Step 3: Verify workflow and scripts**

```powershell
python -m unittest discover scripts/ci/tests
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
cargo test --locked --workspace --all-targets
```

**Step 4: Commit**

```powershell
git add .github/workflows/ci.yml scripts docs/production-parity-verification.md
git commit -m "ci: enforce runtime v2 acceptance scorecard"
```

### Task 27: Run final local verification and publish evidence

**Files:**

- Create: `evidence/runtime-v2/summary.json`
- Create: `evidence/runtime-v2/runtime-scorecard.json`
- Create: `evidence/runtime-v2/build-scorecard.json`
- Create: `evidence/runtime-v2/native-latency.json`
- Create: `docs/architecture.md`
- Modify: `README.md`

**Step 1: Run formatting, lint, and all tests**

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
npm --prefix web run lint
npm --prefix web test
npm --prefix web run build
```

Expected: all pass, no new ignored regression, and deterministic Windows frame
probe passes locally without skip.

**Step 2: Run coverage**

```powershell
cargo llvm-cov --locked --workspace --lcov --output-path evidence/runtime-v2/coverage.lcov
```

Expected: existing workspace line coverage does not decrease; runtime and
controller modules are at least 90%.

**Step 3: Run all scorecards**

```powershell
./scripts/perf/runtime-scorecard.ps1 -Candidate current -Output evidence/runtime-v2/runtime-scorecard.json
./scripts/perf/build-scorecard.ps1 -Candidate current -Output evidence/runtime-v2/build-scorecard.json
./scripts/perf/native-latency.ps1 -Candidate current -Output evidence/runtime-v2/native-latency.json
```

Expected: every approved gate passes on the same machine/toolchain fingerprint.
One failure blocks completion; do not refresh the baseline.

**Step 4: Verify structural limits**

```powershell
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
rg "rustfmt::skip|mpsc::channel\(|unbounded" crates/rssh-app crates/rssh-native crates/rssh-runtime crates/rssh-config
```

Expected: checker passes; grep has no production-policy violation.

**Step 5: Document the final architecture and evidence**

Write ownership, message flow, backpressure, shutdown, config projection,
failure handling, test strategy, benchmark protocol, and exact scorecard
results. Link the structured evidence from README and architecture docs.

**Step 6: Commit**

```powershell
git add evidence/runtime-v2 docs/architecture.md README.md
git commit -m "docs: publish runtime v2 verification evidence"
```

## Completion Audit

Before claiming completion:

1. Confirm the branch contains no unrelated user changes.
2. Confirm the worktree is clean.
3. Confirm every commit's tests were run after the corresponding code.
4. Confirm all final commands above were run from the final commit.
5. Inspect GitHub Actions checks and fix any repository-caused failure.
6. Compare the final evidence with the immutable baseline by metric, not by a
   single composite score.
7. Request code review and address technically valid findings.
8. Only then prepare the branch for merge or PR.
