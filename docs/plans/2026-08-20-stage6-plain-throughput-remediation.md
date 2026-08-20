# Stage 6 Plain-Throughput Remediation Implementation Plan

**Goal:** Restore the protected Windows plain-scroll throughput contract to at
least 5,242,880 bytes/s without changing workload, thresholds, or terminal
semantics.

**Architecture:** Add a narrow direct-ASCII feed path for complete, non-NFC,
non-escape input. Preserve the current UTF-8/control parser as the universal
fallback and prove equivalence with differential tests and frozen traces.

**Tech stack:** Rust 1.89/Cargo, existing R-Term parser and benchmark harness,
PowerShell 7 fixed-runner release workflow, GitHub Actions.

---

### Task 1: Freeze the eligibility and equivalence contracts

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`

1. Add failing unit tests for the direct-ASCII eligibility boundary.
2. Add a differential test seam that can feed the same bytes through the
   optimized and decoded paths.
3. Cover printable ASCII, all ordinary C0 controls, wrapping, scrollback,
   randomized records, and chunk boundaries.
4. Observe RED before adding the production path.

### Task 2: Implement the minimal direct-ASCII path

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`

1. Extract shared printable/C0 state transitions only where needed to prevent
   semantic duplication.
2. Enter the direct path only with empty UTF-8/control pending state, NFC off,
   ASCII-only bytes, and no ESC.
3. Process the chunk without filling UTF-8 or character scratch vectors.
4. Keep every ineligible chunk on the existing decoder.
5. Run focused parser tests and frozen trace tests to GREEN.

### Task 3: Prove the performance contract locally

**Files:**
- Modify only if tests expose a correctness issue.

1. Build the release benchmark with the pinned toolchain and lockfile.
2. Run the fixed protocol with warmups and repeated samples.
3. Require plain throughput at or above 5,242,880 bytes/s.
4. Compare query throughput, p95 chunk/render latency, idle CPU, and RSS with
   both the pre-change candidate and immutable LKG evidence.
5. If the gate remains red, return to root-cause analysis; do not lower limits.

### Task 4: Run regression and quality gates

1. Run `cargo fmt --all -- --check`.
2. Run focused `rterm-terminal` tests and frozen parser traces.
3. Run `cargo clippy --workspace --all-targets --locked -- -D warnings`.
4. Run `cargo test --workspace --all-targets --locked`.
5. Run native GUI and package smoke contracts affected by the release workflow.

### Task 5: Publish, review, merge, and rerun Release

1. Commit the design/plan and implementation in reviewable units.
2. Push `codex/stage6-plain-throughput-remediation` and open a PR.
3. Wait for all required PR checks; address real failures without weakening
   contracts.
4. Merge after review and checks pass.
5. Re-register the protected ephemeral Windows runner, run the main Release
   workflow, and require the full absolute plus 5+40 candidate/LKG gates and
   package smoke to pass.
6. Confirm post-merge CI and CodeQL on `main`.
