# Stage 6 Cross-Repository Release Contract Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the logical R-Term surface consumable, comparable, and rollback-safe across a future repository boundary without performing the Stage 7 physical split.

**Architecture:** Check in one versioned release-contract document that declares the R-Term packages, owned paths, immutable last-known-good ref, consumer commands, vendor patch identities, and history extraction paths. A Python rehearsal tool creates clean candidate/consumer checkouts, overlays only declared R-Term paths, runs a standalone public-API probe plus real R-SSH consumer gates, and repeats them with the last-known-good ref. Hosted CI enforces deterministic compatibility; the protected Windows fixed runner remains authoritative for package and performance comparison.

**Tech Stack:** Rust 1.89/Cargo, Python 3 standard library, PowerShell 7, Git, GitHub Actions, existing Stage 0/4/5 diagnostics and package-smoke scripts.

---

### Task 0: Remove the final R-Term to R-SSH identity dependency

**Files:**
- Modify: `crates/rterm-types/src/lib.rs`
- Modify: `crates/rssh-domain/src/lib.rs`
- Modify: `crates/rssh-runtime/Cargo.toml`
- Modify: `crates/rssh-runtime/src/api.rs`
- Modify: `crates/rssh-runtime/src/hub.rs`
- Modify: `crates/rssh-runtime/src/terminal.rs`
- Modify: `crates/rssh-runtime/src/task10_runtime_trace_codec.rs`
- Modify: `crates/rssh-runtime/tests/api_contract.rs`
- Modify: `crates/rssh-runtime/tests/burst.rs`
- Modify: `crates/rssh-runtime/tests/pane_worker.rs`

**Step 1: Extend the existing architecture contract and observe RED**

Require the `rterm-runtime` manifest and source tree to contain no `rssh-domain`,
`rssh-core`, or `rssh_domain` references. Require its public pane token APIs to
accept the neutral `rterm_types::PaneId`.

**Step 2: Move the shared identity to the neutral leaf crate**

Define `PaneId` in `rterm-types`; re-export that exact type from `rssh-domain`
for source compatibility. Migrate runtime implementation and tests to R-Term
types and remove the two reverse manifest dependencies.

**Step 3: Verify the boundary**

Run the focused API contract, all `rterm-runtime` targets and workspace Clippy.
Confirm with `cargo metadata --locked --no-deps` that no package whose name
starts with `rterm-` depends on a package whose name starts with `rssh-`.

**Step 4: Commit**

```powershell
git commit -m "refactor: remove R-Term reverse dependencies"
```

### Task 1: Freeze the machine-readable R-Term release contract

**Files:**
- Create: `scripts/ci/rterm-release-contract.json`
- Create: `docs/release/rterm-api-compatibility.md`
- Create: `docs/release/rterm-history-paths.txt`
- Create: `scripts/ci/check-rterm-release-contract.py`
- Create: `scripts/ci/tests/test_check_rterm_release_contract.py`

**Step 1: Write the failing contract tests**

Add `unittest` cases that require:

- schema version `1`, API line `0.1`, and an exact 40-character lowercase last-known-good commit;
- all seven public packages (`rterm-types`, `rterm-terminal`, `rterm-runtime`, `rterm-fonts`, `rterm-render-core`, `rterm-render-cpu`, `rterm-render-wgpu`) with exact package paths and version `0.1.0`;
- no declared R-Term package dependency whose package name starts with `rssh-`;
- the exact vendor tree IDs for `glyphon` and `gpu-allocator`, plus the explicit `consumer-root-path-patch` strategy;
- old and new terminal/runtime/fonts/renderer paths in the extraction map; and
- failure for mutable refs, missing paths, mismatched package versions, reverse dependencies, or vendor tree drift.

**Step 2: Run the tests and observe RED**

Run:

```powershell
python -m unittest scripts.ci.tests.test_check_rterm_release_contract -v
```

Expected: FAIL because the contract, checker, compatibility document, and history map do not exist.

**Step 3: Implement the minimal contract and checker**

Use `cargo metadata --locked --no-deps --format-version 1` for package identity and dependency direction. Use `git rev-parse <candidate>:<path>` for immutable vendor tree identities and `git log --all -- <path>` to prove historical predecessor paths. Emit a compact JSON result with `ok`, checked ref, package list, vendor trees, and violations.

The contract must pin:

```json
{
  "schema_version": 1,
  "api_compatibility_line": "0.1",
  "last_known_good_rterm_ref": "31e40c191fcaaed118f5a8822854a66882de4564",
  "vendor_patch_strategy": "consumer-root-path-patch"
}
```

