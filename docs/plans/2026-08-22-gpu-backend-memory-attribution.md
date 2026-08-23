# GPU Backend Memory Attribution Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a diagnostics-only CPU/DX12/Vulkan/GL memory A/B path, record the actual selected adapter in evidence, and use the result to decide the next Stage 7 remediation without changing production renderer defaults.

**Architecture:** Shared diagnostics enums describe the requested renderer and backend. The benchmark launcher forwards them to the private `diagnostic-gui` command, which stores an explicit per-app override and restricts only that window's `GpuContextOptions`; normal commands continue using the existing native backend set. GPU-ready markers expose actual WGPU adapter metrics, and a PowerShell matrix runner stores one self-describing artifact per probe.

**Tech Stack:** Rust 2024, WGPU 30, Winit 0.30, Serde JSON, PowerShell 7, Cargo workspace tests.

---

### Task 1: Shared diagnostic renderer/backend contract

**Files:**
- Modify: `crates/rssh-diagnostics/src/schema.rs:73-129`
- Modify: `crates/rssh-diagnostics/src/lib.rs`
- Test: `crates/rssh-diagnostics/tests/schema_v2.rs`
- Test: `crates/rssh-diagnostics/tests/marker_protocol.rs`

**Step 1: Write the failing schema tests**

Add tests that require:

```rust
assert_eq!(DiagnosticRendererMode::Auto.as_str(), "auto");
assert_eq!("dx12".parse::<DiagnosticGpuBackend>().unwrap(), DiagnosticGpuBackend::Dx12);
assert!("metal".parse::<DiagnosticGpuBackend>().is_err());
```

Add a backwards-compatibility test that deserializes a pre-change v2 result
without requested/actual backend fields, plus a selected-backend fixture that
serializes:

```json
{
  "configuration": {
    "requested_renderer": "auto",
    "requested_gpu_backend": "dx12"
  },
  "renderer": {
    "backend": "dx12",
    "adapter_name": "fixture-adapter"
  }
}
```

Optional fields must use `#[serde(default, skip_serializing_if = ...)]` so the
legacy default fixture remains wire-compatible.

**Step 2: Run tests to verify RED**

Run:

```powershell
$env:CARGO_TARGET_DIR='L:\rssh-targets\stage7-release-certification'
cargo test --locked -p rssh-diagnostics --test schema_v2 --test marker_protocol -j1
```

Expected: compile failure because `DiagnosticRendererMode`,
`DiagnosticGpuBackend`, and actual renderer identity fields do not exist.

**Step 3: Implement the minimal shared types**

Add snake-case Serde enums with `Display`/`FromStr` and these values:

```rust
pub enum DiagnosticRendererMode { Auto, Cpu, Gpu }
pub enum DiagnosticGpuBackend { Dx12, Vulkan, Gl }
```

Extend `RunConfiguration` with `requested_renderer` and optional
`requested_gpu_backend`. Extend `RendererSummary` with optional `backend`,
`adapter_name`, `adapter_vendor_id`, `adapter_device_id`, and `adapter_type`.
Defaults must preserve existing fixture construction and JSON decoding.

**Step 4: Run tests to verify GREEN**

Run the same focused command. Expected: all schema and marker protocol tests
pass with no warnings.

**Step 5: Commit**

```powershell
git add crates/rssh-diagnostics/src/schema.rs crates/rssh-diagnostics/src/lib.rs crates/rssh-diagnostics/tests/schema_v2.rs crates/rssh-diagnostics/tests/marker_protocol.rs
git commit -m "feat(diagnostics): model GPU backend probes"
```

### Task 2: Launcher renderer/backend forwarding

**Files:**
- Modify: `crates/rssh-diagnostics/src/launcher.rs:7-118`
- Modify: `crates/rssh-diagnostics/src/production.rs:566-609`
- Test: `crates/rssh-diagnostics/tests/launcher_state.rs`
- Test: `crates/rssh-diagnostics/tests/launcher_e2e.rs`

