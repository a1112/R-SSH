# WezTerm Multi-Window Focus and Lifecycle Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make native multi-window focus ownership, focus side effects, new-window activation, and macOS application hiding match the pinned WezTerm behavior.

**Architecture:** Add a small generic focus coordinator to `NativeWindowManager`, keep Lua/status/PTY effects in an idempotent `NativeWindowApp` transition, and route `WindowEvent::Focused` through the manager. Materialization requests platform focus only for the intended foreground window, while `HideApplication` is consumed by the manager and calls winit's real macOS application API.

**Tech Stack:** Rust 2024, winit 0.30.13, existing `NativeWindowManager`/`NativeWindowApp`, pinned WezTerm commit `093bf6bf`, Cargo test, rustfmt.

---

### Task 1: Make app-local focus transitions start from OS truth and become idempotent

**Files:**
- Modify: `crates/rssh-app/src/window.rs:80600-80620`
- Modify: `crates/rssh-app/src/window.rs:82140-82170`
- Modify: `crates/rssh-app/src/window.rs:97980-98015`
- Test: `crates/rssh-app/src/window.rs:125880-125950`

**Step 1: Add RED tests for initial state and duplicate suppression**

Add tests next to `window_app_dispatches_focus_changed_for_active_pane`:

```rust
#[test]
fn window_app_starts_unfocused_until_the_os_reports_focus() {
    let app = NativeWindowApp::new(None);
    assert!(!app.window_focused);
    assert!(!app.mouse_click_may_focus_window);
}

#[test]
fn window_app_focus_changes_are_idempotent() {
    let changes = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&changes);
    let mut app = NativeWindowApp::new(None);
    app.focus_change_handler = Box::new(move |change| {
        recorded.lock().unwrap().push(*change);
        true
    });

    assert!(app.handle_focus_changed(true).unwrap());
    assert!(!app.handle_focus_changed(true).unwrap());
    assert!(app.handle_focus_changed(false).unwrap());
    assert!(!app.handle_focus_changed(false).unwrap());

    assert_eq!(
        changes
            .lock()
            .unwrap()
            .iter()
            .map(|change| change.focused)
            .collect::<Vec<_>>(),
        [true, false]
    );
}
```

Update the existing focus test to assert the boolean transition result.

**Step 2: Run the tests to verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_starts_unfocused_until_the_os_reports_focus -- --exact --nocapture
cargo test -p rssh-app window_app_focus_changes_are_idempotent -- --exact --nocapture
```

Expected: the initial-state test fails because `window_focused` is currently
`true`; the idempotence test fails to compile because the transition currently
returns `io::Result<()>`.

**Step 3: Implement the minimal idempotent transition**

Initialize `window_focused` to `false` in `NativeWindowApp::new...`.

Change the transition signature and add the state guard:

```rust
fn handle_focus_changed(&mut self, focused: bool) -> io::Result<bool> {
    if self.window_focused == focused {
        return Ok(false);
    }

    if focused {
        self.mouse_click_may_focus_window = true;
        self.window_focused = true;
    } else {
        self.window_focused = false;
        self.mouse_click_may_focus_window = false;
    }

    let change = NativeWindowFocusChange {
        window_id: self.app_window_id,
        pane: self.app_shell.active_pane_id(),
        focused,
    };
    self.dispatch_focus_change(&change);
    self.dispatch_update_status();

    if let Some(bytes) = encode_window_focus_event(focused, self.runtime.focus_reporting()) {
        self.write_pty_bytes(&bytes)?;
    }

    Ok(true)
}
```

At existing call sites that only care about errors, keep `?`/error handling and
ignore the returned boolean explicitly.

**Step 4: Run the focused tests to verify GREEN**

Run:

```powershell
cargo test -p rssh-app window_app_starts_unfocused_until_the_os_reports_focus -- --exact --nocapture
cargo test -p rssh-app window_app_focus_changes_are_idempotent -- --exact --nocapture
cargo test -p rssh-app window_app_dispatches_focus_changed_for_active_pane -- --exact --nocapture
cargo test -p rssh-app window_app_parses_static_wezterm_focus_changed_status_setter -- --exact --nocapture
```

Expected: all pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "fix: make window focus transitions idempotent"
```

### Task 2: Prove terminal focus reporting is emitted exactly once

