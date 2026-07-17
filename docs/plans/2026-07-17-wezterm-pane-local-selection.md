# WezTerm Pane-Local Selection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make ordinary terminal selections independently owned, restored, rendered, cleared, copied, and retired per pane in the same GUI-window boundary as pinned WezTerm commit `093bf6b`.

**Architecture:** Extend `PaneRuntime` with the inactive pane's ordinary selection and move selection through the existing active-runtime swap boundary. Keep stored snapshots selection-free, overlay inactive selections during split composition before `inactive_pane_hsb`, and retain the existing active selection rebuild path. Treat mouse dragging as window-local transient state and clear selection when a pane crosses into a new GUI window.

**Tech Stack:** Rust 2024, `rssh-app` native window/runtime layer, `TerminalRenderSnapshot`, pinned WezTerm source at `E:\project\R-SSH\refs\wezterm`, Cargo test, rustfmt.

---

### Task 1: Give each pane authoritative ordinary-selection state

**Files:**
- Modify: `crates/rssh-app/src/window.rs:80740-80765`
- Modify: `crates/rssh-app/src/window.rs:81360-81410`
- Modify: `crates/rssh-app/src/window.rs:84195-84345`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write the failing pane-switch ownership tests**

Add tests beside the split-runtime tests:

```rust
#[test]
fn window_app_restores_independent_selection_for_each_pane() {
    let mut app = NativeWindowApp::new(None);
    app.runtime.resize(rssh_core::TerminalSize::new(12, 2));
    app.handle_pty_output(b"left").unwrap();
    app.selection = Some(WindowSelection::new(
        SelectionCell { row: 0, column: 0 },
        SelectionCell { row: 0, column: 3 },
    ));
    app.refresh_snapshot();

    app.dispatch_app_action(AppAction::SplitPane {
        pane: rssh_core::PaneId::new(1),
        direction: SplitDirection::Right,
        launch: None,
    })
    .unwrap();
    assert!(app.selection.is_none());

    app.handle_pty_output(b"right").unwrap();
    app.selection = Some(WindowSelection::new(
        SelectionCell { row: 0, column: 0 },
        SelectionCell { row: 0, column: 4 },
    ));
    app.refresh_snapshot();
    assert_eq!(app.selected_text().as_deref(), Some("right"));

    app.dispatch_app_action(AppAction::ActivatePane {
        pane: rssh_core::PaneId::new(1),
    })
    .unwrap();
    assert_eq!(app.selected_text().as_deref(), Some("left"));

    app.dispatch_app_action(AppAction::ActivatePane {
        pane: rssh_core::PaneId::new(2),
    })
    .unwrap();
    assert_eq!(app.selected_text().as_deref(), Some("right"));
}

#[test]
fn window_app_switching_panes_ends_drag_but_preserves_source_selection() {
    let mut app = NativeWindowApp::new(None);
    app.selection = Some(WindowSelection::new(
        SelectionCell { row: 0, column: 0 },
        SelectionCell { row: 0, column: 1 },
    ));
    app.selecting = true;

    app.dispatch_app_action(AppAction::SplitPane {
        pane: rssh_core::PaneId::new(1),
        direction: SplitDirection::Right,
        launch: None,
    })
    .unwrap();

    assert!(!app.selecting);
    assert_eq!(
        app.pane_runtimes
            .get(&rssh_core::PaneId::new(1))
            .and_then(|runtime| runtime.selection),
        Some(WindowSelection::new(
            SelectionCell { row: 0, column: 0 },
            SelectionCell { row: 0, column: 1 },
        ))
    );
}

#[test]
fn window_app_pane_switch_does_not_persist_copy_mode_selection_as_ordinary_selection() {
    let mut app = NativeWindowApp::new(None);
    app.enter_copy_mode();
    app.selection = Some(WindowSelection::new(
        SelectionCell { row: 0, column: 0 },
        SelectionCell { row: 0, column: 0 },
    ));
    assert!(app.copy_mode.is_some());

    app.dispatch_app_action(AppAction::SplitPane {
        pane: rssh_core::PaneId::new(1),
        direction: SplitDirection::Right,
        launch: None,
    })
    .unwrap();

    assert!(app.copy_mode.is_none());
    assert!(app.search.is_none());
    assert!(app.quick_select.is_none());
    assert!(
        app.pane_runtimes
            .get(&rssh_core::PaneId::new(1))
            .is_some_and(|runtime| runtime.selection.is_none())
    );
}
```

