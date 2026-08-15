# Functional Test Review Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the functional-test infrastructure safe to run from pull requests and ensure its CI, cleanup, and behavior-coverage contracts are truthful before committing and pushing the branch.

**Architecture:** Keep the fixes at the existing contract boundaries: workflow policy for runner trust, static contract tests for CI/script wiring, an explicit synchronous `ChildGuard` termination API for owned forwarding processes, and catalog validation for scenario surfaces. Preserve the fixed matrix and production/functional build separation.

**Tech Stack:** GitHub Actions YAML, Rust 1.89, PowerShell, Playwright/Vite, Python CI checks.

---

### Task 1: Gate privileged self-hosted runners

**Files:**
- Modify: `.github/workflows/functional.yml`
- Test: `crates/rssh-functional-tests/tests/ci_matrix_contract.rs`

1. Add a contract test that finds every job using the `rssh-accessibility` self-hosted label and requires the trusted-source expression `github.event_name == 'workflow_dispatch' || github.event.pull_request.head.repo.full_name == github.repository`.
2. Run `cargo test --locked -p rssh-functional-tests --test ci_matrix_contract` and verify the new assertion fails.
3. Add the trusted-source job condition to `native-macos-accessibility`, `tauri-platform`, and `production-tauri-bundle-smoke`.
4. Re-run the focused test and verify it passes.

### Task 2: Keep baseline Web CI on its installed functional target

**Files:**
- Modify: `.github/workflows/ci.yml`
- Test: `crates/rssh-functional-tests/tests/web_matrix_contract.rs`

1. Add a contract test requiring baseline CI to build functional Web assets before browser E2E and invoke Playwright with `--project=chromium`.
2. Run `cargo test --locked -p rssh-functional-tests --test web_matrix_contract` and verify the assertion fails.
3. Build `web/dist` in functional mode after the production Web build and constrain baseline E2E to Chromium; leave the dedicated functional matrix responsible for Firefox and WebKit.
4. Re-run the focused test and verify it passes.

### Task 3: Pass JSON to the Windows input helper

**Files:**
- Modify: `scripts/functional/smoke-production-tauri.ps1`
- Test: `crates/rssh-functional-tests/tests/platform_input_contract.rs`

1. Add a static contract test requiring `-ActionArgumentsJson` with JSON arrays for text, key, and window-close actions, and rejecting the obsolete `-ActionArguments` spelling.
2. Run `cargo test --locked -p rssh-functional-tests --test platform_input_contract` and verify it fails.
3. Change the three helper calls to valid JSON-array arguments.
4. Re-run the focused test and verify it passes.

### Task 4: Synchronously reap the SSH forwarding child

**Files:**
- Modify: `crates/rssh-test-support/src/process.rs`
- Modify: `crates/rssh-functional-tests/src/transport_driver.rs`
- Test: `crates/rssh-test-support/src/process.rs`
- Test: `crates/rssh-functional-tests/tests/transport_driver_contract.rs`

1. Add a `ChildGuard` unit test showing an explicit terminate operation kills, captures, and reaps a long-running child within the cleanup bound.
2. Add a transport contract assertion that the forwarding journey explicitly terminates the guard and does not rely on `drop(forward_process)`.
3. Run the focused tests and verify both fail for the expected missing API/contract.
4. Implement `ChildGuard::terminate` using the existing bounded cleanup and capture paths; return `CleanupDeferred` if synchronous reaping cannot finish.
5. Replace the forwarding guard drop with explicit termination and error propagation.
6. Re-run both focused tests and the live transport journey.

### Task 5: Enforce behavior surfaces

**Files:**
- Modify: `crates/rssh-functional-tests/src/catalog.rs`
- Modify: `functional-tests/behaviors.toml`
- Test: `crates/rssh-functional-tests/tests/scenario_contract.rs`

1. Add a catalog test where a known console-only behavior is referenced by a host-terminal scenario and require an actionable surface-mismatch error.
2. Run the focused scenario contract and verify it fails.
3. Extend `validate_catalog` to check that every referenced behavior declares the scenario surface.
4. Add `host_terminal` to the five generic behaviors genuinely exercised by `host-terminal.smoke`: lifecycle start/exit, focus, type-text, and key input.
5. Re-run the scenario contract and `rssh-functional validate --suite functional-tests`.

### Task 6: Final verification and publication

**Files:**
- Update generated fixture: `crates/rssh-app/tests/fixtures/task23_app_test_manifest.txt`
- Stage: all intended functional-test changes

1. Run `cargo fmt --all -- --check`.
2. Run the Python matrix/hermeticity tests and hermeticity checker.
3. Run `cargo clippy --locked --workspace --all-targets -- -D warnings`.
4. Run `cargo test --locked --workspace --all-targets`.
5. Run Web lint, unit tests, production build, and functional build.
6. Inspect `git diff --check`, the complete staged diff, and ensure no generated evidence, credentials, or unrelated worktree files are staged.
7. Commit with a concise Conventional Commit message and push `codex/functional-tests` to `origin` with upstream tracking.
