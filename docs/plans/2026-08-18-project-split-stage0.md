# Project Split Stage 0 Diagnostics Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a versioned cross-platform launcher and diagnostics contract that measures startup milestones and steady child-process memory for empty-window and one-SSH GUI scenarios.

**Architecture:** Add a dependency-light `rssh-diagnostics` crate whose library owns schema, marker validation, statistics, sampling ports, and platform samplers, while its `rssh-bench-launcher` binary owns process lifecycle. Extend `rssh-app` only with a versioned marker emitter and bounded benchmark scenario/hold controls; keep SSH fixture orchestration in test/launcher code and retain the existing startup gate until parity is proven.

**Tech Stack:** Rust 1.89, serde/serde_json, platform process APIs (`windows-sys`, `/proc`, macOS task info), existing winit/softbuffer/wgpu GUI, existing `rssh-test-support` loopback SSH fixture, GitHub Actions, PowerShell and POSIX shell runners.

---

## Execution rules

- Work only in `E:/project/R-SSH/.worktrees/project-split-stage0` on `codex/project-split-stage0`.
- Use @superpowers:test-driven-development for every behavior change.
- Use @superpowers:systematic-debugging for any unexpected failure.
- Use @superpowers:verification-before-completion before claiming a batch or branch complete.
- Do not rename crates, move runtime/renderer ownership, or optimize performance in this stage.
- Keep the existing `--benchmark-startup` and first-present gate green while adding v2 diagnostics.
- Commit each task independently after its focused tests pass.

### Task 1: Scaffold the diagnostics crate and freeze schema v2

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/rssh-diagnostics/Cargo.toml`
- Create: `crates/rssh-diagnostics/src/lib.rs`
- Create: `crates/rssh-diagnostics/src/schema.rs`
- Create: `crates/rssh-diagnostics/tests/schema_v2.rs`

**Step 1: Write the failing schema test**

Create `schema_v2.rs` with a complete minimal result and assert the exact wire names:

```rust
use rssh_diagnostics::{
    DiagnosticsResult, MemoryMetric, RunConfiguration, Scenario, SchemaVersion,
};

#[test]
fn schema_v2_serializes_stable_discriminators_and_optional_hybrid_milestones() {
    let result = DiagnosticsResult::successful_fixture(
        Scenario::EmptyWindow,
        MemoryMetric::WindowsPrivateWorkingSetBytes,
        RunConfiguration::default(),
    );
    let value = serde_json::to_value(result).unwrap();

    assert_eq!(value["schema"], "rssh.diagnostics/v2");
    assert_eq!(value["run"]["scenario"], "empty_window");
    assert_eq!(
        value["memory"]["metric"],
        "windows_private_working_set_bytes"
    );
    assert!(value["milestones"]["gpu_ready_ms"].is_null());
}
```

Add a second test that deserializes a checked-in literal with `first_present_ms = 10`,
`gpu_ready_ms = 40`, and verifies it is accepted.

**Step 2: Run the test to verify RED**

Run:

```powershell
cargo test -p rssh-diagnostics --test schema_v2 --locked -j1
```

Expected: FAIL because the workspace member and crate do not exist.

**Step 3: Add the workspace member and minimal schema**

Add `"crates/rssh-diagnostics"` to the root workspace and create the crate with:

```toml
[package]
name = "rssh-diagnostics"
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
version.workspace = true

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[lints]
workspace = true
```

Implement public serde types for `SchemaVersion`, `Scenario`, `Platform`,
`RunIdentity`, `RunConfiguration`, `StartupMilestones`, `Readiness`, `RendererSummary`,
`ConnectionSummary`, `MemoryMetric`, `MemorySample`, `MemoryStatistics`,
`ProcessSummary`, `DiagnosticFailure`, and `DiagnosticsResult`. Use an explicit custom
serializer for the constant schema string or a newtype that rejects any value other
than `rssh.diagnostics/v2`.

Validation must allow GPU readiness after first present and absent GPU readiness for a
CPU result. It must require positive configured durations/counts and exact sample
count for a successful result.

**Step 4: Run focused tests to verify GREEN**

Run:

```powershell
cargo test -p rssh-diagnostics --test schema_v2 --locked -j1
cargo test -p rssh-diagnostics --doc --locked -j1
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add Cargo.toml Cargo.lock crates/rssh-diagnostics
git commit -m "feat(diagnostics): add stage 0 schema v2"
```

### Task 2: Implement deterministic statistics

**Files:**

- Create: `crates/rssh-diagnostics/src/statistics.rs`
- Modify: `crates/rssh-diagnostics/src/lib.rs`
- Test: `crates/rssh-diagnostics/src/statistics.rs`

**Step 1: Write failing boundary tests**

Test empty input rejection, singleton, even/odd median, duplicate values, checked sum
overflow, and nearest-rank p95:

```rust
#[test]
fn nearest_rank_percentiles_are_stable_at_small_sample_boundaries() {
    let stats = summarize_bytes(&[10, 20, 30, 40, 50]).unwrap();
    assert_eq!(stats.p50, 30);
    assert_eq!(stats.p95, 50);
    assert_eq!(stats.median, 30);
    assert_eq!(stats.mean, 30);
}

