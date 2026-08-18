# Project Split Stage 1 Types/Domain Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Establish the first enforceable R-Term/R-SSH package boundary by moving shared terminal value types into `rterm-types`, application domain state into `rssh-domain`, renaming the terminal and font packages, and preserving `rssh-core` as a source-compatible facade.

**Architecture:** `rterm-types` is dependency-free and owns terminal/session value types. `rssh-domain` depends only on `rterm-types` and owns window/workspace/tab/pane identifiers, session lifecycle, pane launch metadata, and `AppShell`. `rssh-core` depends on both and re-exports the old public API. Existing consumers may temporarily use the facade, while foundational crates migrate to direct package dependencies. CI enforces the new one-way dependency graph.

**Tech Stack:** Rust 2024 workspace, Cargo package aliases for compatibility, Python architecture policy tests, GitHub Actions, rustfmt, Clippy.

---

## Task 1: Lock the Stage 1 architecture contract

**Files:**
- Modify: `scripts/ci/tests/test_check_rust_architecture.py`
- Modify: `scripts/ci/architecture-policy.json`
- Modify: `scripts/ci/check-rust-architecture.py`

1. Add failing tests that require `rterm-types` and `rssh-domain` workspace members, forbid `rterm-*` packages from depending on `rssh-*`, forbid `rssh-domain` from depending on app/runtime/renderer/transport packages, and require `rssh-core` to remain a facade.
2. Run `python -m unittest scripts.ci.tests.test_check_rust_architecture` and observe the new contract fail.
3. Extend the checked-in policy/checker only as needed to express package dependency rules.
4. Re-run the focused architecture tests and commit the contract.

## Task 2: Introduce `rterm-types`

**Files:**
- Create: `crates/rterm-types/Cargo.toml`
- Create: `crates/rterm-types/src/lib.rs`
- Create: `crates/rterm-types/tests/public_api.rs`
- Modify: `Cargo.toml`
- Modify: `crates/rssh-core/Cargo.toml`
- Modify: `crates/rssh-core/src/lib.rs`

1. Add a compile-level public API test for `SessionId`, `TerminalSize`, and `DamageRegion`, including value, hashing, cell-count, saturation, and empty-region behavior.
2. Run `cargo test -p rterm-types --locked` and observe the missing package failure.
3. Create the dependency-free package and move the three types without changing their public behavior.
4. Make `rssh-core` re-export the types and retain compatibility tests.
5. Run both package test suites and commit.

## Task 3: Introduce `rssh-domain`

**Files:**
- Create: `crates/rssh-domain/Cargo.toml`
- Create: `crates/rssh-domain/src/lib.rs`
- Move: `crates/rssh-core/src/app_shell.rs` to `crates/rssh-domain/src/app_shell.rs`
- Move: `crates/rssh-core/src/session.rs` to `crates/rssh-domain/src/session.rs`
- Create: `crates/rssh-domain/tests/public_api.rs`
- Modify: `Cargo.toml`
- Modify: `crates/rssh-core/Cargo.toml`
- Modify: `crates/rssh-core/src/lib.rs`

1. Add public API tests proving identifier behavior, session lifecycle, local/SSH launch metadata, child-pane remote-command clearing, and basic `AppShell` behavior.
2. Run `cargo test -p rssh-domain --locked` and observe the missing package failure.
3. Move domain identifiers and modules into the new package, importing `SessionId` from `rterm-types`.
4. Re-export all identifiers and both modules from `rssh-core`, preserving paths such as `rssh_core::app_shell::AppShell`.
5. Run domain/core suites and commit.

## Task 4: Rename terminal and font packages

**Files:**
- Move: `crates/rssh-terminal` to `crates/rterm-terminal`
- Move: `crates/rssh-fonts` to `crates/rterm-fonts`
- Modify: both moved `Cargo.toml` files
- Modify: workspace and consumer `Cargo.toml` files
- Modify: `Cargo.lock`

1. Add manifest contract assertions for the new package names and old-name absence.
2. Observe the contract fail.
3. Use `git mv` for both directories and rename the package names.
4. Use explicit Cargo aliases (`rssh-terminal = { package = "rterm-terminal", ... }`, likewise fonts) in compatibility consumers; use direct `rterm-*` names in newly migrated foundation packages.
5. Regenerate the lockfile, run package tests, and commit.

## Task 5: Migrate foundational consumers off the facade

**Files:**
- Modify: `crates/rterm-terminal/Cargo.toml` and Rust sources/tests
- Modify: `crates/rssh-runtime/Cargo.toml` and Rust sources/tests
- Modify: `crates/rssh-renderer/Cargo.toml` and Rust sources/tests
- Modify: `crates/rssh-native/Cargo.toml` and Rust sources/tests
- Modify: `crates/rssh-ssh/Cargo.toml` and Rust sources/tests

1. Add architecture assertions that these packages use `rterm-types` for terminal primitives and `rssh-domain` for pane/window identifiers.
2. Observe violations against the current manifests/imports.
3. Replace direct primitive imports from `rssh-core` with their owning packages. Retain `rssh-core` only where compatibility or full app-shell APIs are genuinely required.
4. Run focused tests for all changed packages and commit.

## Task 6: Enforce the dependency direction in CI and documentation

**Files:**
- Modify: `scripts/ci/architecture-policy.json`
- Modify: `scripts/ci/tests/test_check_rust_architecture.py`
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Modify: `docs/plans/2026-08-18-project-split-stage1-6-design.md`

1. Add a repository-level test that the CI workflow invokes the package-boundary check and the README documents the Stage 1 package ownership.
2. Observe the documentation/CI contract fail.
3. Wire the final policy into the existing architecture job and document the facade/deprecation path.
4. Run architecture tests and commit.

## Task 7: Stage 1 verification and integration

1. Run `cargo fmt --all -- --check`.
2. Run `python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json`.
3. Run `cargo test -p rterm-types -p rssh-domain -p rssh-core --all-targets --locked`.
4. Run focused tests for `rterm-terminal`, `rterm-fonts`, runtime, renderer, native, SSH, and app.
5. Run `cargo test --workspace --all-targets --locked` after generating `web/dist` and satisfying the native test binary precondition.
6. Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
7. Request code review, address findings through RED/GREEN tests, and repeat the relevant verification.
8. Commit any verification-only contract updates, push `codex/project-split-stage1-types-domain`, open a PR, wait for CI, and merge only when required checks pass.

## Stage 2–6 handoff

After Stage 1 merges, create each following branch from the refreshed `origin/main`: Stage 2 runtime/transport ownership, Stage 3 renderer split, Stage 4 snapshot/cache memory reduction, Stage 5 startup/lazy-loading reduction, and Stage 6 release contract. Each stage remains an independently reviewed and revertible PR; Stage 7 physical repository separation is explicitly out of scope.