**Files:**
- Test: `crates/rssh-app/src/window.rs:125850-125950`
- Modify: `crates/rssh-app/src/window.rs:97980-98015` only if the RED test exposes a defect

**Step 1: Add the RED regression test**

Use the existing `SharedWriter` test helper:

```rust
#[test]
fn window_app_focus_reporting_suppresses_duplicate_sequences() {
    let mut app = NativeWindowApp::new(None);
    let written = Arc::new(Mutex::new(Vec::new()));
    app.writer = Some(Box::new(SharedWriter(Arc::clone(&written))));
    app.runtime.feed_pty_output(b"\x1b[?1004h");

    assert!(app.handle_focus_changed(true).unwrap());
    assert!(!app.handle_focus_changed(true).unwrap());
    assert!(app.handle_focus_changed(false).unwrap());
    assert!(!app.handle_focus_changed(false).unwrap());

    assert_eq!(written.lock().unwrap().as_slice(), b"\x1b[I\x1b[O");
}
```

**Step 2: Run the test to verify RED or immediate GREEN from Task 1**

Run:

```powershell
cargo test -p rssh-app window_app_focus_reporting_suppresses_duplicate_sequences -- --exact --nocapture
```

Expected: GREEN if Task 1 correctly guarded all side effects. If it fails,
retain the RED output as evidence and move the guard before every side effect.

**Step 3: Run the focus/input regression cluster**

Run:

```powershell
cargo test -p rssh-app focus_changed
cargo test -p rssh-app focus_reporting
cargo test -p rssh-app swallow_mouse_click_on_window_focus
cargo test -p rssh-app notification_handling
```

Expected: all pass.

**Step 4: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "test: lock exact window focus reporting"
```

### Task 3: Add a pure cross-window focus coordinator

**Files:**
- Modify: `crates/rssh-app/src/window.rs:80520-80540`
- Modify: `crates/rssh-app/src/window.rs:80955-80980`
- Test: `crates/rssh-app/src/window.rs:195390-195430`

**Step 1: Add RED coordinator tests**

Define tests using integer ids so no native OS window is required:

```rust
#[test]
fn window_focus_coordinator_transfers_exclusive_focus() {
    let mut focus = WindowFocusCoordinator::default();

    assert_eq!(
        focus.apply(10_u64, true),
        WindowFocusTransitions {
            blur: None,
            focus: Some(10),
        }
    );
    assert_eq!(focus.focused(), Some(10));
    assert_eq!(focus.apply(10, true), WindowFocusTransitions::default());
    assert_eq!(
        focus.apply(20, true),
        WindowFocusTransitions {
            blur: Some(10),
            focus: Some(20),
        }
    );
    assert_eq!(focus.focused(), Some(20));
    assert_eq!(focus.apply(10, false), WindowFocusTransitions::default());
    assert_eq!(
        focus.apply(20, false),
        WindowFocusTransitions {
            blur: Some(20),
            focus: None,
        }
    );
    assert_eq!(focus.focused(), None);
}

#[test]
fn window_focus_coordinator_forgets_removed_focus_owner() {
    let mut focus = WindowFocusCoordinator::default();
    focus.apply(10_u64, true);

    assert!(!focus.remove(20));
    assert!(focus.remove(10));
    assert_eq!(focus.focused(), None);
}
```

**Step 2: Run the tests to verify RED**

Run:

```powershell
cargo test -p rssh-app window_focus_coordinator -- --nocapture
```

Expected: FAIL to compile because the coordinator types do not exist.

**Step 3: Implement the generic coordinator**

Add near `WindowActivateWindowRequest`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowFocusTransitions<Id> {
    blur: Option<Id>,
    focus: Option<Id>,
}

impl<Id> Default for WindowFocusTransitions<Id> {
    fn default() -> Self {
        Self {
            blur: None,
            focus: None,
        }
    }
}

#[derive(Debug)]
struct WindowFocusCoordinator<Id> {
    focused: Option<Id>,
}

impl<Id> Default for WindowFocusCoordinator<Id> {
    fn default() -> Self {
        Self { focused: None }
    }
}

impl<Id: Copy + Eq> WindowFocusCoordinator<Id> {
    const fn focused(&self) -> Option<Id> {
        self.focused
    }

    fn apply(&mut self, id: Id, focused: bool) -> WindowFocusTransitions<Id> {
        if focused {
            if self.focused == Some(id) {
                return WindowFocusTransitions::default();
            }
            let blur = self.focused.replace(id);
            return WindowFocusTransitions {
                blur,
                focus: Some(id),
            };
        }

        if self.focused == Some(id) {
            self.focused = None;
            return WindowFocusTransitions {
                blur: Some(id),
                focus: None,
            };
        }

        WindowFocusTransitions::default()
    }

    fn remove(&mut self, id: Id) -> bool {
        if self.focused != Some(id) {
            return false;
        }
        self.focused = None;
        true
    }
}
```

