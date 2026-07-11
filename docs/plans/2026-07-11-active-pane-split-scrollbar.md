# Active Pane Split Scrollbar Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Keep one WezTerm-style window-right scrollbar visible in split layouts and bind it to the active pane.

**Architecture:** Reuse the existing window-level `ScrollbackScrollbar`, framebuffer overlay, and mouse-input path. Remove the split-layout suppression guard; pane focus already swaps the active runtime and preserves each pane's scrollback offset, so regression tests can verify the scrollbar follows focus without adding state.

**Tech Stack:** Rust, `rssh-app`, `rssh-renderer::ScrollbackScrollbar`, existing app-shell pane runtime and headless framebuffer tests.

---

### Task 1: Keep the enabled scrollbar visible in a split layout

**Files:**
- Modify: `crates/rssh-app/src/window.rs:91432-91451`
- Test: `crates/rssh-app/src/window.rs:152405-152440`

**Step 1: Write the failing split visibility test**

Add this test beside the existing scrollbar tests:

```rust
#[test]
fn window_app_renders_active_pane_scrollbar_with_split_layout() {
    let mut app = NativeWindowApp::new(None);
    app.set_config_overrides(NativeConfigOverrides {
        enable_scroll_bar: Some(true),
        ..NativeConfigOverrides::default()
    });
    app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
    app.dispatch_app_action(AppAction::SplitPane {
        pane: rssh_core::PaneId::new(1),
        direction: rssh_core::app_shell::SplitDirection::Right,
        launch: None,
    })
    .unwrap();
    app.handle_pty_output(b"aa\r\nbb\r\ncc\r\ndd\r\nee")
        .unwrap();
    app.scroll_viewport_lines(99);

    let scrollbar = app
        .scrollback_scrollbar()
        .expect("active pane scrollbar should remain visible in a split");
    assert_eq!(scrollbar.scrollback_offset, 3);

    let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];
    assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
    assert_eq!(
        frame_pixel_at(
            &frame,
            FRAME_WIDTH as usize,
            FRAME_WIDTH as usize - 1,
            tab_bar_pixel_height() as usize,
        ),
        SCROLLBAR_THUMB_COLOR
    );
}
```

