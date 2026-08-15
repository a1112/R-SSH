# Functional CI Stabilization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make PR #4's hosted CI and functional matrix pass deterministically while retaining privileged macOS validation as a manual workflow.

**Architecture:** Preserve the scenario/evidence contract and fix failures at platform and lifecycle boundaries. PR aggregation consumes a hosted-only catalog; manual dispatch adds the privileged macOS jobs and validates the full catalog.

**Tech Stack:** Rust 1.89, GitHub Actions YAML, Bash/PowerShell, Playwright, Python 3, cargo-deny.

---

### Task 1: Encode hosted PR versus manual privileged topology

**Files:**
- Modify: `crates/rssh-functional-tests/tests/ci_matrix_contract.rs`
- Modify: `functional-tests/matrix.toml`
- Modify: `functional-tests/fork-matrix.toml`
- Modify: `.github/workflows/functional.yml`
- Modify: `docs/functional-testing.md`

**Step 1: Write the failing contract test**

Add assertions that PR aggregation needs only hosted jobs and validates the hosted matrix, while `workflow_dispatch` aggregation needs the three privileged macOS jobs and validates the full matrix.

**Step 2: Run the test and verify RED**

Run: `cargo test --locked -p rssh-functional-tests --test ci_matrix_contract`

Expected: FAIL because the current trusted PR aggregate requires queued self-hosted jobs.

**Step 3: Implement the minimal workflow split**

Gate privileged jobs and the full aggregate on `github.event_name == 'workflow_dispatch'`; use a hosted aggregate for every PR. Rename the hosted catalog if needed so its role is explicit and update documentation.

**Step 4: Run focused workflow contracts**

Run: `cargo test --locked -p rssh-functional-tests --test ci_matrix_contract --test coverage_contract`

Expected: PASS.

### Task 2: Fix dependency policy and production Web config resolution

**Files:**
- Modify: `deny.toml`
- Modify: `crates/rssh-functional-tests/tests/package_smoke_contract.rs`
- Modify: `.github/workflows/functional.yml`

**Step 1: Add failing assertions**

Assert that `0BSD` is explicitly allowed and that the production Playwright command resolves `web/playwright.production.config.ts` through the package prefix.

**Step 2: Verify RED**

Run: `cargo test --locked -p rssh-functional-tests --test package_smoke_contract`

Expected: FAIL on both current configuration defects.

**Step 3: Apply minimal configuration fixes**

Add `0BSD` to the allowlist and correct the Playwright invocation to use a config path relative to the `web` package.

**Step 4: Verify GREEN**

Run the focused Rust test and `cargo deny check licenses` when `cargo-deny` is available.

### Task 3: Bound Unix observer endpoint paths

**Files:**
- Modify: `crates/rssh-functional-tests/src/observer.rs`
- Modify: `crates/rssh-functional-tests/tests/observer_contract.rs`

**Step 1: Add a Unix-only failing regression test**

Resolve an intentionally long requested path and assert that the actual socket path is short, deterministic, rooted in an owner-only runtime directory, and shared by server and client.

**Step 2: Verify RED on a Unix target**

Run the test in the available Linux environment; if local Linux is unavailable, use a compile-time contract test and verify the behavioral test in GitHub Actions.

**Step 3: Implement deterministic endpoint hashing**

Move the existing FNV path hash to all platforms. On Unix, place the socket under a protected short runtime directory and retain the requested evidence path only as identity input.

**Step 4: Verify GREEN**

Run observer unit and contract tests plus Clippy for `rssh-functional-tests`.

### Task 4: Make Wayland and X11 readiness bounded and diagnostic

**Files:**
- Modify: `crates/rssh-functional-tests/tests/platform_helpers_contract.rs`
- Modify: `scripts/functional/run-wayland-seat.sh`
- Modify: `scripts/functional/x11-xtest-input.sh`
- Modify: `crates/rssh-functional-tests/src/platform_input.rs`

**Step 1: Add failing script contracts**

Assert that Weston socket and visible-window readiness are separately polled, `kill -0` cannot mask the compositor log, cleanup is idempotent, and X11 discovery can follow descendants of the launched PID.

**Step 2: Verify RED**