**Step 1: Write the failing launcher tests**

Require `LauncherOptions` to default to auto/no backend, parse all valid modes,
reject invalid names, and reject CPU plus a GPU backend:

```rust
let mut args = base_args("empty-window");
args.extend(["--renderer", "auto", "--gpu-backend", "dx12"].map(str::to_owned));
let options = LauncherOptions::parse(args).unwrap();
assert_eq!(options.renderer, DiagnosticRendererMode::Auto);
assert_eq!(options.gpu_backend, Some(DiagnosticGpuBackend::Dx12));
```

Add a command-construction test proving that the child receives exactly:

```text
diagnostic-gui ... --renderer auto --gpu-backend dx12
```

**Step 2: Run tests to verify RED**

```powershell
cargo test --locked -p rssh-diagnostics --test launcher_state --test launcher_e2e -j1
```

Expected: missing option fields/parser branches and missing child forwarding.

**Step 3: Implement minimal parsing and forwarding**

Add `renderer` and `gpu_backend` to `LauncherOptions`, update `LAUNCHER_USAGE`,
validate combinations after parsing, populate the requested fields in
`RunConfiguration`, and append `--gpu-backend` only when selected. Do not use a
process-global environment override.

**Step 4: Run tests to verify GREEN**

Run the same launcher tests, then:

```powershell
cargo test --locked -p rssh-diagnostics --all-targets -j1
```

Expected: all diagnostics tests pass.

**Step 5: Commit**

```powershell
git add crates/rssh-diagnostics/src/launcher.rs crates/rssh-diagnostics/src/production.rs crates/rssh-diagnostics/tests/launcher_state.rs crates/rssh-diagnostics/tests/launcher_e2e.rs
git commit -m "feat(diagnostics): forward renderer backend probes"
```

### Task 3: Diagnostics-only WGPU backend restriction

**Files:**
- Modify: `crates/rssh-app/src/cli.rs:31-50,510-602`
- Modify: `crates/rssh-app/src/window.rs:804-879`
- Modify: `crates/rssh-app/src/window_gpu.rs:150-210`
- Modify: `crates/rssh-app/src/window_parts/part07.rs:1440-1470`
- Modify: `crates/rssh-app/src/window_parts/part08.rs:740-765,7415-7488`
- Test: `crates/rssh-app/src/cli.rs:5028-5100`
- Test: `crates/rssh-app/src/window_gpu.rs`
- Test: `crates/rssh-app/src/window_compat_tests/part02_tests.rs`

**Step 1: Write the failing app CLI tests**

Add a test parsing `diagnostic-gui --renderer auto --gpu-backend dx12`, a test
rejecting `--renderer cpu --gpu-backend vulkan`, and a test rejecting unsupported
backend names. Assert that normal `window` and `ssh --gui` syntax is unchanged.

**Step 2: Run the CLI test and verify RED**

```powershell
cargo test --locked -p rssh-app --bin rssh-app diagnostic_gpu_backend -j1
```

Expected: `DiagnosticGuiOptions` has no backend field and the parser rejects the
new option.

**Step 3: Write the failing WGPU option test**

Add a hardware-free helper contract:

```rust
let options = diagnostic_gpu_context_options(false, false, Some(DiagnosticGpuBackend::Dx12)).unwrap();
assert_eq!(options.backends, wgpu::Backends::DX12);
assert_eq!(diagnostic_gpu_context_options(false, false, None).unwrap().backends, native_default);
```

Run the same focused test and confirm the helper is missing.

**Step 4: Implement the minimal explicit data flow**

- Add `gpu_backend: Option<DiagnosticGpuBackend>` to `DiagnosticGuiOptions`.
- Store the value in a diagnostics-only field on `NativeWindowApp`.
- Add a setter used only by `run_diagnostic_gui`.
- Build the existing `GpuContextOptions`, then call
  `with_only_backend_name(backend.as_str())` when the field is present.