Add `focus: WindowFocusCoordinator<winit::window::WindowId>` to
`NativeWindowManager` and initialize it with `default()`.

**Step 4: Run the coordinator tests to verify GREEN**

Run:

```powershell
cargo test -p rssh-app window_focus_coordinator -- --nocapture
```

Expected: both pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: coordinate multi-window focus ownership"
```

### Task 4: Route OS focus events through the manager and clear stale ownership

**Files:**
- Modify: `crates/rssh-app/src/window.rs:81095-81130`
- Modify: `crates/rssh-app/src/window.rs:123535-123590`
- Modify: `crates/rssh-app/src/window.rs:123705-123725`
- Test: `crates/rssh-app/src/window.rs:125880-125980`

**Step 1: Add a RED manager transition test around an extracted dispatcher**

Add a manager helper that can be exercised with the existing app instances by
logical id in tests. The test should install focus handlers on two apps, submit
A true, B true, A false, B false, and assert this exact event sequence:

```rust
[
    (WindowId::new(1), true),
    (WindowId::new(1), false),
    (WindowId::new(2), true),
    (WindowId::new(2), false),
]
```

Because constructing winit window ids is platform-private, split the logic:

```rust
fn apply_focus_transitions<Id: Copy + Eq>(
    focus: &mut WindowFocusCoordinator<Id>,
    id: Id,
    focused: bool,
) -> WindowFocusTransitions<Id> {
    focus.apply(id, focused)
}
```

Test the transition sequence through this helper and keep app-local side-effect
tests in Tasks 1 and 2. Do not add unsafe fake winit ids.

**Step 2: Run the manager focus tests to verify RED**

Run:

```powershell
cargo test -p rssh-app window_manager_focus -- --nocapture
```

Expected: FAIL until the dispatcher/helper and manager integration exist.

**Step 3: Implement manager routing**

Add a method shaped like:

```rust
fn handle_window_focus_changed(
    &mut self,
    window_id: winit::window::WindowId,
    focused: bool,
) -> io::Result<()> {
    let transitions = self.focus.apply(window_id, focused);

    if let Some(blur) = transitions.blur {
        if let Some(app) = self.windows.get_mut(&blur) {
            let _ = app.handle_focus_changed(false)?;
            if let Some(window) = &app.window {
                window.request_redraw();
            }
        }
    }
    if let Some(focus) = transitions.focus {
        if let Some(app) = self.windows.get_mut(&focus) {
            let _ = app.handle_focus_changed(true)?;
            if let Some(window) = &app.window {
                window.request_redraw();
            }
        }
    }
    Ok(())
}
```

Special-case `WindowEvent::Focused(focused)` in
`NativeWindowManager::window_event` before removing the target app from the
map. Log the existing `PTY focus error` and exit on error. Remove the
`WindowEvent::Focused` branch from `NativeWindowApp::window_event` so focus is
not dispatched twice.

Call `self.focus.remove(window_id)` in every path that permanently removes a
native window: close request, PTY-driven close, explicit window-close request,
and application quit/clear.

**Step 4: Run focused manager/lifecycle tests**

Run:

```powershell
cargo test -p rssh-app window_manager_focus -- --nocapture
cargo test -p rssh-app window_app_focus -- --nocapture
cargo test -p rssh-app window_manager_collects_detached_app
cargo test -p rssh-app window_manager_quit_application
cargo test -p rssh-app window_manager_can_keep_running_after_last_window_closes
```

Expected: all pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: route native focus through window manager"
```

### Task 5: Focus only the intended newly materialized window

**Files:**
- Modify: `crates/rssh-app/src/window.rs:80995-81045`
- Test: `crates/rssh-app/src/window.rs:81125-81200`
- Test: `crates/rssh-app/src/window.rs:161650-162250`

**Step 1: Add RED policy tests**