#[test]
fn even_median_uses_checked_integer_midpoint() {
    let stats = summarize_bytes(&[10, 20, 30, 40]).unwrap();
    assert_eq!(stats.median, 25);
}
```

**Step 2: Run the tests to verify RED**

```powershell
cargo test -p rssh-diagnostics statistics::tests --locked -j1
```

Expected: FAIL with unresolved `summarize_bytes`.

**Step 3: Implement the smallest complete calculator**

Sort a copied `Vec<u64>`, calculate min/max/count, use `u128` for sum and checked
midpoints, and implement nearest rank as `ceil(percent * count) - 1`. Return a typed
`StatisticsError::EmptySamples` rather than a zero-valued summary.

**Step 4: Verify GREEN**

```powershell
cargo test -p rssh-diagnostics statistics::tests --locked -j1
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-diagnostics/src
git commit -m "feat(diagnostics): add deterministic memory statistics"
```

### Task 3: Add marker parsing and lifecycle validation

**Files:**

- Create: `crates/rssh-diagnostics/src/marker.rs`
- Modify: `crates/rssh-diagnostics/src/lib.rs`
- Create: `crates/rssh-diagnostics/tests/marker_protocol.rs`

**Step 1: Write failing parser tests**

Freeze a prefix such as `rssh_diagnostic ` and test:

```rust
#[test]
fn parser_ignores_plain_output_and_accepts_cpu_first_gpu_later() {
    let lines = [
        "ordinary diagnostic",
        r#"rssh_diagnostic {"schema":"rssh.diagnostics/v2","run_id":"r1","pid":42,"scenario":"empty_window","kind":"first_present","elapsed_ms":12,"renderer":"cpu"}"#,
        r#"rssh_diagnostic {"schema":"rssh.diagnostics/v2","run_id":"r1","pid":42,"scenario":"empty_window","kind":"gpu_ready","elapsed_ms":50,"renderer":"gpu"}"#,
    ];
    let trace = collect_markers(lines, MarkerIdentity::new("r1", 42, Scenario::EmptyWindow)).unwrap();
    assert_eq!(trace.first_present_ms, Some(12));
    assert_eq!(trace.gpu_ready_ms, Some(50));
}
```

Also test malformed prefixed JSON, mismatched run/PID/scenario, decreasing elapsed
time, duplicate first present, duplicate terminal marker, and unknown unprefixed lines.

**Step 2: Verify RED**

```powershell
cargo test -p rssh-diagnostics --test marker_protocol --locked -j1
```

Expected: FAIL because marker APIs are absent.

**Step 3: Implement marker types and validator**

Create `MarkerKind` variants for process/window/config/transport/GPU/presentation,
scenario readiness, memory sampling boundaries, and process exit. Implement a
stateful `MarkerCollector::push_line` that returns `Ignored`, `Accepted`, or typed
`MarkerError`. Preserve unknown JSON fields using `serde_json::Map` with `flatten`.

Do not impose GPU-before-first-present ordering. Require monotonic elapsed times and
single-assignment for lifecycle milestones.

**Step 4: Verify GREEN and schema interaction**

```powershell
cargo test -p rssh-diagnostics --test marker_protocol --locked -j1
cargo test -p rssh-diagnostics --locked -j1
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-diagnostics
git commit -m "feat(diagnostics): validate lifecycle marker protocol"
```

### Task 4: Model launcher options, deadlines, and structured failures

**Files:**

- Create: `crates/rssh-diagnostics/src/launcher.rs`
- Create: `crates/rssh-diagnostics/src/bin/rssh-bench-launcher.rs`
- Modify: `crates/rssh-diagnostics/src/lib.rs`
- Create: `crates/rssh-diagnostics/tests/launcher_state.rs`

**Step 1: Write failing CLI and fake-state tests**

Cover defaults, both scenarios, zero-value rejection, missing executable, unknown
arguments, early child exit, readiness timeout, exact stabilization/sample schedule,
graceful shutdown, forced bounded shutdown, and JSON-on-diagnostic-failure.

```rust
#[test]
fn defaults_encode_the_approved_sampling_contract() {
    let options = LauncherOptions::parse([
        "rssh-bench-launcher", "--app", "rssh-app", "--scenario", "empty-window", "--json"
    ]).unwrap();
    assert_eq!(options.stabilization, Duration::from_millis(5_000));
    assert_eq!(options.sample_interval, Duration::from_millis(100));
    assert_eq!(options.sample_count, 10);
}
```

Use fake `Clock`, `ChildProcess`, `LineSource`, and `MemorySampler` ports so tests do
not sleep or spawn.

**Step 2: Verify RED**

```powershell
cargo test -p rssh-diagnostics --test launcher_state --locked -j1
```

Expected: FAIL with missing launcher types.

**Step 3: Implement the orchestration state machine**

Implement states `Launch`, `AwaitMarkers`, `AwaitScenarioReady`, `Stabilize`, `Sample`,
`RequestShutdown`, `Reap`, and `EmitResult`. Each transition takes a deadline and
produces a stable failure code. Keep parsing manual and dependency-free unless a
checked-in CLI dependency is already approved.

**Step 4: Verify GREEN**

```powershell
cargo test -p rssh-diagnostics --test launcher_state --locked -j1
cargo run -p rssh-diagnostics --bin rssh-bench-launcher -- --help
```

Expected: tests pass and help lists the approved interface.

**Step 5: Commit**

```powershell
git add crates/rssh-diagnostics Cargo.lock
git commit -m "feat(diagnostics): add bounded benchmark launcher state machine"
```

### Task 5: Add Linux PSS sampling

**Files:**

- Create: `crates/rssh-diagnostics/src/sampler.rs`
- Create: `crates/rssh-diagnostics/src/sampler/linux.rs`
- Modify: `crates/rssh-diagnostics/src/lib.rs`
- Create: `crates/rssh-diagnostics/tests/linux_sampler.rs`

**Step 1: Write parser RED tests**

Test a realistic `smaps_rollup`, whitespace, missing PSS, duplicate PSS, malformed
unit/value, overflow, process disappearance, and explicit no-RSS-fallback behavior.

```rust
#[test]
fn parses_pss_kib_and_checks_byte_conversion() {
    assert_eq!(parse_smaps_rollup("Pss:       123 kB\n").unwrap(), 123 * 1024);
}
```

**Step 2: Verify RED on every platform**

```powershell
cargo test -p rssh-diagnostics --test linux_sampler --locked -j1
```

Expected: FAIL before implementation; non-Linux builds exercise the pure parser and
unsupported platform seam.

**Step 3: Implement the sampler**

Define the shared `MemorySampler` trait and `SamplerError`. On Linux read only
`/proc/<pid>/smaps_rollup`; do not use RSS fallback. Store samples as bytes with
`MemoryMetric::LinuxPssBytes`.

**Step 4: Verify GREEN**

```powershell
cargo test -p rssh-diagnostics --test linux_sampler --locked -j1
```

On Linux additionally run the ignored live-child probe with `--ignored`.

**Step 5: Commit**

```powershell
git add crates/rssh-diagnostics
git commit -m "feat(diagnostics): sample Linux process PSS"
```

### Task 6: Add Windows Private Working Set sampling

**Files:**

- Modify: `crates/rssh-diagnostics/Cargo.toml`
- Create: `crates/rssh-diagnostics/src/sampler/windows.rs`
- Create: `crates/rssh-diagnostics/tests/windows_sampler.rs`

**Step 1: Write API-seam RED tests**

Introduce an injectable native query seam returning `{ private_working_set_bytes,
process_identity }`. Test success, access denied, process missing, unsupported counter
version, PID reuse mismatch, overflow/invalid response, and verify that private bytes
and total working set are never substituted.

**Step 2: Verify RED**

```powershell
cargo test -p rssh-diagnostics --test windows_sampler --locked -j1
```

Expected: FAIL because the Windows sampler does not exist.

**Step 3: Implement the native query**

Use the minimum `windows-sys` feature set for process query and memory counters. Put
the unavoidable FFI in one small, documented Windows-only module; do not relax unsafe
policy for the rest of the crate. Open only the launched PID with query rights, read
the process creation identity, and query `PROCESS_MEMORY_COUNTERS_EX2` /
`PrivateWorkingSetSize`. Return `SamplerError::Unsupported` if the required field is
not available; never substitute `PrivateUsage`, RSS, or working set.

**Step 4: Verify GREEN and live probe**

```powershell
cargo test -p rssh-diagnostics --test windows_sampler --locked -j1
cargo test -p rssh-diagnostics --test windows_sampler live_child --locked -j1 -- --ignored
```

Expected: PASS on Windows; other platforms compile an explicit unsupported stub.

**Step 5: Commit**

```powershell
git add crates/rssh-diagnostics Cargo.lock
git commit -m "feat(diagnostics): sample Windows private working set"
```

### Task 7: Add macOS physical-footprint sampling

**Files:**

- Modify: `crates/rssh-diagnostics/Cargo.toml`
- Create: `crates/rssh-diagnostics/src/sampler/macos.rs`
- Create: `crates/rssh-diagnostics/tests/macos_sampler.rs`

**Step 1: Write API-seam RED tests**

Test physical-footprint success, permission denial, missing process, unsupported task
info flavor/version, identity mismatch, and explicit RSS non-fallback.

**Step 2: Verify RED**

```powershell
cargo test -p rssh-diagnostics --test macos_sampler --locked -j1
```

Expected: FAIL because the module is absent.

**Step 3: Implement the native query**

Use the supported macOS task/process info flavor that exposes `phys_footprint`.
Confine unavoidable FFI to one documented macOS-only module. Pair the PID with an
identity value when available and map native errors to shared typed errors. Do not
substitute resident size.

**Step 4: Verify GREEN and live probe**

```bash
cargo test -p rssh-diagnostics --test macos_sampler --locked -j1
cargo test -p rssh-diagnostics --test macos_sampler live_child --locked -j1 -- --ignored
```

Expected: PASS on macOS; unsupported stub compiles elsewhere.

**Step 5: Commit**

```bash
git add crates/rssh-diagnostics Cargo.lock
git commit -m "feat(diagnostics): sample macOS physical footprint"
```

### Task 8: Emit v2 GUI markers and support the empty-window hold scenario

**Files:**

- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `crates/rssh-app/src/cli.rs`
- Modify: `crates/rssh-app/src/main.rs`
- Modify: `crates/rssh-app/src/startup_metrics.rs`
- Modify: `crates/rssh-app/src/window.rs`
- Modify: relevant `crates/rssh-app/src/window_parts/part*.rs` selected by the existing window split
- Create: `crates/rssh-app/tests/diagnostics_marker_contract.rs`
- Modify: `crates/rssh-app/tests/native_window_debug.rs`

**Step 1: Write CLI and marker RED tests**

Add hidden/diagnostic-only application arguments carrying run ID, scenario, hold
deadline, and shutdown channel contract. Test that ordinary launches emit no v2
markers. Test exact JSON markers for window creation, first present, config ready, GPU
ready/fallback, scenario ready, and process exit.

Add an empty-window native test asserting no PTY/SSH startup metrics, a non-empty first
frame, a bounded hold, and successful launcher-requested shutdown.

**Step 2: Verify RED**

```powershell
cargo test -p rssh-app diagnostics_marker --locked -j1
cargo test -p rssh-app --test native_window_debug diagnostic_empty_window --locked -j1 -- --nocapture
```

Expected: FAIL because the diagnostic scenario and v2 emitter are absent.

**Step 3: Implement the emitter and scenario**

Depend on `rssh-diagnostics` for shared wire types. Capture marker timestamps at event
boundaries and flush immediately. Add an empty launch domain that constructs the
window/presentation without spawning local or SSH transport. Keep deferred config and
GPU behavior real. The hold must be bounded even when the launcher disappears.

**Step 4: Verify GREEN and old startup compatibility**

```powershell
cargo test -p rssh-app diagnostics_marker --locked -j1
cargo test -p rssh-app --test native_window_debug diagnostic_empty_window --locked -j1 -- --nocapture
cargo test -p rssh-app startup_trace --locked -j1
cargo test -p rssh-app --test performance_scorecard_contract --locked -j1
```

Expected: PASS; legacy `first_present` and `first_frame_memory` remain unchanged.

**Step 5: Commit**

```powershell
git add crates/rssh-app Cargo.lock
git commit -m "feat(app): expose empty-window diagnostics scenario"
```

### Task 9: Add deterministic one-SSH scenario readiness

**Files:**

- Modify: `crates/rssh-test-support/src/ssh/server.rs`
- Modify: `crates/rssh-test-support/src/ssh/agent.rs` if required for the isolated identity channel
- Modify: `crates/rssh-app/src/window_ssh_gui.rs`
- Modify: `crates/rssh-app/src/startup_metrics.rs`
- Create: `crates/rssh-app/tests/diagnostics_ssh1.rs`

**Step 1: Write the loopback RED test**

Start the existing isolated SSH fixture, launch one native SSH GUI pane, and assert:

- first present occurs;
- a visible non-secret connection state/prompt is rendered;
- `transport_started`, `transport_ready`, and `scenario_ready` markers have the same
  run/scenario/PID;
- ambient home, config, and agent are ignored;
- the fixture secret has zero matches in stdout, stderr, marker JSON, metrics, session
  log, and visible snapshots;
- shutdown closes the connection and fixture within the deadline.

**Step 2: Verify RED**

```powershell
cargo test -p rssh-app --test diagnostics_ssh1 --locked -j1 -- --nocapture
```

Expected: FAIL because scenario readiness markers and secret channel are absent.

**Step 3: Implement readiness without widening SSH product scope**

Reuse the existing native SSH GUI state machine. Emit marker events from state changes
but keep passwords/passphrases out of marker payloads. Readiness is the first rendered
eligible prompt/state after transport connection progress; do not mark ready from a
background connection event that has not reached the visible snapshot.

**Step 4: Verify GREEN and SSH regressions**

```powershell
cargo test -p rssh-app --test diagnostics_ssh1 --locked -j1 -- --nocapture
cargo test -p rssh-app window_ssh_gui::tests --locked -j1
cargo test -p rssh-ssh --all-targets --locked -j1
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-app crates/rssh-test-support Cargo.lock
git commit -m "feat(app): expose deterministic SSH diagnostics readiness"
```

### Task 10: Connect the real launcher to both scenarios

**Files:**

- Modify: `crates/rssh-diagnostics/src/launcher.rs`
- Modify: `crates/rssh-diagnostics/src/bin/rssh-bench-launcher.rs`
- Create: `crates/rssh-diagnostics/tests/launcher_e2e.rs`
- Modify: `crates/rssh-test-support/Cargo.toml` only if a reusable fixture helper is needed

**Step 1: Write process E2E RED tests**

Run a tiny marker fixture child for deterministic failure/success cases and real
`rssh-app` ignored/native cases. Assert child-only PID sampling, exact 5-second/100-ms
schedule through injected shorter test values, sample count 10, statistics, graceful
then forced shutdown, bounded output tails, and JSON emission on diagnostic failure.

**Step 2: Verify RED**

```powershell
cargo test -p rssh-diagnostics --test launcher_e2e --locked -j1
```

Expected: FAIL because production process/pipe/sampler ports are not connected.

**Step 3: Implement real orchestration**

Spawn the app with piped stdout/stderr and a process identity captured at launch.
Drain both pipes concurrently into bounded tails while parsing stdout markers. Select
the native sampler by target OS. Wait for readiness, stabilize, collect exactly the
configured samples, request shutdown, reap, validate, and print one JSON object.

For `ssh1`, own the loopback fixture lifecycle in the runner/test harness; never ask
the production GUI to start a test server.

**Step 4: Verify GREEN and native probes**

```powershell
cargo test -p rssh-diagnostics --test launcher_e2e --locked -j1
cargo test -p rssh-diagnostics --test launcher_e2e real_empty_window --locked -j1 -- --ignored --nocapture
cargo test -p rssh-diagnostics --test launcher_e2e real_ssh1 --locked -j1 -- --ignored --nocapture
```

Expected: PASS on the supported native runner.

**Step 5: Commit**

```powershell
git add crates/rssh-diagnostics crates/rssh-test-support Cargo.lock
git commit -m "feat(diagnostics): launch and sample GUI scenarios"
```

### Task 11: Add runners, CI contracts, reports, and documentation

**Files:**

- Create: `scripts/ci/run-stage0-diagnostics.ps1`
- Create: `scripts/ci/run-stage0-diagnostics.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Modify: `crates/rssh-app/tests/performance_scorecard_contract.rs`
- Modify: `README.md`
- Create: `docs/benchmarks/stage0-schema-v2.md`