Run: `cargo test --locked -p rssh-functional-tests --test platform_helpers_contract --test platform_input_contract`

Expected: FAIL on one-shot readiness and direct-PID discovery.

**Step 3: Implement minimal readiness and discovery changes**

Use bounded condition loops, dump logs before exit, tolerate already-exited cleanup targets, and search the process tree before rejecting the window.

**Step 4: Verify GREEN**

Run focused contracts and `bash -n` for both scripts.

### Task 5: Make browser clipboard coverage engine-aware

**Files:**
- Modify: `web/tests/terminal.spec.ts`
- Modify: `crates/rssh-functional-tests/tests/web_matrix_contract.rs`

**Step 1: Add a failing source contract or Playwright test**

Require Chromium-only clipboard permission grants and preserve Firefox/WebKit coverage through capability-aware assertions.

**Step 2: Verify RED**

Run the Web matrix contract and list Firefox/WebKit tests.

**Step 3: Implement engine-aware permission setup**

Inspect `browserName`; grant clipboard permissions only for Chromium and use the existing browser clipboard behavior otherwise.

**Step 4: Verify GREEN**

Run Web lint, unit tests, builds, and Playwright test enumeration for all three projects.

### Task 6: Normalize PTY shutdown and stress synchronization

**Files:**
- Modify: `crates/rssh-functional-tests/src/pty_driver.rs`
- Modify: `crates/rssh-functional-tests/src/runner.rs`
- Modify: `crates/rssh-functional-tests/tests/pty_driver_contract.rs`
- Modify: `crates/rssh-functional-tests/tests/runner_contract.rs`

**Step 1: Add deterministic failing lifecycle tests**

Cover a fixture that exits immediately after output, a reader that disconnects after the expected payload, a master whose writer is already closed, and a stress run whose synchronization marker is split across reads.

**Step 2: Verify RED**

Run the two focused test targets repeatedly and under coverage when available.

**Step 3: Implement minimal lifecycle ordering**

Drain expected output before classifying disconnect, make already-closed writer shutdown idempotent, and derive synchronization from the full accumulated stream rather than a single chunk.

**Step 4: Verify GREEN**

Run focused tests repeatedly and then the functional-test crate with all targets.

### Task 7: Harden remaining hosted platform smoke paths

**Files:**
- Modify: `scripts/functional/windows-send-input.ps1`
- Modify: `scripts/functional/smoke-production-tauri.sh`
- Modify: `crates/rssh-functional-tests/src/transport_driver.rs`
- Modify: `crates/rssh-functional-tests/src/runner.rs`
- Modify: corresponding contract tests under `crates/rssh-functional-tests/tests/`

**Step 1: Reproduce and encode each remaining failure**

Add one regression contract for Windows cursor positioning failure propagation, SCP fixture protocol acceptance, observer readiness/revision advancement, macOS pane startup, and production Tauri PTY-child observation.

**Step 2: Verify each test RED before its implementation change**

Run only the relevant contract target for each defect.

**Step 3: Fix one root cause at a time**

Keep input targeting process-scoped, align the SCP fixture with the exact OpenSSH invocation, wait on observer state transitions rather than time, and capture startup diagnostics before smoke failure.

**Step 4: Verify each fix GREEN**

Run the focused test after every change and retain all prior focused tests in the command.

### Task 8: Full verification, review, commit, push, and monitor

**Files:**
- Review all modified files.

**Step 1: Run formatting and static checks**

Run `cargo fmt --all -- --check`, workspace Clippy with `-D warnings`, Python policy tests, shell syntax checks, Web lint, unit tests, and builds.

**Step 2: Run complete test suites**

Run `cargo test --locked --workspace --all-targets` and all available Playwright projects or their hermetic enumeration.

**Step 3: Review staged scope**

Run `git diff --check`, inspect the full staged stat/name status, verify no unrelated files or secrets, and request code review.

**Step 4: Commit and push**

Create focused commits, push `codex/functional-tests`, and confirm the remote hash matches local `HEAD`.

**Step 5: Monitor PR checks**

Watch PR #4 CI and Functional runs. If a new failure appears, return to systematic debugging and repeat RED/GREEN for that root cause before another push.
