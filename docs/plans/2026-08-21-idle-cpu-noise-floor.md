# Idle CPU Noise Floor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Prevent near-zero idle CPU measurement noise from failing the fixed-runner relative regression gate while retaining absolute and material relative protections.

**Architecture:** Extend the existing PowerShell lower-is-better regression helper with an optional minimum absolute delta. Pass a `0.01` percentage-point delta only for idle CPU; all other metrics keep a zero delta and therefore preserve current behavior.

**Tech Stack:** GitHub Actions YAML, PowerShell, Rust static workflow contract tests.

---

### Task 1: Encode the failing workflow contract

**Files:**
- Modify: `crates/rssh-app/src/bench.rs`

**Step 1: Write the failing test**

Require `$idleCpuRegressionNoiseFloor = 0.01`, an absolute-delta guard in
`Test-Lower-Is-BetterRegression`, idle-only use of the floor, and boundary
self-checks for near-zero and material deltas.

**Step 2: Run test to verify it fails**

Run: `cargo test -p rssh-app --bin rssh-app release_workflow_encodes_fixed_runner_performance_contract --locked -j1`

Expected: FAIL because the release workflow does not contain the new noise-floor contract.

### Task 2: Implement the minimum workflow change

**Files:**
- Modify: `.github/workflows/release.yml`

**Step 1: Add the idle CPU noise floor**

Add a `0.01` percentage-point constant and an optional minimum absolute delta
parameter to the lower-is-better wrapper. Record a violation only when both the
relative threshold and absolute-delta threshold are exceeded.

**Step 2: Preserve existing metrics**

Pass the new floor only to `idle_cpu_regression`; latency, rendering, and RSS
continue using the default zero delta.

**Step 3: Run focused verification**

Run the focused release workflow contract test and expect PASS.

### Task 3: Verify, publish, and rerun Release

**Files:**
- Verify: `.github/workflows/release.yml`
- Verify: `crates/rssh-app/src/bench.rs`

**Step 1: Run local verification**

Run formatting, focused Clippy/tests, PowerShell syntax parsing, and
`git diff --check`.

**Step 2: Commit and open a PR**

Commit the design, plan, test, and workflow change together. Push a
`codex/` branch and wait for all PR checks.

**Step 3: Merge and verify main**

Merge only after all checks pass, then require main CI and CodeQL success.

**Step 4: Run a fresh fixed-runner Release**

Register a new ephemeral fixed runner, dispatch `release.yml` from the new main
SHA, and require the fixed performance job plus all six unsigned package jobs
to pass.