- Pass the resulting option into `WindowGpu::prepare` during deferred GPU init.
- Leave ordinary constructors and all production command paths at `None`.

**Step 5: Run tests to verify GREEN**

```powershell
cargo test --locked -p rssh-app --bin rssh-app diagnostic_gpu_backend -j1
cargo test --locked -p rssh-app --bin rssh-app deferred_gpu_ -j1
```

Expected: focused CLI/option/deferred state tests pass.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/cli.rs crates/rssh-app/src/window.rs crates/rssh-app/src/window_gpu.rs crates/rssh-app/src/window_parts/part07.rs crates/rssh-app/src/window_parts/part08.rs crates/rssh-app/src/window_compat_tests/part02_tests.rs
git commit -m "feat(app): restrict diagnostic GPU backend"
```

### Task 4: Record the actual WGPU backend and adapter

**Files:**
- Modify: `crates/rssh-app/src/window_parts/diagnostics.rs:180-218`
- Modify: `crates/rssh-diagnostics/src/marker.rs:48-245`
- Modify: `crates/rssh-diagnostics/src/production.rs:450-552`
- Test: `crates/rssh-diagnostics/tests/marker_protocol.rs`
- Test: `crates/rssh-app/src/window_compat_tests/part02_tests.rs`

**Step 1: Write the failing marker tests**

Push a `gpu_ready` marker whose `extra` map contains the five actual adapter
fields and require `MarkerCollector::trace()` to expose them. Add an app test
that supplies fixture `GpuPresentationMetrics` and verifies the emitted marker
contains no path/environment/secret fields.

**Step 2: Run tests to verify RED**

```powershell
cargo test --locked -p rssh-diagnostics --test marker_protocol -j1
cargo test --locked -p rssh-app --bin rssh-app diagnostic_gpu_ready -j1
```

Expected: collected trace and renderer summary lack adapter identity.

**Step 3: Implement minimal marker propagation**

When the first GPU frame is ready, read `WindowGpu::metrics()` and emit:

```text
gpu_backend, gpu_adapter_name, gpu_adapter_vendor_id,
gpu_adapter_device_id, gpu_adapter_type
```

Parse only correctly typed fields from `MarkerRecord.extra`; malformed optional
identity does not invalidate timing markers. Copy the collected values into the
final `RendererSummary` on success and failure result construction.

**Step 4: Run tests to verify GREEN**

Run both focused tests and `cargo test --locked -p rssh-diagnostics --all-targets
-j1`. Expected: all pass and legacy markers still decode.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window_parts/diagnostics.rs crates/rssh-diagnostics/src/marker.rs crates/rssh-diagnostics/src/production.rs crates/rssh-diagnostics/tests/marker_protocol.rs crates/rssh-app/src/window_compat_tests/part02_tests.rs
git commit -m "feat(diagnostics): report selected GPU adapter"
```

### Task 5: Windows backend memory matrix runner

**Files:**
- Create: `scripts/ci/run-gpu-backend-memory-matrix.ps1`
- Modify: `crates/rssh-app/tests/performance_scorecard_contract.rs`
- Modify: `docs/release-console.md`

**Step 1: Write the failing static contract test**

Require the script to run these four named probes with release/locked builds,
80x24 geometry, 100% benchmark DPI, 5 warmups, 30 measured runs, and isolated
JSON output:

```text
cpu
dx12
vulkan
gl
```

Require CPU to omit `--gpu-backend`; require all GPU probes to name the backend;
require aggregate output to retain failures rather than substituting another
backend.

**Step 2: Run the contract and verify RED**

```powershell
cargo test --locked -p rssh-app --test performance_scorecard_contract gpu_backend_memory_matrix -j1
```

Expected: the script does not exist.

**Step 3: Implement the runner**

