# Stage 7 Product GUI Probe Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Stage 7 residence measurements run the exact packaged `production-gui` executable through the normal SSH GUI entry point.

**Architecture:** `rssh-bench-launcher --product-gui` starts normal `ssh --gui` arguments and supplies a closed, versioned probe descriptor through `RSSH_STAGE7_PRODUCT_GUI_PROBE`. `run_ssh_gui` validates the descriptor and installs the existing bounded diagnostic marker/hold controller around the normal product pane, while font and attribution probes continue to use the separate diagnostic executable.

**Tech Stack:** Rust, PowerShell, Bash, Cargo feature sets, `rssh-diagnostics/v2` markers, Python contract tests.

---

### Task 1: Define and validate launcher product mode

**Files:**
- Modify: `crates/rssh-diagnostics/src/launcher.rs`
- Modify: `crates/rssh-diagnostics/src/production.rs`

**Step 1: Write the failing parser tests**

Add tests named `parses_product_gui_mode` and
`product_gui_rejects_diagnostic_overrides`. The successful case must parse:

```rust
[
    "rssh-bench-launcher",
    "--app", executable,
    "--scenario", "empty-window",
    "--renderer", "auto",
    "--product-gui",
]
```

and assert `options.product_gui`. Reject product mode with `--renderer cpu`,
`--gpu-backend`, font proof options, or `--attribution-stage`.

**Step 2: Run the focused tests and observe RED**

Run:

```powershell
$env:CARGO_TARGET_DIR='L:\rssh-targets\stage7-split-readiness'
cargo test --locked -p rssh-diagnostics launcher::tests::product_gui -j1
```

Expected: FAIL because `--product-gui` and `LauncherOptions::product_gui` do
not exist.

**Step 3: Implement the minimal parser contract**

Add a `bool product_gui` field, parse the one-shot `--product-gui` flag, reject
duplicates, and validate:

```rust
if product_gui
    && (renderer != DiagnosticRendererMode::Auto
        || gpu_backend.is_some()
        || font_mode.is_some()
        || attribution_stage.is_some())
{
    return Err(LauncherCliError::ProductGuiDiagnosticOverride);
}
```

Keep `RunConfiguration` unchanged so the public diagnostics JSON schema does
not gain a field.

**Step 4: Write product command-construction tests**

Add tests named `product_gui_empty_window_uses_normal_ssh_entrypoint` and
`product_gui_ssh1_binds_the_loopback_fixture`. Assert that product arguments
start with `ssh --gui`, include `--renderer auto`, dimensions, password auth,
and accept-unknown-host-key, and never contain `diagnostic-gui`. Assert the
child environment contains a descriptor but no secret inside that descriptor.

**Step 5: Implement product command construction**

When `product_gui` is true, construct normal SSH GUI arguments. Use the SSH
fixture address for SSH1 and a non-routable local placeholder for empty-window;
transport suppression happens inside the validated probe. Set:

```text
RSSH_STAGE7_PRODUCT_GUI_PROBE={"schema":"rssh.stage7/product-gui-probe/v1","run_id":"...","scenario":"...","hold_ms":...}
```

Continue to pass the SSH fixture password only through
`RSSH_DIAGNOSTIC_SSH_SECRET`.

**Step 6: Run the crate tests**

Run:

```powershell
cargo test --locked -p rssh-diagnostics --all-targets -j1
```

Expected: PASS.