**Step 2: Run the test to verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_renders_active_pane_scrollbar_with_split_layout -- --nocapture
```

Expected: FAIL at the `expect` because `scrollback_scrollbar()` returns `None`
when `has_visible_split_layout()` is true.

**Step 3: Remove only the split-layout suppression**

Change the guard in `NativeWindowApp::scrollback_scrollbar` to:

```rust
fn scrollback_scrollbar(&self) -> Option<ScrollbackScrollbar> {
    if !self.enable_scroll_bar {
        return None;
    }

    // Existing active-runtime geometry and style construction remains unchanged.
```

Do not create pane-local scrollbar state and do not change renderer geometry.

**Step 4: Run focused scrollbar tests to verify GREEN**

Run:

```powershell
cargo test -p rssh-app scrollback_scrollbar -- --nocapture
```

Expected: all matching tests PASS, including the new split test and the
disabled-by-default regression.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: keep active pane scrollbar visible in splits"
```

### Task 2: Prove focus tracking and input isolation

**Files:**
- Test: `crates/rssh-app/src/window.rs:152405-152645`

**Step 1: Add an active-pane focus regression test**

Create two panes with different history lengths and offsets. Capture each
pane's scrollbar value, switch focus away and back, and assert the complete
`ScrollbackScrollbar` value is restored:

```rust
#[test]
fn window_app_split_scrollbar_follows_active_pane_runtime() {
    let mut app = NativeWindowApp::new(None);
    app.set_config_overrides(NativeConfigOverrides {
        enable_scroll_bar: Some(true),
        ..NativeConfigOverrides::default()
    });
    app.runtime.resize(rssh_core::TerminalSize::new(4, 2));
    app.handle_pty_output(b"p1a\r\np1b\r\np1c\r\np1d\r\np1e")
        .unwrap();
    app.scroll_viewport_lines(2);
    let pane_one_scrollbar = app.scrollback_scrollbar().expect("pane one scrollbar");

    app.dispatch_app_action(AppAction::SplitPane {
        pane: rssh_core::PaneId::new(1),
        direction: rssh_core::app_shell::SplitDirection::Right,
        launch: None,
    })
    .unwrap();
    app.handle_pty_output(b"p2a\r\np2b\r\np2c\r\np2d\r\np2e\r\np2f\r\np2g")
        .unwrap();
    app.scroll_viewport_lines(3);
    let pane_two_scrollbar = app.scrollback_scrollbar().expect("pane two scrollbar");
    assert_ne!(pane_two_scrollbar, pane_one_scrollbar);

    app.dispatch_app_action(AppAction::ActivatePane {
        pane: rssh_core::PaneId::new(1),
    })
    .unwrap();
    assert_eq!(app.scrollback_scrollbar(), Some(pane_one_scrollbar));

    app.dispatch_app_action(AppAction::ActivatePane {
        pane: rssh_core::PaneId::new(2),
    })
    .unwrap();
    assert_eq!(app.scrollback_scrollbar(), Some(pane_two_scrollbar));
}
```

**Step 2: Run the focus test**

Run:

```powershell
cargo test -p rssh-app window_app_split_scrollbar_follows_active_pane_runtime -- --nocapture
```

Expected: PASS. If it fails, fix only the active-runtime save/install flow
needed to preserve `scrollback_offset`; do not add duplicate scrollbar state.

**Step 3: Add a mouse-input isolation regression test**

Starting from a two-pane app with scrollable active-pane history, record the
inactive pane runtime's offset from `app.pane_runtimes`. Click the window-right
scrollbar at the top and assert `app.scrollback_offset` changes while the
inactive stored offset does not:

```rust
let inactive_offset = app
    .pane_runtimes
    .get(&rssh_core::PaneId::new(1))
    .expect("inactive pane runtime")
    .scrollback_offset;
app.handle_cursor_moved(PhysicalPosition::new(
    f64::from(FRAME_WIDTH - 1),
    f64::from(tab_bar_pixel_height()),
))
.unwrap();
assert!(
    app.handle_mouse_input(ElementState::Pressed, MouseButton::Left)
        .unwrap()
);
assert!(app.scrollback_offset > 0);
assert_eq!(
    app.pane_runtimes
        .get(&rssh_core::PaneId::new(1))
        .expect("inactive pane runtime")
        .scrollback_offset,
    inactive_offset
);
assert!(
    app.handle_mouse_input(ElementState::Released, MouseButton::Left)
        .unwrap()
);
```

**Step 4: Run the split scrollbar tests**

Run:

```powershell
cargo test -p rssh-app split_scrollbar -- --nocapture
```

Expected: all split-scrollbar tests PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "test: cover active pane split scrollbar routing"
```

### Task 3: Record parity and verify the workspace

**Files:**
- Modify: `docs/research/wezterm-parity-gap.md:21`
- Modify: `docs/research/wezterm-parity-gap.md:4102-4109`
- Modify: `docs/research/wezterm-parity-gap.md:4275-4278`

**Step 1: Update the parity record**

State that enabled scrollbars remain visible in split layouts as a single
window-right scrollbar bound to the active pane, following its saved history
and offset. Explicitly retain per-pane scrollbar tracks as future/non-upstream
work and remove the now-stale generic split-scrollbar item from the final next
layer summary.

**Step 2: Format and inspect the diff**

Run:

```powershell
cargo fmt --all -- --check
git diff --check
git diff --stat
```

Expected: all commands exit 0; the diff is limited to `window.rs` tests/guard
and the parity document.

**Step 3: Run the app crate suite**

Run:

```powershell
cargo test -p rssh-app
```

Expected: all `rssh-app` tests PASS.

**Step 4: Run the full workspace suite**

Run:

```powershell
cargo test --workspace
```

Expected: all workspace tests PASS.

**Step 5: Commit**

```powershell
git add docs/research/wezterm-parity-gap.md
git commit -m "docs: record active pane split scrollbar parity"
```
