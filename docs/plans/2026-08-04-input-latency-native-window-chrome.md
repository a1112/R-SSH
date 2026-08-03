# Input Latency and Native Window Chrome Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove blocking PTY writes from the native event loop and enable native rounded corners and shadows for the default Windows integrated-titlebar window.

**Architecture:** Add a per-pane FIFO writer worker whose sender is used by the UI thread and whose completion/error events return through `WindowUserEvent`. Extend the existing pane PTY ownership and cleanup transaction to own the writer worker. Use winit 0.30.13 Windows extensions for undecorated shadow and corner preference without adding Win32 bindings.

**Tech Stack:** Rust 2024, std `mpsc`, winit 0.30.13, portable-pty/ConPTY, existing native-window unit and E2E tests.

---

### Task 1: Non-blocking pane input dispatch

**Files:**
- Modify: `crates/rssh-app/src/window.rs`

**Step 1: Write the failing test**

Add a synthetic blocking writer and assert that enqueueing a key payload returns before the writer is released. Add a FIFO assertion for several queued payloads.

**Step 2: Run test to verify it fails**

Run: `cargo test --locked -p rssh-app pane_input_queue_returns_before_blocking_writer -- --nocapture`

Expected: FAIL because `write_pty_bytes` still invokes the blocking writer on the caller thread.

**Step 3: Write minimal implementation**

Introduce a pane input sender, spawn a named writer worker when a pane runtime is created, send owned byte buffers from `write_pty_bytes`, and report completed writes/errors via generation-scoped `WindowUserEvent` variants.

**Step 4: Run focused tests**

Run: `cargo test --locked -p rssh-app pane_input_queue -- --nocapture`

Expected: PASS, including non-blocking dispatch and FIFO ordering.

### Task 2: Writer-worker lifecycle ownership

**Files:**
- Modify: `crates/rssh-app/src/window.rs`

**Step 1: Write the failing cleanup test**

Extend the pane PTY cleanup fixture with a gated writer worker and assert that cleanup neither reports completion nor drops ownership while the writer worker remains active.

**Step 2: Run test to verify it fails**

Run: `cargo test --locked -p rssh-app pane_pty_cleanup_waits_for_writer_worker -- --nocapture`

Expected: FAIL because the current ownership transaction only tracks the reader worker.

**Step 3: Write minimal implementation**

Add the writer worker to `PaneRuntime`, `PanePtyOwnership`, polling, bounded cleanup, and reaper transfer. Drop the input sender at the correct point so the worker can drain and exit.

**Step 4: Run lifecycle tests**

Run: `cargo test --locked -p rssh-app pane_pty_ -- --nocapture`

Expected: PASS with no detached worker.

### Task 3: Windows native rounded corners and shadow

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/tests/native_window_e2e.rs`

**Step 1: Write the failing policy tests**

Add tests for a small platform policy function: default Windows integrated-titlebar windows request undecorated shadow and round corners; decorated or non-Windows windows do not request the undecorated policy.

**Step 2: Run tests to verify they fail**

Run: `cargo test --locked -p rssh-app native_window_chrome_policy -- --nocapture`

Expected: FAIL because no native chrome policy exists.

**Step 3: Write minimal implementation**

Import `CornerPreference`, `WindowExtWindows`, and `WindowAttributesExtWindows` under Windows cfg. Apply `with_undecorated_shadow(true)` before creation and `set_corner_preference(CornerPreference::Round)` after creation for the integrated undecorated default.

**Step 4: Run focused and native tests**

Run: `cargo test --locked -p rssh-app native_window_chrome_policy -- --nocapture`

Run: `cargo test --locked -p rssh-app --test native_window_e2e -- --nocapture`

Expected: PASS.

### Task 4: Final verification

**Files:**
- Modify if needed: `docs/plans/2026-08-04-input-latency-native-window-chrome-design.md`

**Step 1: Run formatting and static checks**

Run: `cargo fmt --all -- --check`

Run: `git diff --check`

Expected: PASS.

**Step 2: Run the complete workspace suite**

Run: `cargo test --locked --workspace --all-targets -j1`

Expected: PASS.

**Step 3: Build and launch the final application**

Run: `cargo build --locked -p rssh-app`

Launch `target/debug/rssh-app.exe window`, type a burst of ASCII and Chinese text, and visually confirm native rounded corners, drop shadow, responsive echo, and correct 4K scaling.

**Step 4: Commit**

Stage only the files listed above and commit with a focused message after verification.