Document that patch releases are compatible additions/fixes, breaking changes require the next minor version, releases use immutable commits, and Stage 7 remains unauthorized.

**Step 4: Verify GREEN**

Run:

```powershell
python -m unittest scripts.ci.tests.test_check_rterm_release_contract -v
python scripts/ci/check-rterm-release-contract.py --contract scripts/ci/rterm-release-contract.json
```

Expected: all tests pass and the checker emits `"ok": true`.

**Step 5: Commit**

```powershell
git add scripts/ci/rterm-release-contract.json scripts/ci/check-rterm-release-contract.py scripts/ci/tests/test_check_rterm_release_contract.py docs/release/rterm-api-compatibility.md docs/release/rterm-history-paths.txt
git commit -m "feat: freeze the R-Term release contract"
```

### Task 2: Add a standalone downstream public-API probe

**Files:**
- Create: `contracts/rterm-consumer/Cargo.toml`
- Create: `contracts/rterm-consumer/Cargo.lock`
- Create: `contracts/rterm-consumer/src/main.rs`
- Modify: `Cargo.toml`
- Modify: `scripts/ci/tests/test_check_rterm_release_contract.py`

**Step 1: Write the failing probe contract**

Require the standalone manifest to depend on every declared R-Term package at
`version = "0.1.0"`, use only R-Term package names, live outside the root
workspace, and compile representative public APIs for terminal sizes/damage,
terminal parsing/snapshots, runtime transport-neutral batches, fonts, render
snapshots, CPU rendering, and WGPU context configuration.

**Step 2: Observe RED**

Run:

```powershell
python -m unittest scripts.ci.tests.test_check_rterm_release_contract.RTermReleaseContractTests.test_standalone_consumer_declares_only_rterm_packages -v
```

Expected: FAIL because `contracts/rterm-consumer` does not exist.

**Step 3: Implement the probe**

Add `contracts/rterm-consumer` to `workspace.exclude`. Give the probe its own
lockfile. Do not use R-SSH compatibility facades or product crates; package aliases
are forbidden in this manifest.

**Step 4: Verify GREEN**

Run:

```powershell
cargo check --locked --manifest-path contracts/rterm-consumer/Cargo.toml
cargo tree --manifest-path contracts/rterm-consumer/Cargo.toml --prefix none
```

Expected: check succeeds and the tree contains no package whose name starts with `rssh-`.

**Step 5: Commit**

```powershell
git add Cargo.toml contracts/rterm-consumer scripts/ci/tests/test_check_rterm_release_contract.py
git commit -m "test: compile the declared R-Term downstream API"
```

### Task 3: Rehearse clean R-SSH candidate consumption and rollback

**Files:**
- Create: `scripts/ci/rehearse-rterm-consumer.py`
- Create: `scripts/ci/tests/test_rehearse_rterm_consumer.py`
- Modify: `scripts/ci/rterm-release-contract.json`

**Step 1: Write failing unit and integration seams**

Cover path containment, candidate/consumer ref resolution, clean-clone creation,
replacement of exactly the declared R-Term and vendor paths, refusal to copy
R-SSH product paths, stable command recording, subprocess failure propagation,
and rollback to the configured last-known-good ref. Use temporary Git fixtures;
do not mock Git command output.

**Step 2: Observe RED**

Run:

```powershell
python -m unittest scripts.ci.tests.test_rehearse_rterm_consumer -v
```

Expected: FAIL because the rehearsal tool is missing.

**Step 3: Implement candidate and rollback modes**

The tool must:

1. resolve candidate, consumer, and last-known-good refs to full commits;
2. make independent non-local clones under a caller-provided output directory;
3. replace only contract-owned R-Term/vendor paths in the consumer;
4. run the standalone API probe and configured real consumer commands;
5. repeat with the last-known-good R-Term paths;
6. write candidate and rollback evidence JSON atomically; and
7. clean temporary checkouts on success while retaining them on failure.

Initial consumer commands:

```text
cargo check --locked -p rssh-app --no-default-features --features production-gui
cargo test --locked -p rssh-ssh --all-targets
cargo test --locked -p rssh-pty --all-targets
cargo test --locked -p rssh-native --all-targets
cargo test --locked -p rssh-functional-tests --all-targets
```

**Step 4: Verify GREEN on the real repository**

Run a bounded local rehearsal with the focused command set, then inspect both
evidence files for full immutable refs and zero failures.