Create a bounded PowerShell runner around `rssh-bench-launcher`. Reuse the Stage
0 percentile calculation and memory metric, but keep the matrix report-only.
Validate that GPU probe JSON reports the requested actual backend and that CPU
reports final renderer `cpu`. Write raw and aggregate JSON beneath the requested
output directory.

**Step 4: Run static and syntax checks**

```powershell
cargo test --locked -p rssh-app --test performance_scorecard_contract gpu_backend_memory_matrix -j1
[System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path scripts/ci/run-gpu-backend-memory-matrix.ps1), [ref]$null, [ref]$null) | Out-Null
```

Expected: contract passes and PowerShell reports no parse errors.

**Step 5: Commit**

```powershell
git add scripts/ci/run-gpu-backend-memory-matrix.ps1 crates/rssh-app/tests/performance_scorecard_contract.rs docs/release-console.md
git commit -m "test(perf): add GPU backend memory matrix"
```

### Task 6: Run the matrix and make the Stage 7 decision

**Files:**
- Create after measurement: `docs/plans/2026-08-23-stage7-gpu-memory-evidence.md`

**Step 1: Build release artifacts on L:**

```powershell
$env:CARGO_TARGET_DIR='L:\rssh-targets\stage7-release-certification'
cargo build --locked --release -p rssh-app -p rssh-diagnostics --bin rssh-app --bin rssh-bench-launcher
```

**Step 2: Run one smoke sample per probe**

```powershell
pwsh -File scripts/ci/run-gpu-backend-memory-matrix.ps1 -Profile release -Warmups 0 -Samples 1 -OutputDirectory L:\rssh-evidence\gpu-backend-smoke -SkipBuild
```

Expected: CPU succeeds; supported hardware backends either produce a validated
artifact or an explicit initialization failure.

**Step 3: Run the fixed matrix**

```powershell
pwsh -File scripts/ci/run-gpu-backend-memory-matrix.ps1 -Profile release -Warmups 5 -Samples 30 -OutputDirectory L:\rssh-evidence\gpu-backend-fixed -SkipBuild
```

Expected: aggregate p50/p95/max memory for every successful probe and no backend
identity mismatch.

**Step 4: Record the evidence-backed decision**

Write the hardware/driver identity, requested/actual backend, sample counts,
memory p50/p95/max, startup milestones, failures, and one of:

- candidate backend meets the 45 MiB target: design a separate production
  selection change and recovery matrix;
- no backend meets the target: keep Stage 7 NO-GO and attribute allocations
  inside the lowest-memory backend;
- fixed-runner evidence unavailable: keep Stage 7 NO-GO and publish the missing
  evidence as the blocker.

Do not physically split repositories in this task.

**Step 5: Commit the evidence decision**

```powershell
git add docs/plans/2026-08-23-stage7-gpu-memory-evidence.md
git commit -m "docs: record Stage 7 GPU memory evidence"
```

### Task 7: Final regression verification

**Files:**
- No new files unless a failing regression requires a TDD fix.

**Step 1: Run focused renderer and native E2E tests**

```powershell
cargo test --locked -p rssh-app --test native_window_debug ssh_gui_auto_presents_cpu_then_gpu_on_the_same_window -j1 -- --nocapture
cargo test --locked -p rssh-app --test native_window_debug ssh_gui_deferred_gpu_init_failure_presents_a_second_cpu_frame -j1 -- --nocapture
cargo test --locked -p rssh-app --test native_window_debug static_native_window_reaches_ten_frames_without_external_damage -j1 -- --nocapture
```

**Step 2: Run package and workspace gates**

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --locked --workspace --all-targets
python scripts/ci/check-rterm-release-contract.py --contract scripts/ci/rterm-release-contract.json
cargo check --locked --manifest-path contracts/rterm-consumer/Cargo.toml
```

Expected: every command exits zero with no warnings.

**Step 3: Verify repository state**

```powershell
git diff --check
git status --short --branch
```

Expected: no uncommitted changes; branch contains only the planned commits.
