# Xterm Mouse Protocol Expansion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add faithful `1005` UTF-8 mouse and `1015` urxvt mouse support while keeping `1016` SGR-pixels unsupported until pixel coordinates exist.

**Architecture:** Extend the shared mouse mode tracker so console and native window paths observe the same negotiated protocol. Reuse the existing button/modifier calculation and add protocol-specific encoders in both input paths.

**Tech Stack:** Rust, crossterm input events, winit input events, existing R-SSH terminal runtime tests.

---

### Task 1: Shared Mode Tracking

**Files:**
- Modify: `crates/rssh-app/src/terminal_modes.rs`
- Test: `crates/rssh-app/src/terminal_modes.rs`

**Step 1: Write failing tests**

Add tests that assert:

- `ESC[?1000;1005h` emits `MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Utf8)`.
- `ESC[?1000;1015h` emits `MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Urxvt)`.
- `private_mode_report_value(1005)` and `private_mode_report_value(1015)` return `1` when enabled and `2` when disabled.
- `private_mode_report_value(1016)` returns `0`.
- When `1005`, `1015`, and `1006` are enabled, effective protocol preference is SGR, then urxvt, then UTF-8, then X10 as protocols are disabled.

**Step 2: Run test to verify failure**

Run:

```powershell
cargo test -p rssh-app mouse
```

Expected: tests fail because `Utf8` and `Urxvt` variants do not exist and `1005`/`1015` are not tracked.

**Step 3: Implement minimal tracking**

Update:

- `MouseProtocolMode`
- `MouseInputMode::bits/from_bits`
- `MouseModes` bit masks
- `MouseModes::input_mode`
- `MouseModes::mask`
- `TerminalModeTracker::private_mode_report_value`

**Step 4: Run test to verify pass**

Run:

```powershell
cargo test -p rssh-app mouse
```

Expected: new and existing mode tests pass.

### Task 2: Console Mouse Encoding

**Files:**
- Modify: `crates/rssh-app/src/local.rs`
- Test: `crates/rssh-app/src/local.rs`

**Step 1: Write failing tests**

Add tests that assert:

- UTF-8 protocol encodes a left click at `(95, 96)` as `ESC [ M` plus UTF-8 bytes for button code `32`, column `128`, row `129`.
- UTF-8 protocol encodes release with the X10-style release code.
- urxvt protocol encodes down as `ESC [ 32 ; Cx ; Cy M` and release as `ESC [ 35 ; Cx ; Cy M`.

**Step 2: Run test to verify failure**

Run:

```powershell
cargo test -p rssh-app encodes_mouse_events_as
```

Expected: tests fail because the new protocol variants are not encoded.

**Step 3: Implement minimal encoding**

Add helper functions for UTF-8 and urxvt mouse encoding. Keep current legacy and
SGR behavior unchanged.

**Step 4: Run test to verify pass**

Run:

```powershell
cargo test -p rssh-app encodes_mouse_events_as
```

Expected: mouse encoding tests pass.

### Task 3: Native Window Mouse Encoding

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write failing tests**

Mirror the console UTF-8 and urxvt encoder tests for `WindowMouseEvent`.

**Step 2: Run test to verify failure**

Run:

```powershell
cargo test -p rssh-app encodes_window_mouse_events_as
```

Expected: tests fail because the native window encoder does not handle the new protocol variants.

**Step 3: Implement minimal encoding**

Add window-specific UTF-8 and urxvt helpers or share logic locally without changing
the public API.

**Step 4: Run test to verify pass**

Run:

```powershell
cargo test -p rssh-app encodes_window_mouse_events_as
```

Expected: native window mouse encoding tests pass.

### Task 4: Runtime Query Coverage

**Files:**
- Modify: `crates/rssh-app/src/terminal_runtime.rs`
- Test: `crates/rssh-app/src/terminal_runtime.rs`

**Step 1: Write failing tests**

Add tests that assert:

- Runtime DECRQM responds enabled for `1005` and `1015`.
- Runtime DECRQM responds unknown for `1016`.
- `TerminalRuntime::mouse_input_mode()` follows the shared fallback sequence.

**Step 2: Run test to verify failure**

Run:

```powershell
cargo test -p rssh-app mouse
```

Expected: new runtime assertions fail before implementation.

**Step 3: Implement any missing runtime integration**

Runtime should need little or no new production code if the tracker is correct.

**Step 4: Run test to verify pass**

Run:

```powershell
cargo test -p rssh-app mouse
```

Expected: runtime mouse tests pass.

### Task 5: Documentation, Verification, Packaging

**Files:**
- Modify: `README.md`
- Modify: `docs/mvp-2-local-terminal.md`
- Modify: `docs/mvp-4-live-pty-window.md`
- Modify: `dist/R-SSH-windows-x64.zip`

**Step 1: Update docs**

Document `1005`, `1006`, and `1015` support. State that `1016` is not declared
until pixel-coordinate input exists.

**Step 2: Run full verification**

Run:

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p rssh-app
```

Expected: all commands exit `0`.

**Step 3: Refresh package and smoke test**

Run the existing package command/checks used by the project for
`dist/R-SSH-windows-x64.zip`, then run packaged `version`, `doctor`,
`self-test`, `bench`, `profile`, `console`, and `rssh-console.cmd` smoke checks.

**Step 4: Commit and push**

```powershell
git add README.md docs crates dist
git commit -m "feat: expand xterm mouse protocols"
git push
```