### Task 2: Attach the validated probe to normal `run_ssh_gui`

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/src/window_parts/diagnostics.rs`
- Test: `crates/rssh-app/src/window_compat_tests/part02_tests.rs`

**Step 1: Write failing descriptor tests**

Add tests named `product_gui_probe_accepts_the_closed_v1_descriptor` and
`product_gui_probe_rejects_unknown_or_secret_fields`. Require exactly
`schema`, `run_id`, `scenario`, and `hold_ms`; require a stable ASCII run ID,
`empty-window|ssh1`, and a hold duration in `5_000..=300_000` milliseconds.

**Step 2: Observe RED**

Run:

```powershell
cargo test --locked -p rssh-app product_gui_probe -j1
```

Expected: FAIL because the descriptor parser does not exist.

**Step 3: Implement fail-closed parsing**

Deserialize to `serde_json::Value`, require the exact field set, and return a
typed internal value. Never accept or retain password, passphrase, path, host,
or runner fields.

**Step 4: Wire the controller into `run_ssh_gui`**

If `RSSH_STAGE7_PRODUCT_GUI_PROBE` is absent, preserve normal behavior exactly.
If present:

```rust
let markers = DiagnosticMarkerHandle::new(probe.run_id, probe.scenario, process_started_at);
markers.emit(DiagnosticMarkerKind::ProcessStarted, None, None)?;
app.set_diagnostic_gui(
    markers.clone(),
    probe.scenario,
    Duration::from_millis(probe.hold_ms),
    probe.ssh_secret_from_environment()?,
    None,
    None,
);
spawn_diagnostic_stdin_shutdown_listener(event_proxy.clone())?;
```

After the event loop, shut down runtime owners, reap retired apps, and emit one
`ProcessExited` marker. Empty-window uses the existing transport-suppression
branch; SSH1 uses the normal native SSH pane and masked prompt injection.

**Step 5: Verify application behavior**

Run:

```powershell
cargo test --locked -p rssh-app product_gui_probe -j1
cargo test --locked -p rssh-app diagnostic_gui -j1
cargo test --locked -p rssh-app --test stage5_startup_contract -j1
```

Expected: PASS with no public CLI or metrics-schema change.

### Task 3: Require product mode in Stage 7 residence sub-runs

**Files:**
- Modify: `scripts/ci/run-stage0-diagnostics.ps1`
- Modify: `scripts/ci/run-stage7-product-gates.ps1`
- Modify: `scripts/ci/run-stage7-product-gates.sh`
- Modify: `scripts/ci/tests/test_stage7_product_runners.py`
- Modify: `crates/rssh-app/tests/performance_scorecard_contract.rs`

**Step 1: Write failing contract tests**

Require Stage 0 to accept `-ProductGui` and append `--product-gui` to the
launcher invocation. Require every non-startup item in both product
coordinators to select this mode, while startup remains the direct
`--benchmark-startup` path. Require static evidence that diagnostic font and
matrix runs do not select product mode.

**Step 2: Observe RED**

Run:

```powershell
python -m unittest scripts.ci.tests.test_stage7_product_runners -v
cargo test --locked -p rssh-app --test performance_scorecard_contract stage7_product_runner -j1
```

Expected: FAIL on the missing product-mode routing.

**Step 3: Implement routing**

Add the Stage 0 switch and pass it only from product residence collection.
On Unix, pass `--product-gui` directly to the native launcher. Do not modify
font/stage proof invocations. Retain `-SkipBuild`, explicit paths, raw record
shape, and all aggregation logic.

**Step 4: Run focused verification**

Run:

```powershell
python -m unittest scripts.ci.tests.test_stage7_product_runners -v
cargo test --locked -p rssh-app --test performance_scorecard_contract -j1
pwsh -NoProfile -File scripts/ci/run-stage7-product-gates.ps1 -Contract scripts/ci/stage7-split-contract.json -CandidateRef 0000000000000000000000000000000000000000 -OutputDirectory L:\rssh-evidence\stage7-product-dry-run -WhatIf
```

Expected: PASS; dry-run has no filesystem side effects.

### Task 4: Verify and commit the product defect separately

**Files:**
- Modify: all files from Tasks 1-3 that are product-probe code or tests

**Step 1: Run deterministic checks**

```powershell
cargo fmt --all -- --check
cargo clippy -p rssh-diagnostics -p rssh-app --all-targets --locked -- -D warnings
cargo test --locked -p rssh-diagnostics --all-targets -j1
cargo test --locked -p rssh-app product_gui_probe -j1
cargo test --locked -p rssh-app --test performance_scorecard_contract -j1
python -m unittest scripts.ci.tests.test_stage7_product_runners -v
```

Expected: PASS and no repository-local `target` directory.

**Step 2: Commit only the product-probe fix**

Stage the Rust product/launcher changes and their focused contract changes,
then commit:

```powershell
git commit -m "fix(diagnostics): probe the packaged product GUI"
```

Leave the remaining Task 12 coordinator/workflow changes unstaged for the
separate `test(ci): add the Stage 7 product gate runner` commit.