**Step 2: Run the tests to verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_restores_independent_selection_for_each_pane -- --nocapture
cargo test -p rssh-app window_app_switching_panes_ends_drag_but_preserves_source_selection -- --nocapture
cargo test -p rssh-app window_app_pane_switch_does_not_persist_copy_mode_selection_as_ordinary_selection -- --nocapture
```

Expected:

- the first test fails because the window-wide selection appears in pane 2 or
  is not restored for pane 1;
- the second test fails to compile because `PaneRuntime::selection` does not
  exist.
- the third test fails because copy mode remains active or its transient
  selection is stored in the outgoing runtime.

Use the unique function-name filters without libtest `--exact`; Rust's actual
test names include the `window::tests::` module prefix. Each command must report
one test selected and failed, not `0 tests`.

**Step 3: Add selection to `PaneRuntime`**

Add:

```rust
struct PaneRuntime {
    runtime: TerminalRuntime,
    session: Option<PtySession>,
    session_process_id: Option<u32>,
    session_tty_name: Option<String>,
    writer: Option<Box<dyn Write + Send>>,
    reader_thread: Option<thread::JoinHandle<()>>,
    snapshot: TerminalRenderSnapshot,
    scrollback_offset: usize,
    selection: Option<WindowSelection>,
}
```

Initialize `selection: None` in every new `PaneRuntime`.

**Step 4: Move selection through the active-runtime boundary**

In `take_active_runtime`, take selection and end any drag:

```rust
let selection = self.selection.take();
self.selecting = false;
```

Include `selection` in the returned `PaneRuntime`.

Before taking an outgoing runtime, end selection-owning transient controllers:

```rust
fn end_transient_selection_modes_for_pane_change(&mut self) {
    if self.search.is_some() || self.copy_mode.is_some() || self.quick_select.is_some() {
        self.search = None;
        self.copy_mode = None;
        self.quick_select = None;
        self.selection = None;
    }
    self.selecting = false;
}
```

Call this only when `previous_active_pane != active_pane`, before
`take_active_runtime`. This prevents a search/copy-mode/quick-select highlight
from being promoted into persistent ordinary selection for the outgoing pane.
Pane-select already clears selection when it starts and exits through its
existing caller path.

In `install_active_runtime`, restore the selection without restoring a drag:

```rust
self.selection = runtime.selection.take();
self.selecting = false;
self.rebuild_snapshot();
```

Call `rebuild_snapshot` only after `self.runtime`, `self.snapshot`, and
`self.scrollback_offset` have been installed so selected text and colors are
computed against the destination pane.

**Step 5: Run focused and nearby runtime tests**

Run:

```powershell
cargo test -p rssh-app window_app_restores_independent_selection_for_each_pane -- --nocapture
cargo test -p rssh-app window_app_switching_panes_ends_drag_but_preserves_source_selection -- --nocapture
cargo test -p rssh-app window_app_pane_switch_does_not_persist_copy_mode_selection_as_ordinary_selection -- --nocapture
cargo test -p rssh-app split_scrollbar -- --nocapture
cargo test -p rssh-app pane_focus -- --nocapture
```

Expected: all tests pass.

Require each unique function-name command above to report `1 passed`.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: retain ordinary selection per pane"
```

### Task 2: Render inactive pane selections from base snapshots

**Files:**
- Modify: `crates/rssh-app/src/window.rs:88945-89020`
- Modify: `crates/rssh-app/src/window.rs:89070-89140`
- Modify: `crates/rssh-app/src/window.rs:90075-90165`
- Modify: `crates/rssh-app/src/window.rs:119620-119680`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write failing simultaneous-render and inactive-output tests**