Extract the queue rule into a pure helper and test it:

```rust
#[test]
fn pending_window_batch_focuses_only_the_last_materialized_window() {
    assert!(!should_focus_materialized_window(0, 3));
    assert!(!should_focus_materialized_window(1, 3));
    assert!(should_focus_materialized_window(2, 3));
    assert!(!should_focus_materialized_window(0, 0));
}
```

Use this implementation contract:

```rust
const fn should_focus_materialized_window(index: usize, len: usize) -> bool {
    len > 0 && index + 1 == len
}
```

Also extend existing pending-window manager tests to assert that collecting
pending apps preserves queue order by logical `app_window_id`.

**Step 2: Run the tests to verify RED**

Run:

```powershell
cargo test -p rssh-app pending_window_batch_focuses_only_the_last_materialized_window -- --exact --nocapture
cargo test -p rssh-app window_manager_collects_detached_app -- --nocapture
```

Expected: the new policy test fails to compile.

**Step 3: Refactor materialization to request activation explicitly**

Change `materialize_app` to return the created winit window id and not focus by
itself:

```rust
fn materialize_app(
    &mut self,
    event_loop: &ActiveEventLoop,
    mut app: NativeWindowApp,
) -> Result<winit::window::WindowId, Box<dyn Error>> {
    app.create_window(event_loop)?;
    app.spawn_pty()?;
    let window_id = app
        .window_id()
        .ok_or_else(|| io::Error::other("window was not created"))?;
    if let Some(window) = &app.window {
        window.request_redraw();
    }
    self.windows.insert(window_id, app);
    Ok(window_id)
}
```

Add one activation helper:

```rust
fn request_window_focus(&self, window_id: winit::window::WindowId) -> bool {
    let Some(window) = self.windows.get(&window_id).and_then(|app| app.window.as_ref()) else {
        return false;
    };
    window.set_visible(true);
    window.set_minimized(false);
    window.focus_window();
    true
}
```

Use it after startup materialization. In pending materialization, collect the
successful ids and call it only for the final id. Do not mutate the focus
coordinator until `Focused(true)` arrives.

Reuse `request_window_focus` from `activate_window_relative_from` to keep all
show/unminimize/focus behavior identical.

**Step 4: Run new-window and activation regressions**

Run:

```powershell
cargo test -p rssh-app pending_window_batch_focuses_only_the_last_materialized_window -- --exact --nocapture
cargo test -p rssh-app consumes_pending_new_window
cargo test -p rssh-app window_manager_collects_detached_app
cargo test -p rssh-app move_to_new_window
cargo test -p rssh-app activate_window
```

Expected: all pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: activate the latest materialized window"
```

### Task 6: Route HideApplication to the native macOS application API

**Files:**
- Modify: `crates/rssh-app/src/window.rs:1-60`
- Modify: `crates/rssh-app/src/window.rs:82570-82595`
- Modify: `crates/rssh-app/src/window.rs:82865-82900`
- Modify: `crates/rssh-app/src/window.rs:123545-123590`
- Test: `crates/rssh-app/src/window.rs:196540-196580`
- Test: `crates/rssh-app/src/window.rs:221440-221485`

**Step 1: Add RED request-lifecycle tests**

Replace the current persistent-flag-only assertion with exact consumption:

```rust
#[test]
fn window_app_hide_application_request_is_consumed_once() {
    let mut app = NativeWindowApp::new(None);

    app.hide_application();

    assert!(app.take_application_hide_request());
    assert!(!app.take_application_hide_request());
    assert!(!app.window_hide_requested);
}
```

Keep the existing command-palette and shortcut tests, but assert the request is
consumable rather than permanently true.

**Step 2: Run tests to verify RED**

Run:

```powershell
cargo test -p rssh-app hide_application -- --nocapture
```

Expected: FAIL because no take method exists and `hide_application` currently
minimizes the current window.

**Step 3: Implement app request semantics and platform dispatch**

Change the app methods:

```rust
fn hide_application(&mut self) {
    self.application_hide_requested = true;
}

fn take_application_hide_request(&mut self) -> bool {
    std::mem::take(&mut self.application_hide_requested)
}
```

Add platform helpers:

```rust
#[cfg(target_os = "macos")]
fn hide_native_application(event_loop: &ActiveEventLoop) {
    use winit::platform::macos::ActiveEventLoopExtMacOS;
    event_loop.hide_application();
}

