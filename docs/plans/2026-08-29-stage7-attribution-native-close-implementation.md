# Stage 7 Attribution Native-Close Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Prevent intermittent Gate 0 `auto/full-frame` readiness stalls by applying the existing Windows NVIDIA Vulkan native-close policy to the private Stage 7 attribution GPU owner.

**Architecture:** Add a small generic teardown helper that either releases Stage 7 renderer/device/context owners in dependency order or intentionally retains them until process exit for the known unsafe driver combination. Invoke it through the Stage 7 runtime after every controller outcome, before propagating controller or hold errors. Keep readiness deadlines, backend selection, sampling, and evidence validation unchanged.

**Tech Stack:** Rust, winit, wgpu, Cargo tests, PowerShell Gate 0 harness.

---

### Task 1: Add Failing Native-Close Contract Tests

**Files:**
- Modify: `crates/rssh-app/src/window_gpu.rs`
- Modify: `crates/rssh-app/src/window_compat_tests/part02_tests.rs`

**Step 1: Write the failing resource-lifetime test**

Add a unit test with tracked `Drop` values that calls the wished-for helper:

```rust
#[test]
fn stage7_native_close_retains_only_windows_nvidia_vulkan_gpu_owners() {
    // Matching resources are taken without Drop; an unmatched renderer is
    // dropped before its context/device owner.
    let retained = finalize_stage7_native_gpu_owners(
        "windows",
        "Vulkan",
        0x10de,
        &mut renderer,
        &mut device,
        &mut context,
    );
    assert!(retained);
}
```

Cover both the matching and non-matching branches and assert exact release
order for the latter.

**Step 2: Write the failing native-path integration test**

In the existing `exact_gpu_stop_stage` compatibility test module, inspect the
`run_stage7_native_attribution` source and assert that it:

```rust
assert!(native_owner.contains("runtime.shutdown_after_native_window_close()"));
assert!(
    native_owner.find("runtime.shutdown_after_native_window_close()")
        < native_owner.find("let report = report_result?")
);
```

This proves teardown occurs before controller error propagation.

**Step 3: Run the focused tests to verify RED**

Run:

```powershell
cargo test --locked -p rssh-app stage7_native_close -j1
```

Expected: FAIL because `finalize_stage7_native_gpu_owners` and the runtime
teardown call do not exist.

**Step 4: Commit the RED tests**

```powershell
git add crates/rssh-app/src/window_gpu.rs crates/rssh-app/src/window_compat_tests/part02_tests.rs
git commit -m "test(gpu): require Stage 7 native close policy"
```

### Task 2: Implement Stage 7 Native-Close Teardown

**Files:**
- Modify: `crates/rssh-app/src/window_gpu.rs`

**Step 1: Add the minimal generic teardown helper**

Implement:

```rust
fn finalize_stage7_native_gpu_owners<Renderer, Device, Context>(
    os: &str,
    backend: &str,
    vendor_id: u32,
    renderer: &mut Option<Renderer>,
    device: &mut Option<Device>,
    context: &mut Option<Context>,
) -> bool {
    let abandon =
        should_abandon_current_adapter_after_native_close(os, backend, vendor_id);
    if abandon {
        if let Some(renderer) = renderer.take() {
            std::mem::forget(renderer);
        }
        if let Some(context) = context.take() {
            std::mem::forget(context);
        }
        if let Some(device) = device.take() {
            std::mem::forget(device);
        }
    } else {
        drop(renderer.take());
        drop(context.take());
        drop(device.take());
    }
    abandon
}
```

Renderer must be handled before the context-bearing owner.

**Step 2: Add the runtime method**

Add `Stage7WindowAttributionRuntime::shutdown_after_native_window_close`, using
`gpu_identity` to supply backend and vendor identity. If identity is absent, use
ordinary ordered release.

**Step 3: Call teardown on success and failure**

Capture the controller result and hold error, invoke teardown, then propagate
the captured result:

```rust
let report_result = AttributionStageController::new(self.stop_stage)
    .run(&mut runtime)
    .map_err(|error| io::Error::other(error.to_string()));
let hold_error = runtime.take_diagnostic_hold_error();
runtime.shutdown_after_native_window_close();
let report = report_result?;
```

Keep the existing held-stage and hold-error checks after teardown.

**Step 4: Run focused tests to verify GREEN**

Run:

```powershell
cargo test --locked -p rssh-app stage7_native_close -j1
```

Expected: all focused tests PASS.

**Step 5: Commit the implementation**

```powershell
git add crates/rssh-app/src/window_gpu.rs
git commit -m "fix(gpu): protect Stage 7 native Vulkan teardown"
```

### Task 3: Run Static and Regression Verification

**Files:**
- Verify only.

**Step 1: Run Stage 7 attribution behavior tests**

```powershell
cargo test --locked -p rssh-app --test gpu_backend_memory_matrix_behavior stage7_attribution -j1
cargo test --locked -p rssh-app exact_gpu_stop_stage -j1
```

Expected: PASS.

**Step 2: Run formatting and clippy**

```powershell
cargo fmt --all -- --check
cargo clippy --locked -p rssh-app --all-targets --features production-gui,diagnostic-tools -- -D warnings
git diff --check
```

Expected: all commands exit 0 with no warnings or whitespace errors.

### Task 4: Rebuild and Run Targeted Hardware Reproduction

**Files:**
- Output only under `L:\rssh-targets`.

**Step 1: Rebuild immutable release binaries**

Use the external target directory and build `rssh-app` plus
`rssh-bench-launcher` from the current clean source commit.

**Step 2: Run 30 process-cold `auto/full-frame` diagnostics**

Use the exact Gate settings: 80x24, 100% scale, 5-second stabilization, ten
100ms samples, and no retries.

Expected: 30/30 exit 0, readiness is `ready`, final renderer is GPU, and the
actual backend remains recorded rather than forced.

### Task 5: Re-run Gate 0 Evidence Collection

**Files:**
- Archive failed evidence under `L:\rssh-evidence\failed`.
- Generate new evidence under `L:\rssh-evidence\stage7-gate0`.

**Step 1: Archive the failed evidence recoverably**

Resolve and verify the exact old evidence directory before moving it. Do not
delete it.

**Step 2: Run the complete fixed-runner pipeline**

Run prebuild, font proof, attribution matrix, deterministic attribution tests,
external-source proof, four-fragment assembly, and the Stage 7 validator.

**Step 3: Verify the exact terminal state**

Expected validator output:

```text
attribution-ready
```

Do not begin Task 10 or claim Gate 0 success without that exact result.