**Step 5: Commit**

```powershell
git add scripts/ci/rehearse-rterm-consumer.py scripts/ci/tests/test_rehearse_rterm_consumer.py scripts/ci/rterm-release-contract.json
git commit -m "feat: rehearse R-Term consumption and rollback"
```

### Task 4: Enforce the deterministic contract in hosted CI

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `scripts/ci/tests/test_check_rterm_release_contract.py`

**Step 1: Write a failing workflow contract test**

Require a dedicated `rterm-consumer-contract` job with `fetch-depth: 0`, read-only
permissions, the pinned Rust toolchain, contract/unit validation, standalone API
probe, clean candidate rehearsal, last-known-good rollback rehearsal, and evidence
upload. Assert that hosted CI does not enforce absolute memory or startup limits.

**Step 2: Observe RED**

Run the workflow contract test and confirm it fails because the job is absent.

**Step 3: Add the CI job**

Use Ubuntu for deterministic compile/test coverage. Give the job a bounded
timeout and no secrets or write permissions. Upload both candidate and rollback
evidence with `if-no-files-found: error`.

**Step 4: Verify GREEN**

Run all Stage 6 Python tests, parse the workflow as YAML through the existing
workflow contract surface, and run the standalone probe.

**Step 5: Commit**

```powershell
git add .github/workflows/ci.yml scripts/ci/tests/test_check_rterm_release_contract.py
git commit -m "ci: enforce the R-Term consumer and rollback contract"
```

### Task 5: Add protected fixed-runner candidate/LKG comparison

**Files:**
- Create: `scripts/ci/run-rterm-release-comparison.ps1`
- Modify: `.github/workflows/release.yml`
- Modify: `crates/rssh-app/tests/performance_scorecard_contract.rs`

**Step 1: Write failing static and PowerShell contract tests**

Require the protected fixed-performance job to compare candidate and immutable
last-known-good minimal production GUI binaries on the same runner. Require 5
warmups and 40 process-cold samples, Stage 5 absolute limits, a 5% first-present
and memory relative-regression ceiling, structured evidence upload, and package
smoke after candidate and rollback builds. Assert the script is absent from PR CI.

**Step 2: Observe RED**

Run:

```powershell
cargo test --locked -p rssh-app --test performance_scorecard_contract rterm_release -j1
```

Expected: FAIL because the comparison script and workflow step are missing.

**Step 3: Implement the comparison runner**

The script resolves both commits, creates isolated target/output directories,
builds `rssh-app --no-default-features --features production-gui --release` for
each, calls the existing startup harness without changing its metric semantics,
validates package smoke, computes same-machine ratios, and emits one JSON report.
It must reject mutable refs, missing samples, mismatched machine fingerprints,
or a candidate that violates either absolute or relative thresholds.

**Step 4: Wire the release job and verify GREEN**

Parse the PowerShell AST, run boundary unit tests for ratio comparison, run the
Rust workflow contract test, and perform a one-sample local smoke before the
protected runner executes the authoritative 5+40 gate.

**Step 5: Commit**

```powershell
git add scripts/ci/run-rterm-release-comparison.ps1 .github/workflows/release.yml crates/rssh-app/tests/performance_scorecard_contract.rs
git commit -m "ci: compare candidate and rollback R-Term releases"
```

### Task 6: Final verification, review, and integration

**Files:**
- Modify only files required by review findings.

**Step 1: Verify the Stage 6 checklist**

Run:

```powershell
python -m unittest discover -s scripts/ci/tests -p 'test_*rterm*.py' -v
python scripts/ci/check-rterm-release-contract.py --contract scripts/ci/rterm-release-contract.json
cargo check --locked --manifest-path contracts/rterm-consumer/Cargo.toml
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Also run package smoke, the real clean-clone candidate/rollback rehearsal, and a
bounded local comparison probe. Confirm no Stage 7 repository creation, history
rewrite, or dependency-source switch is present in the diff.

**Step 2: Review the complete diff**

Compare against `origin/main`, verify every declared path and command, inspect
failure cleanup and secret handling, and resolve all important findings before
publication.

**Step 3: Commit any review fixes and push**

Use the branch `codex/project-split-stage6-release-contract`.

**Step 4: Open PR, monitor all checks, and merge**

After PR checks are green, merge to `main`, delete the remote branch, and monitor
post-merge CI/CodeQL. The protected fixed-runner release workflow remains a
manual/default-branch or tag gate and must be run before declaring Stage 6 exit.