Add:

```rust
#[test]
fn window_app_renders_active_and_inactive_pane_selections_together() {
    let mut app = NativeWindowApp::new(None);
    app.set_config_overrides(NativeConfigOverrides {
        selection_bg_color: Some(Color::Rgb(100, 120, 140)),
        inactive_pane_hsb: Some(NativeInactivePaneHsb {
            hue: NativeHsbMultiplier::from_f32(1.0),
            saturation: NativeHsbMultiplier::from_f32(1.0),
            brightness: NativeHsbMultiplier::from_f32(0.5),
        }),
        ..NativeConfigOverrides::default()
    });
    app.runtime.resize(rssh_core::TerminalSize::new(20, 4));
    app.handle_pty_output(b"A").unwrap();
    app.selection = Some(WindowSelection::new(
        SelectionCell { row: 0, column: 0 },
        SelectionCell { row: 0, column: 0 },
    ));
    app.refresh_snapshot();

    app.dispatch_app_action(AppAction::SplitPane {
        pane: rssh_core::PaneId::new(1),
        direction: SplitDirection::Right,
        launch: None,
    })
    .unwrap();
    app.handle_pty_output(b"B").unwrap();
    app.selection = Some(WindowSelection::new(
        SelectionCell { row: 0, column: 0 },
        SelectionCell { row: 0, column: 0 },
    ));
    app.refresh_snapshot();

    let layout = app.pane_render_layout();
    let inactive = layout
        .panes
        .iter()
        .find(|rect| rect.pane_id == rssh_core::PaneId::new(1))
        .unwrap();
    let active = layout
        .panes
        .iter()
        .find(|rect| rect.pane_id == rssh_core::PaneId::new(2))
        .unwrap();
    let snapshot = app.render_snapshot();

    assert_eq!(
        snapshot_cell(&snapshot, inactive.row, inactive.column)
            .unwrap()
            .background,
        Color::Rgb(50, 60, 70)
    );
    assert_eq!(
        snapshot_cell(&snapshot, active.row, active.column)
            .unwrap()
            .background,
        Color::Rgb(100, 120, 140)
    );
}

#[test]
fn window_app_keeps_inactive_selection_after_inactive_pty_output() {
    let mut app = NativeWindowApp::new(None);
    app.set_config_overrides(NativeConfigOverrides {
        selection_bg_color: Some(Color::Rgb(90, 110, 130)),
        ..NativeConfigOverrides::default()
    });
    app.runtime.resize(rssh_core::TerminalSize::new(20, 4));
    app.handle_pty_output(b"A").unwrap();
    app.selection = Some(WindowSelection::new(
        SelectionCell { row: 0, column: 0 },
        SelectionCell { row: 0, column: 0 },
    ));
    app.refresh_snapshot();
    app.dispatch_app_action(AppAction::SplitPane {
        pane: rssh_core::PaneId::new(1),
        direction: SplitDirection::Right,
        launch: None,
    })
    .unwrap();

    app.handle_pane_pty_output(rssh_core::PaneId::new(1), b"\rZ")
        .unwrap();

    let inactive = app
        .pane_render_layout()
        .panes
        .into_iter()
        .find(|rect| rect.pane_id == rssh_core::PaneId::new(1))
        .unwrap();
    let snapshot = app.render_snapshot();
    assert_eq!(
        snapshot_cell(&snapshot, inactive.row, inactive.column)
            .unwrap()
            .background,
        Color::Rgb(90, 110, 130)
    );
}

#[test]
fn window_app_single_pane_applies_translucent_selection_once() {
    let mut app = NativeWindowApp::new(None);
    app.set_config_overrides(NativeConfigOverrides {
        selection_bg_color: Some(Color::Rgba(100, 120, 140, 128)),
        ..NativeConfigOverrides::default()
    });
    app.handle_pty_output(b"\x1b[48;2;20;40;60mA").unwrap();
    app.selection = Some(WindowSelection::new(
        SelectionCell { row: 0, column: 0 },
        SelectionCell { row: 0, column: 0 },
    ));
    app.refresh_snapshot();

    let snapshot = app.render_snapshot();
    assert_eq!(
        snapshot_cell(&snapshot, TAB_BAR_ROWS, 0)
            .unwrap()
            .background,
        Color::Rgb(60, 80, 100)
    );
}
```

