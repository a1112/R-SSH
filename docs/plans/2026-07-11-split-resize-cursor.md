# Split Resize Cursor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Show WezTerm-style horizontal and vertical native resize cursors while hovering or dragging pane split separators.

**Architecture:** Store the current winit cursor icon in `NativeWindowApp` beside cursor visibility. Derive it from the existing separator hit-test or active `PaneSplitResizeDrag`, then send changes through one stateful setter so headless tests and live windows share behavior.

**Tech Stack:** Rust, winit 0.30 `CursorIcon`, existing app-shell split layout and native-window tests.

---

### Task 1: Add cursor icon state and direction mapping

**Files:**
- Modify: `crates/rssh-app/src/window.rs:45-53`
- Modify: `crates/rssh-app/src/window.rs:77600-77620`
- Modify: `crates/rssh-app/src/window.rs:79150-79175`
- Modify: `crates/rssh-app/src/window.rs:86500-86520`
- Test: `crates/rssh-app/src/window.rs:174270-174340`

**Step 1: Write the failing direction/state tests**

Import `CursorIcon` from `winit::window`. Add a test that expects the WezTerm axis mapping:

```rust
#[test]
fn split_resize_cursor_icon_matches_wezterm_split_axes() {
    assert_eq!(
        split_resize_cursor_icon(SplitDirection::Left),
        CursorIcon::EwResize
    );
    assert_eq!(
        split_resize_cursor_icon(SplitDirection::Right),
        CursorIcon::EwResize
    );
    assert_eq!(
        split_resize_cursor_icon(SplitDirection::Up),
        CursorIcon::NsResize
    );
    assert_eq!(
        split_resize_cursor_icon(SplitDirection::Down),
        CursorIcon::NsResize
    );
}
```

Add a headless setter test:

```rust
#[test]
fn window_app_tracks_mouse_cursor_icon_without_native_window() {
    let mut app = NativeWindowApp::new(None);
    assert_eq!(app.mouse_cursor_icon, CursorIcon::Default);

    app.set_mouse_cursor_icon(CursorIcon::EwResize);

    assert_eq!(app.mouse_cursor_icon, CursorIcon::EwResize);
}
```

**Step 2: Run the tests and verify RED**

Run:

```powershell
cargo test -p rssh-app split_resize_cursor_icon_matches_wezterm_split_axes -- --nocapture
```

Expected: compile failure because `split_resize_cursor_icon` and cursor icon state do not exist.

**Step 3: Add the minimal state and helpers**

Add `CursorIcon` to the winit window imports and add this field beside `mouse_cursor_visible`:

```rust
mouse_cursor_icon: CursorIcon,
```

Initialize it with `CursorIcon::Default`. Add:

```rust
fn split_resize_cursor_icon(direction: SplitDirection) -> CursorIcon {
    match direction {
        SplitDirection::Left | SplitDirection::Right => CursorIcon::EwResize,
        SplitDirection::Up | SplitDirection::Down => CursorIcon::NsResize,
    }
}
```

Add the stateful setter beside `set_mouse_cursor_visible`:

```rust
fn set_mouse_cursor_icon(&mut self, icon: CursorIcon) {
    if self.mouse_cursor_icon == icon {
        return;
    }

    self.mouse_cursor_icon = icon;
    if let Some(window) = &self.window {
        window.set_cursor(icon);
    }
}
```

**Step 4: Run the tests and verify GREEN**

Run both new tests. Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: track split resize cursor icons"
```

### Task 2: Integrate hover and drag cursor lifecycle

**Files:**
- Modify: `crates/rssh-app/src/window.rs:86395-86610`
- Test: `crates/rssh-app/src/window.rs:174290-174340`

**Step 1: Write failing lifecycle tests**

Add a right-split hover test that creates a right split, moves to column 39 on the separator, expects `EwResize`, moves to column 0, and expects `Default`.

Add a down-split hover test. Read the separator from `app.pane_render_layout().separators[0]`, convert its render row and column to pixel coordinates, move there, and expect `NsResize`.

Extend the existing drag test so it asserts:

```rust
assert_eq!(app.mouse_cursor_icon, CursorIcon::EwResize);
```

after pressing; move far beyond the maximum split position and assert the icon remains `EwResize`; release and assert it recomputes to `Default`. Add a cursor-left assertion that hovering a separator followed by `handle_cursor_left()` restores `Default`.

**Step 2: Run lifecycle tests and verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_uses_resize_cursor_for_split_separator -- --nocapture
cargo test -p rssh-app window_app_dragging_right_split_separator_resizes_panes -- --nocapture
```

Expected: FAIL because cursor movement and drag lifecycle do not update the icon.

**Step 3: Implement cursor recomputation**

Add:

```rust
fn update_split_resize_cursor_icon(&mut self) {
    let drag = self
        .split_resize_dragging
        .or_else(|| self.split_resize_drag_at_mouse_position());
    let icon = drag
        .map(|drag| split_resize_cursor_icon(drag.direction))
        .unwrap_or(CursorIcon::Default);
    self.set_mouse_cursor_icon(icon);
}
```

Call it in `handle_cursor_moved` after updating mouse coordinates and before early returns for scrollbar or split dragging. When a split drag starts or ends, call it after updating `split_resize_dragging`. In `handle_cursor_left`, reset the icon to `CursorIcon::Default` after clearing coordinates.

Do not change resize geometry, selection routing, mouse reporting, or cursor visibility semantics.

**Step 4: Run lifecycle and regression tests**

Run the new hover tests, the existing drag test, and mouse cursor visibility tests. Expected: all pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: show split resize cursor affordance"
```

### Task 3: Document and verify the slice

**Files:**
- Modify: `docs/research/wezterm-parity-gap.md:4130-4140`

**Step 1: Update the parity record**

Extend the split-drag bullet to state that separator hover and active dragging use WezTerm-style horizontal/vertical native resize cursors, including drag retention and restoration on release/leave.

**Step 2: Run final verification**

Run:

```powershell
cargo fmt --all -- --check
cargo test --workspace
git diff --check
```

Expected: all commands exit 0.

**Step 3: Commit documentation**

```powershell
git add docs/research/wezterm-parity-gap.md
git commit -m "docs: record split resize cursor parity"
```