**Step 1: Write static CI contract RED tests**

Assert that:

- shared PR jobs run deterministic diagnostics tests but do not apply 45/60 MiB gates;
- the protected fixed runner uses release/locked, 5 warmups, 30 cold samples, both
  scenarios, 80 x 24, and benchmark-only scale 1.0;
- raw v2 records and aggregate JSON are uploaded;
- the existing first-present p95 <= 500 ms gate remains blocking;
- steady targets 45 MiB/60 MiB are report-only;
- Windows/Linux/macOS metric names are documented exactly.

**Step 2: Verify RED**

```powershell
cargo test -p rssh-app --test performance_scorecard_contract stage0 --locked -j1
```

Expected: FAIL because runners/workflow/docs are absent.

**Step 3: Implement runners and workflow wiring**

Make scripts use `rssh-bench-launcher`, never duplicate sampling logic. Warmups produce
no retained result; each measured run is a new child process. Aggregate within each
scenario/platform and retain individual records. A sampler or schema failure fails the
job; exceeding 45/60 MiB only annotates the report.

Document local invocation, metric semantics, schema versioning, unsupported behavior,
artifact locations, and how thresholds graduate from report-only to blocking.

**Step 4: Verify GREEN**

```powershell
cargo test -p rssh-app --test performance_scorecard_contract --locked -j1
cargo test -p rssh-diagnostics --all-targets --locked -j1
pwsh -NoProfile -Command "[void][ScriptBlock]::Create((Get-Content -Raw scripts/ci/run-stage0-diagnostics.ps1))"
bash -n scripts/ci/run-stage0-diagnostics.sh
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add .github scripts README.md docs/benchmarks crates/rssh-app/tests/performance_scorecard_contract.rs
git commit -m "ci: collect stage 0 diagnostics on fixed runners"
```

