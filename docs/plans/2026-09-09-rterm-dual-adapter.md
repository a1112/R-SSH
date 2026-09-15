# R-Term Dual Adapter Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restore current-consumer/frozen-R-Term rollback without losing modern GUI optimizations or weakening extraction gates.

**Architecture:** Use an app-owned legacy adapter selected by a narrowly generated manifest in a verified disposable checkout. Keep modern behavior and all three frozen baseline references intact. Local and CI builds use the same preparation and evidence path.

**Tech Stack:** Rust/Cargo, Python 3.11+ standard library, PowerShell, Git, existing native GPU/SSH tests and GitHub Actions.

---

## Workspace and checkpoints

Work in `.worktrees/stage7-split-readiness` on `codex/stage7-split-readiness`.
Preserve the nine unfinished Task 12 draft files. Use `L:/rssh-targets` for Cargo
and temporary evidence, not the space-constrained repository drive. The starting
commit is `a8430ed94c6c74ddd4d7df3b80cc58955030195c`; it has known rollback,
Windows probe and local crash concerns. This is not a green baseline.

Execute bounded tasks with TDD and independent commits. Do not wire legacy
preparation into the release rehearsal until both adapters exist. Never claim
the preparer alone fixes the failing rollback check.

### Task 1: Pure profile selection and manifest transformation

**Files:** Create `scripts/ci/rterm_consumer_profile.py` and
`scripts/ci/tests/test_rterm_consumer_profile.py`.

1. Write tests using real TOML bytes for frozen identity, modern feature
   declarations, unknown sources, missing/partial capabilities, and malformed
   input. The frozen profile requires exact declared LKG identity, not simply
   absence of modern features. Profile selection is declaration checking, not
   proof that Rust APIs compile.
2. Run `python -m unittest discover -s scripts/ci/tests -p test_rterm_consumer_profile.py -v`.
   Expect assertions to fail because the new module/behavior is absent.
3. Implement `select_profile(source_ref, lkg_ref, fonts_manifest)` returning
   `modern` or `legacy-0.1`; require full lowercase SHAs and the expected package
   name. Modern requires both forwarded feature declarations; partial/unknown
   declarations fail. Require the source to have been verified by Task 2 before
   trusting these inputs for a real build.
4. Test/implement `prepare_app_manifest(original, profile) -> bytes`.
   Modern returns the exact original bytes. Legacy requires the declared empty
   `rterm-legacy-0-1` feature, transforms only the two forwarding arrays and
   appends the selector to `production-gui`. Reject unexpected feature values,
   repeated preparation, a pre-enabled selector, and ambiguous formatting.
   Verify parsed TOML outside those arrays is identical. Test CRLF preservation.
5. Run the focused suite plus existing release/rehearsal tests and `git diff --check`.
   Commit only the new module/tests after review. No existing CI contract change.

### Task 2: Verified disposable-checkout preparation and receipts

**Files:** Create `scripts/ci/prepare-rterm-consumer.py` and
`scripts/ci/tests/test_prepare_rterm_consumer.py`; reuse Task 1 and existing
release path validation in `scripts/ci/rehearse-rterm-consumer.py`.