**Step 2: Run the tests to verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_renders_active_and_inactive_pane_selections_together -- --nocapture
cargo test -p rssh-app window_app_keeps_inactive_selection_after_inactive_pty_output -- --nocapture
cargo test -p rssh-app window_app_single_pane_applies_translucent_selection_once -- --nocapture
```

Expected: the inactive cell has its terminal background instead of its
selection background. The single-pane test may already pass before the
refactor; retain it as a guard that the new split-composition overlay never
double-blends an active selection.

**Step 3: Extract a plain ordinary-selection overlay helper**

Add a helper near `inactive_pane_snapshot`:

```rust
fn ordinary_selection_snapshot(
    snapshot: TerminalRenderSnapshot,
    selection: Option<WindowSelection>,
    size: rssh_core::TerminalSize,
    foreground: Option<Option<Color>>,
    background: Option<Color>,
) -> TerminalRenderSnapshot {
    let Some(selection) = selection else {
        return snapshot;
    };
    snapshot.with_selection_colors_overlay(
        |row, column| selection.contains(row, column, size),
        foreground,
        background,
    )
}
```

Use this helper from `rebuild_snapshot` for the ordinary active selection while
preserving the existing copy-mode and quick-select color selection. Do not
change the existing inactive-search overlay.

**Step 4: Overlay only inactive selections during split composition**

Before `foreground_text_hsb_snapshot` in the split loop, add an inactive-only
overlay:

```rust
if rect.pane_id != active_pane {
    let runtime = self
        .pane_runtimes
        .get(&rect.pane_id)
        .expect("rendered inactive pane must have a runtime");
    pane_snapshot = ordinary_selection_snapshot(
        pane_snapshot,
        runtime.selection,
        runtime.runtime.terminal().grid().size(),
        self.selection_fg_color,
        self.selection_bg_color,
    );
}
```

Keep the active pane's snapshot unchanged in this loop because
`rebuild_snapshot` already applied its mode-aware selection. The resulting
order must remain:

```text
ordinary selection
→ foreground_text_hsb
→ opacity
→ inactive_pane_hsb
→ minimum contrast
→ bell/hyperlink/viewport
```

Do not bake ordinary selection into `handle_inactive_pane_output`; that
function must continue to store a base terminal snapshot.

**Step 5: Run rendering and output regressions**

Run:

```powershell
cargo test -p rssh-app window_app_renders_active_and_inactive_pane_selections_together -- --nocapture
cargo test -p rssh-app window_app_keeps_inactive_selection_after_inactive_pty_output -- --nocapture
cargo test -p rssh-app window_app_single_pane_applies_translucent_selection_once -- --nocapture
cargo test -p rssh-app inactive_pane_hsb -- --nocapture
cargo test -p rssh-app selection -- --nocapture
```

Expected: all tests pass, and the inactive expected background proves that HSB
was applied after the selection overlay.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: render selections in inactive panes"
```

### Task 3: Lock close, tab movement, window detach, and active-only commands

**Files:**
- Modify: `crates/rssh-app/src/window.rs:82785-82950`
- Modify: `crates/rssh-app/src/window.rs:83465-83785`
- Modify: `crates/rssh-app/src/window.rs:86290-86310`
- Test: `crates/rssh-app/src/window.rs`

**Step 1: Write failing lifecycle tests**

Add tests that use distinct single-line text and selections for pane 1 and pane
2:

```rust
#[test]
fn window_app_clear_selection_only_clears_active_pane() {
    let mut app = app_with_two_selected_panes_for_test();
    app.clear_selection();
    assert!(app.selection.is_none());

    app.dispatch_app_action(AppAction::ActivatePane {
        pane: rssh_core::PaneId::new(1),
    })
    .unwrap();
    assert_eq!(app.selected_text().as_deref(), Some("left"));
}

#[test]
fn window_app_close_pane_restores_surviving_pane_selection() {
    let mut app = app_with_two_selected_panes_for_test();
    app.dispatch_app_action(AppAction::ClosePane {
        pane: rssh_core::PaneId::new(2),
    })
    .unwrap();

    assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
    assert_eq!(app.selected_text().as_deref(), Some("left"));
    assert!(!app.pane_runtimes.contains_key(&rssh_core::PaneId::new(2)));
}

#[test]
fn window_app_move_pane_to_new_tab_preserves_its_selection() {
    let mut app = app_with_two_selected_panes_for_test();
    app.dispatch_app_action(AppAction::MovePaneToNewTab {
        pane: rssh_core::PaneId::new(2),
    })
    .unwrap();

    assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(2));
    assert_eq!(app.selected_text().as_deref(), Some("right"));
}

#[test]
fn window_app_move_pane_to_new_window_drops_gui_selection() {
    let mut app = app_with_two_selected_panes_for_test();
    app.runtime.resize(rssh_core::TerminalSize::new(12, 2));
    app.handle_pty_output(b"one\r\ntwo\r\nthree\r\nfour")
        .unwrap();
    app.scroll_viewport_lines(1);
    assert_eq!(app.scrollback_offset, 1);
    app.selection = Some(WindowSelection::new(
        SelectionCell { row: 0, column: 0 },
        SelectionCell { row: 0, column: 2 },
    ));
    app.refresh_snapshot();

    app.dispatch_app_action(AppAction::MovePaneToNewWindow {
        pane: rssh_core::PaneId::new(2),
    })
    .unwrap();
    let detached = app
        .take_next_pending_window_app()
        .expect("pane should materialize as a detached window");

    assert!(detached.selection.is_none());
    assert_eq!(detached.scrollback_offset, 1);
    assert!(!detached.runtime.terminal().scrollback().is_empty());
    assert_eq!(app.selected_text().as_deref(), Some("left"));
}
```

Create `app_with_two_selected_panes_for_test` only if it reduces duplication
without hiding pane IDs, text, or selection geometry. Also add:

- closing a selected inactive pane leaves the active pane's selection intact;
- a new split pane starts with no selection;
- copying after switching panes copies the active pane's text, not the prior
  pane's text.

**Step 2: Run the lifecycle tests to verify RED**

Run each new test with:

```powershell
cargo test -p rssh-app <unique-test-function-name> -- --nocapture
```

Expected:

- close and tab movement fail until selection follows pane state;
- detached-window selection remains present until the GUI-window boundary is
  explicitly enforced;
- active-only clear/copy tests expose any remaining selection leakage.

Each command must report exactly one selected test by its unique function-name
filter.

**Step 3: Clear selection at the GUI-window detach boundary**

In `take_next_pending_window_app`, make the removed runtime mutable and clear
only its GUI selection before installing it into the detached app:

```rust
let mut runtime = self
    .pane_runtimes
    .remove(&active_pane)
    .unwrap_or_else(|| self.new_inactive_pane_runtime());
runtime.selection = None;
```

Do not reset terminal content, snapshot, scrollback offset, PTY handles, or
bell state.

**Step 4: Preserve active-only command behavior**

Keep `clear_selection`, `selected_text`, copy actions, and mouse selection
using `NativeWindowApp::selection`. Do not add cross-pane iteration to command
handlers. Any pane switch must first complete runtime synchronization, so these
functions naturally address the active pane.

If a close or move path bypasses `sync_pane_runtimes`, route it through the
existing synchronization boundary rather than manually copying selection.

**Step 5: Run lifecycle, selection, pane-select, and manager regressions**

Run:

```powershell
cargo test -p rssh-app window_app_clear_selection_only_clears_active_pane -- --nocapture
cargo test -p rssh-app window_app_close_pane_restores_surviving_pane_selection -- --nocapture
cargo test -p rssh-app window_app_move_pane_to_new_tab_preserves_its_selection -- --nocapture
cargo test -p rssh-app window_app_move_pane_to_new_window_drops_gui_selection -- --nocapture
cargo test -p rssh-app pane_select -- --nocapture
cargo test -p rssh-app detached -- --nocapture
cargo test -p rssh-app selection -- --nocapture
```

Expected: all tests pass.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: enforce pane selection lifecycle"
```

### Task 4: Update parity documentation

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/research/wezterm-parity-gap.md`
- Modify: `docs/mvp-6-app-shell-v1.md`

**Step 1: Document the completed boundary**

Record that ordinary selection is pane-local across focus and tab movement,
inactive selections render simultaneously and survive inactive output, and
GUI-window detach intentionally clears selection to match the pinned upstream
window boundary.

**Step 2: Preserve explicit remaining gaps**

Keep these items open:

- stable scrollback selection row coordinates;
- sequence-number/dirty-line-aware invalidation;
- pane-local search, copy-mode, quick-select, and overlay controller state;
- inactive-pane wheel routing without focus transfer;
- mux/window registry, external CLI, arbitrary Lua callbacks, domains,
  protocol, and renderer parity.

Correct any wording that treats per-pane scrollbars as an open parity gap. The
pinned upstream uses one window-right scrollbar for the active pane.

**Step 3: Verify documentation consistency**

Run:

```powershell
rg -n "pane-local|selection|scrollbar|App Shell v2" docs/architecture.md docs/research/wezterm-parity-gap.md docs/mvp-6-app-shell-v1.md
git diff --check
```

Expected: the three documents agree on both the completed slice and remaining
work, and `git diff --check` passes.

**Step 4: Commit**

```powershell
git add docs/architecture.md docs/research/wezterm-parity-gap.md docs/mvp-6-app-shell-v1.md
git commit -m "docs: record pane-local selection parity"
```

### Task 5: Review and verify the complete slice

**Files:**
- Review: `crates/rssh-app/src/window.rs`
- Review: `docs/architecture.md`
- Review: `docs/research/wezterm-parity-gap.md`
- Review: `docs/mvp-6-app-shell-v1.md`

**Step 1: Run formatting and static diff gates**

Run:

```powershell
cargo fmt --all -- --check
git diff --check
git status --short
```

Expected: formatting and whitespace checks pass; only intentional committed
changes exist.

**Step 2: Run focused clusters**

Run:

```powershell
cargo test -p rssh-app selection -- --nocapture
cargo test -p rssh-app pane_focus -- --nocapture
cargo test -p rssh-app inactive_pane_hsb -- --nocapture
cargo test -p rssh-app split_scrollbar -- --nocapture
cargo test -p rssh-app pane_select -- --nocapture
cargo test -p rssh-app detached -- --nocapture
```

Expected: all focused tests pass.

**Step 3: Run the full application suite**

Run:

```powershell
cargo test -p rssh-app
```

Expected: all `rssh-app` unit tests pass; the existing real-PTY integration
tests may remain explicitly ignored.

**Step 4: Run the workspace gate**

Run:

```powershell
cargo test --workspace
```

Expected: all workspace unit, integration, and doc tests pass.

**Step 5: Request independent code review**

Use `superpowers:requesting-code-review`. The reviewer must compare the final
diff with:

- `docs/plans/2026-07-17-wezterm-pane-local-selection-design.md`;
- pinned upstream `E:\project\R-SSH\refs\wezterm` at
  `093bf6bf2b82b929ed80c04fd54ebc80464f715e`;
- the RED/GREEN evidence from Tasks 1-3.

Resolve every correctness issue and rerun the affected focused tests.

**Step 6: Verify final branch state**

Run:

```powershell
git status --short --branch
git log --oneline --decorate -6
```

Expected: the feature worktree is clean and the branch contains the design,
implementation, tests, and documentation commits ready for local integration
into `codex/wezterm-parity-progress`.