### Task 12: Final verification and migration evidence

**Files:**

- Modify: `docs/plans/2026-08-18-project-split-stage0.md` only if actual command or
  platform constraints need an evidence note
- Create: `docs/benchmarks/stage0-baseline/README.md`

**Step 1: Run formatting and lint**

```powershell
cargo fmt --all -- --check
cargo clippy -p rssh-diagnostics --all-targets --locked -j1 -- -D warnings
cargo clippy -p rssh-app --all-targets --locked -j1 -- -D warnings
```

Expected: PASS.

**Step 2: Run component regressions**

```powershell
cargo test -p rssh-diagnostics --all-targets --locked -j1
cargo test -p rssh-core --all-targets --locked -j1
cargo test -p rssh-runtime --all-targets --locked -j1
cargo test -p rssh-ssh --all-targets --locked -j1
cargo test -p rssh-app --all-targets --locked -j1
```

Expected: PASS.

**Step 3: Run the full locked workspace suite**

Build `web/dist` first in a fresh worktree, then run:

```powershell
npm --prefix web ci
npm --prefix web run build
cargo test --workspace --all-targets --locked -j1
```

Expected: PASS.

**Step 4: Run native Stage 0 probes outside the sandbox/fixed environment**

```powershell
cargo build -p rssh-app -p rssh-diagnostics --release --locked -j1
pwsh -File scripts/ci/run-stage0-diagnostics.ps1 -Profile release -Warmups 5 -Samples 30
```

Expected: both scenarios produce v2 raw/aggregate artifacts, existing startup gate is
green, and 45/60 MiB values are recorded as report-only observations.

**Step 5: Record evidence and perform final diff review**

Document commands, runner identity, metric semantics, results, and any unsupported
platform reason in `docs/benchmarks/stage0-baseline/README.md`. Then run:

```powershell
git diff --check
git status --short
git log --oneline origin/main..HEAD
```

Expected: no whitespace errors, only intentional files, and one focused commit per
task.

**Step 6: Finish the branch**

Use @superpowers:requesting-code-review, address findings with
@superpowers:receiving-code-review, rerun affected verification, then use
@superpowers:finishing-a-development-branch to offer push/PR/merge choices.