Execution refinement: declare the inert app selector here (originally Task 3)
so a committed real consumer can be prepared for the frozen source. This does
not implement or enable legacy Rust behavior in ordinary profiles. The CLI
creates its own new external output checkout instead of accepting permission to
rewrite an existing one; local worktree edits are never exported. Python 3.11+
is required (the local bundled Python 3.12 is used, not PATH's Python 3.9).

1. Add real temporary Git fixture tests for both profiles. Assert source trees
   are exact, retained product files match consumer commit, and only the
   allowlisted manifest/lockfile changes occur. Test dirty/untracked/ignored
   source injection, symlinks/junctions, path escape, invalid commit, source
   mismatch, unknown capability and existing receipt/output refusal.
2. Run `python -m unittest discover -s scripts/ci/tests -p test_prepare_rterm_consumer.py -v`;
   observe RED before implementing the CLI.
3. Require explicit repository, immutable source/consumer refs, contract and
   output receipt. Verify identity before any write. Prepare only disposable
   checkouts, fail closed on mutation outside the allowlist, and regenerate the
   lockfile. Do not infer trust from `RTERM_REHEARSAL_MODE` or merely HEAD.
4. Record source tree IDs, profile, manifest before/after SHA-256 and lock SHA-256;
   preserve failures. Prove lock generation failure cannot emit a success receipt.
5. Run all focused tests and independently inspect a real frozen-source checkout.
   Keep legacy Rust compilation failure visible until Tasks 3-4 are complete.

### Task 3: Font adapter and capability boundary

**Files:** Modify `crates/rssh-app/Cargo.toml`,
`crates/rssh-app/src/platform_fonts.rs`, `crates/rssh-app/src/window_gpu.rs`;
create `crates/rssh-app/src/rterm_compat.rs` and its modern/legacy implementation
modules as needed; update feature-contract tests only to express the approved
dual-profile contract.

1. Use the inert selector declared in Task 2 and add RED tests for legacy ordered transactional
   expansion, invalid-source rollback, incarnation-aware invalidation, late
   fallback and explicit unsupported diagnostics. Run modern focused tests and
   compile the same source against frozen packages to observe actual API errors.
2. Introduce the narrow adapter boundary without changing modern allocations.
   Legacy builds complete replacement catalogs using old public APIs; commit
   catalog and logical generation only after success. Never patch old packages.
3. Route unavailable diagnostics to explicit unsupported errors. Preserve all
   existing modern diagnostic tests and checks against secret exposure.
4. Run `cargo test --locked -p rssh-app platform_fonts -- --test-threads=1`
   on modern, then corresponding real legacy-profile tests. Compare render
   snapshots and full-width/CJK fallback behavior; regenerate the Task 23 test
   manifest with `scripts/ci/update-task23-test-manifest.ps1`.
5. Review and commit the adapter, tests and updated test inventory together.

### Task 4: GPU adapter, full-frame safety and recovery

**Files:** Modify `crates/rssh-app/src/window_gpu.rs` and the compatibility
modules; add focused app GPU tests beside existing tests.

1. Write RED tests against real old GPU APIs for initial full frame, catalog
   replacement, partial-frame discard, resize and lost-device retirement.
2. Keep modern hooks unchanged. Implement legacy full-frame preparation with
   old renderer methods and safe cache reset/rebuild. Preserve the native
   teardown workaround and CPU fallback; do not synthesize unavailable resource
   metrics or force CPU-only rendering.
3. Run `cargo test --locked -p rssh-app gpu_text_frame -- --test-threads=1`
   and native device-loss/resize tests for both profiles. Require actual GPU
   execution, not only mocked success. Verify static missing glyphs converge.
4. Build both profiles with `--no-default-features --features production-gui`
   and `production-gui,transfer-tools`. Test diagnostic invocation rejects
   unsupported legacy operations with a clear non-success result.
5. Review, update the Task 23 manifest, and commit.

### Task 5: Integrate the unchanged rollback promise

**Files:** Modify `scripts/ci/rehearse-rterm-consumer.py`,
`scripts/ci/rterm-release-contract.json`, related Python tests,
`.github/workflows/ci.yml`, and `docs/release/rterm-api-compatibility.md`.

1. Test that rehearsal runs the common preparation command and retains receipts
   for both modes without changing product Rust or any frozen source tree.
   Keep all existing consumer commands. Add assertions that failure cannot be
   converted into a skip/GO and all immutable baseline fields stay unchanged.
2. Wire Task 2 only after both real adapters build. Include exact generated
   manifest/lock provenance and executable/package hashes in retained evidence.
3. Run candidate and frozen rollback rehearsals against the exact committed
   consumer SHA. Add packaged GUI/native SSH/GPU functional tests for both.
   Preserve all failed-mode logs; do not relabel legacy evidence as performance GO.
4. Run release-contract and rehearsal Python suites, then the full real rehearsal.
   Review the artifacts and code before committing integration.

### Task 6: Independent failures, merge and original extraction gates

**Files:** Investigate `crates/rssh-test-support/src/windows/window_probe.rs`,
`crates/rssh-test-support/tests/windows_window_probe.rs` and the original local
access-violation evidence. Edit only after a root-cause-specific RED test.

1. Diagnose all window-probe error paths; preserve PID/enumeration context on
   helper failure instead of increasing timeout or removing the assertion.
2. Capture/reproduce the local access violation with bounded tests/debugger;
   isolated passes are not evidence that the full-suite crash is resolved.
3. Run `cargo test --locked --workspace --all-targets`,
   `cargo clippy --locked --workspace --all-targets -- -D warnings`,
   `cargo fmt --all -- --check`, functional/package tests and exact-SHA CI.
4. Use requesting-code-review and finishing-a-development-branch. Merge only
   after all required evidence passes; preserve frozen-baseline ancestry.
5. Resume original Stage 7 Task 12 certification and Task 13+ extraction work.
   Fixed-runner availability and the original absolute/relative budgets remain
   mandatory. This repair is not physical-split approval by itself.