#[cfg(not(target_os = "macos"))]
fn hide_native_application(_event_loop: &ActiveEventLoop) {}
```

In `NativeWindowManager::window_event`, take the request after app dispatch,
reinsert the app normally, and call `hide_native_application(event_loop)` once.
Do not synthesize focus changes; macOS focus events drive them.

**Step 4: Run native tests and cross-target check**

Run:

```powershell
cargo test -p rssh-app hide_application -- --nocapture
rustup target list --installed
```

If `x86_64-apple-darwin` is absent, install the Rust standard library target:

```powershell
rustup target add x86_64-apple-darwin
```

Then run:

```powershell
cargo check -p rssh-app --target x86_64-apple-darwin
```

Expected: tests pass and the macOS-only winit API type-checks. If a native
third-party dependency prevents cross-target checking on Windows, record the
exact dependency error, verify the locked winit method signature directly, and
do not weaken the macOS implementation.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: hide the native macOS application"
```

### Task 7: Record the completed App Shell v2 focus slice

**Files:**
- Modify: `docs/architecture.md:1160-1220`
- Modify: `docs/mvp-6-app-shell-v1.md:2330-2375`
- Modify: `docs/mvp-6-app-shell-v1.md:2500-2530`
- Modify: `docs/research/wezterm-parity-gap.md:4300-4330`

**Step 1: Update architecture and milestone language**

Document only behavior proven by tests:

- manager-owned exclusive focus state;
- OS-truth initial focus;
- duplicate suppression across Lua/status/PTY effects;
- latest pending window activation request;
- shared `ActivateWindow*` show/unminimize/focus path;
- stale focus cleanup on window removal;
- real macOS `HideApplication` dispatch.

Keep broader pane focus visuals, selection polish, mux/window registry,
arbitrary Lua callbacks, and external CLI tab-title control open.

**Step 2: Check documentation diff**

Run:

```powershell
git diff --check
git diff -- docs/architecture.md docs/mvp-6-app-shell-v1.md docs/research/wezterm-parity-gap.md
```

Expected: no whitespace errors and no broad App Shell v2 completion claim.

**Step 3: Commit**

```powershell
git add docs/architecture.md docs/mvp-6-app-shell-v1.md docs/research/wezterm-parity-gap.md
git commit -m "docs: record multi-window focus lifecycle parity"
```

### Task 8: Run final verification and finish the branch

**Files:**
- Verify: `crates/rssh-app/src/window.rs`
- Verify: `docs/plans/2026-07-17-wezterm-multi-window-focus-lifecycle-design.md`
- Verify: `docs/plans/2026-07-17-wezterm-multi-window-focus-lifecycle.md`

**Step 1: Run focused behavior gates**

Run:

```powershell
$env:RUST_TEST_THREADS='1'
$env:CARGO_BUILD_JOBS='1'
cargo test -p rssh-app focus_changed
cargo test -p rssh-app focus_reporting
cargo test -p rssh-app window_focus_coordinator
cargo test -p rssh-app activate_window
cargo test -p rssh-app move_to_new_window
cargo test -p rssh-app hide_application
cargo test -p rssh-app notification_handling
```

Expected: all pass.

**Step 2: Run full app and workspace gates**

Run:

```powershell
$env:RUST_TEST_THREADS='1'
$env:CARGO_BUILD_JOBS='1'
cargo test -p rssh-app
cargo test --workspace
```

Expected: all tests pass; only the two existing real-PTY integration tests may
remain ignored.

**Step 3: Run formatting and repository checks**

Run:

```powershell
cargo fmt --all -- --check
git diff --check
git status --short --branch
git log --oneline codex/wezterm-parity-progress..HEAD
```

Expected: formatting/diff checks exit 0 and the feature worktree is clean.

**Step 4: Review requirements against the design**

Re-read
`docs/plans/2026-07-17-wezterm-multi-window-focus-lifecycle-design.md` and map
each of its seven completion criteria to fresh command output or an exact test.
Do not claim general WezTerm or full App Shell v2 parity.

**Step 5: Finish the development branch**

Invoke `superpowers:verification-before-completion`, then
`superpowers:finishing-a-development-branch`. The user has historically chosen
local merge into `codex/wezterm-parity-progress`, but obtain or honor the
current explicit integration choice rather than assuming push/PR authority.
