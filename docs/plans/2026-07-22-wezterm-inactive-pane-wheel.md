# WezTerm Inactive-Pane Wheel Routing Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Route vertical wheel events to the pane under the pointer, with WezTerm-compatible pane-local coordinates, state, commands, and PTY output, without changing focus unless an explicitly bound action says to do so.

**Architecture:** Separate wheel hit-testing from focus changes and return a copied `WheelHitTarget`: either `PaneSurface(WheelTarget)` with pane id, render rectangle, local cell, and local pixel coordinates, or `ActiveScrollbar` with no pane-surface coordinates. Add owner-local helpers for active and inactive `PaneRuntime`/`PaneUiState` access, refresh, viewport mutation, terminal encoding, and PTY writes; dispatch pane-surface wheel bindings through an exhaustive target-aware `WindowCommand` classifier rather than swapping active state or temporarily focusing a pane. Preserve tab-bar priority and route the active window-right scrollbar directly through its existing active stable-viewport behavior before assignment/reporting/alternate-arrow logic.

**Tech Stack:** Rust 2024, `winit` mouse geometry/events, `rssh-app` native window shell, `rssh-core` App Shell pane actions, `rssh-terminal` stable-row/runtime APIs, built-in unit and integration-style tests in `crates/rssh-app/src/window.rs`.

**Pinned Reference:** WezTerm `093bf6bf2b82b929ed80c04fd54ebc80464f715e`, especially `wezterm-gui/src/termwindow/mouseevent.rs:670-704`.

**Design:** `docs/plans/2026-07-22-wezterm-inactive-pane-wheel-design.md`.

---

## Execution Rules

- Execute Tasks 0-4 in order with one fresh implementation subagent per task.
- After each task commit, dispatch a fresh spec-compliance reviewer. Only after the spec review reports Ready with no critical or important findings, dispatch a fresh code-quality reviewer. The original implementer fixes findings and the same reviewer re-reviews.
- Do not run implementers in parallel: every implementation task edits `crates/rssh-app/src/window.rs`, so parallel work would share mutable state and invalidate RED/GREEN evidence.
- Every behavior change starts with named focused tests, records the expected RED output, implements the smallest coherent slice, then records GREEN plus the listed regressions before committing.
- Never call `focus_pane_for_mouse_position`, dispatch `ActivatePane`, swap active runtime/UI fields, or install an inactive runtime merely to route a wheel event. Explicit user-bound focus or creation commands retain their normal effects.
- Do not generalize Press, Move, drag, click, or split-resize routing. `pane_focus_follows_mouse` remains a cursor-move feature and click-to-focus/swallow behavior remains a press feature.
- Treat `WindowCommand::DisableDefaultAssignment` as mouse-binding control flow, not an ordinary command. It returns `Ok(false)` after suppressing reporting/arrows/default scrolling and restoring event state.
- `WheelHitTarget::ActiveScrollbar` never acquires or clamps a pane-local cell/pixel coordinate and never enters user assignment, terminal reporting, or alternate-screen arrow translation. It invokes only the existing active scrollbar/stable-viewport wheel behavior.
- A missing inactive runtime never falls back to the active pane. A resolved pane without a writer preserves the existing consumed no-op for writer paths.
- Keep `WindowCommand` wheel classification exhaustive. Do not add a wildcard arm or an active-pane fallback for unclassified commands.
- Every task commit must pass `cargo fmt --all -- --check` and `git diff --check`. Commit only that task's intended files.

## Shared Test Conventions

All focused tests live in the existing `#[cfg(test)]` module in `crates/rssh-app/src/window.rs`. Reuse `SharedWriter`, `pane_rect_for_test`, `snapshot_char`, `snapshot_row_text`, `app_with_two_selected_panes_for_test`, and the established `AppAction::SplitPane` fixture pattern. Add compact helpers only where three or more tests need identical setup:

- `app_with_inactive_left_wheel_target_for_test()` creates left pane 1 with history, splits right, leaves pane 2 active, and positions the pointer inside pane 1.
- `wheel_position_for_pane_cell_for_test(app, pane_id, column, row)` derives a physical pointer position from the real `PaneRenderRect`, frame-content left edge, terminal pixel top, frame row offset, and current cell metrics.
- `install_writer_for_pane_for_test(app, pane_id, writer)` installs a writer into the active fields or matching `pane_runtimes` entry without focusing it.
- `pane_viewport_offset_for_test(app, pane_id)` and `pane_snapshot_for_test(app, pane_id)` read the owner's state without changing focus.

Do not duplicate terminal geometry constants in tests. Derive all split-cell and pixel expectations from `pane_render_layout`, `frame_content_pixel_left`, `terminal_pixel_top`, `terminal_frame_row_offset`, `cell_width`, and `cell_height` so padding, tab-bar, DPI, and split geometry remain covered.

### Task 0: Add Non-Focusing Wheel Target and Local Geometry Primitives

**Files:**

- Modify: `crates/rssh-app/src/window.rs:81761-81820` (`PaneRenderRect`, `PaneMouseCell`, new `WheelTarget`)
- Modify: `crates/rssh-app/src/window.rs:90595-90620` (`scrollbar_hit_test`)
- Modify: `crates/rssh-app/src/window.rs:90780-90820` (window/cell pixel geometry)
- Modify: `crates/rssh-app/src/window.rs:91035-91165` (`focus_pane_for_mouse_position`, `pane_cell_at_mouse_position`, pane layout lookup)
- Test: `crates/rssh-app/src/window.rs` unit-test module near `window_app_mouse_wheel_scrolls_split_pane_under_cursor`

**Step 1: Write RED hit-test and cell-geometry tests**

Add these tests:

```rust
#[test]
fn window_app_wheel_target_hits_inactive_pane_without_focusing() { /* ... */ }

#[test]
fn window_app_wheel_target_cell_is_local_to_inactive_split() { /* ... */ }

#[test]
fn window_app_wheel_target_rejects_split_separator_and_outside_terminal() { /* ... */ }

#[test]
fn window_app_wheel_target_zoom_uses_only_visible_pane() { /* ... */ }
```

Prove the helper returns pane 1 while pane 2 remains active; subtracts the matched rect's row/column; returns `None` on the separator, padding/outside-terminal area, and no-hit positions; and in zoom mode only returns the visible zoomed pane.

**Step 2: Run the cell tests and witness RED**

Run:

```text
cargo test -p rssh-app window_app_wheel_target_ -- --nocapture
```

Expected: compile failure because `wheel_hit_target_at_mouse_position`/`WheelHitTarget`/`WheelTarget` do not exist. The fixture must otherwise compile; do not accept an unrelated split-layout failure as RED.

**Step 3: Add the copied target value and non-focusing lookup**

Add a private shape near `PaneMouseCell`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
struct WheelTarget {
    pane_id: rssh_core::PaneId,
    rect: PaneRenderRect,
    cell: PaneMouseCell,
    pixel_position: PhysicalPosition<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WheelHitTarget {
    PaneSurface(WheelTarget),
    ActiveScrollbar {
        pane_id: rssh_core::PaneId,
    },
}
```

Implement these private helpers on `NativeWindowApp`:

```rust
fn wheel_hit_target_at_mouse_position(&self) -> Option<WheelHitTarget>;
fn wheel_target_for_rect(
    &self,
    rect: PaneRenderRect,
    position: PhysicalPosition<f64>,
) -> Option<WheelTarget>;
fn pane_render_rect(&self, pane_id: rssh_core::PaneId) -> Option<PaneRenderRect>;
```

`wheel_hit_target_at_mouse_position` must test the active window-right scrollbar first, then use the visible `pane_render_layout`, and never call the focusing helper. A scrollbar hit returns `ActiveScrollbar { pane_id: active_pane_id }` without calling `wheel_target_for_rect`; there is no synthetic cell/pixel coordinate and no edge clamping. A pane hit returns `PaneSurface(WheelTarget { .. })`. For a pane surface, `cell.column`/`cell.row` are local to `rect`. Compute local pixels as:

- `x = mouse_x - frame_content_pixel_left - rect.column * cell_width`;
- `y = mouse_y - terminal_pixel_top - (rect.row - terminal_frame_row_offset) * cell_height`.

Reject negative/out-of-rect values before conversion. Retain the original floating pixel coordinate inside the pane; clamp only at protocol encoding if existing encoders require integer bounds.

**Step 4: Write RED scrollbar-precedence tests**

Add:

```rust
#[test]
fn window_app_wheel_target_scrollbar_over_inactive_right_split_targets_active_left() { /* ... */ }

#[test]
fn window_app_wheel_target_scrollbar_over_active_right_split_targets_active_right() { /* ... */ }
```

Build both active-left/inactive-right and inactive-left/active-right layouts. Position the pointer over the real window-right scrollbar overlay and assert the result is `WheelHitTarget::ActiveScrollbar { pane_id: active }`, never `PaneSurface`, even when another pane lies beneath it. Also assert no fabricated local cell or pixel value is available from that variant.

**Step 5: Run the scrollbar tests and witness RED**

Run:

```text
cargo test -p rssh-app window_app_wheel_target_scrollbar_ -- --nocapture
```

Expected: compile failure because `WheelHitTarget::ActiveScrollbar` does not exist, or assertions fail because raw pane hit-testing selects the geometric pane/no target.

**Step 6: Add explicit scrollbar precedence**

Before pane hit-testing, call `scrollbar_hit_test(self.mouse_pixel_position?)`. On a hit, return `WheelHitTarget::ActiveScrollbar { pane_id: self.app_shell.active_pane_id() }` directly. Do not resolve a pane rect, derive/clamp a local coordinate, subtract the scrollbar from a pane rect, or focus. Keep tab-bar precedence in the event handler, not in the primitive.

**Step 7: Run GREEN and geometry regressions**

Run:

```text
cargo test -p rssh-app window_app_wheel_target_ -- --nocapture
cargo test -p rssh-app window_app_pane_focus_follows_mouse_moves_focus_when_enabled -- --nocapture
cargo test -p rssh-app window_app_mouse_click_on_window_focus_passes_through_when_disabled -- --nocapture
cargo test -p rssh-app window_app_scrollbar_hit_testing_ -- --nocapture
cargo fmt --all -- --check
git diff --check
```

Expected: all focused and legacy tests pass; formatting and whitespace checks exit zero.

**Step 8: Commit Task 0**

```text
git add crates/rssh-app/src/window.rs
git commit -m "refactor: resolve wheel targets without pane focus"
```

Expected: one commit containing only target/geometry primitives and tests.

### Task 1: Route Runtime, Viewport, Mouse Reports, and Writers to the Target Owner

**Files:**

- Modify: `crates/rssh-app/src/window.rs:81594-81730` (`PaneRuntime` owner-local reconcile/viewport helpers)
- Modify: `crates/rssh-app/src/window.rs:89521-89995` (owner-local snapshot/UI reconciliation and viewport mutation)
- Modify: `crates/rssh-app/src/window.rs:90009-90129` (`handle_window_mouse_wheel*`, alternate-screen translation)
- Modify: `crates/rssh-app/src/window.rs:95500-95555` (`write_pty_bytes`, `write_pty_bytes_to_pane`)
- Test: `crates/rssh-app/src/window.rs` unit-test module near existing wheel/reporting tests

**Step 1: Write RED inactive-scrollback ownership tests**

Add:

```rust
#[test]
fn window_app_wheel_scrolls_inactive_pane_without_focus_transfer() { /* ... */ }

#[test]
fn window_app_wheel_refreshes_only_target_selection_overlay_and_composite() { /* ... */ }

#[test]
fn window_app_wheel_inactive_pane_without_history_matches_active_noop() { /* ... */ }

#[test]
fn window_app_wheel_active_scrollbar_over_inactive_right_uses_active_scrollback_only() { /* ... */ }

#[test]
fn window_app_wheel_active_scrollbar_over_active_right_uses_active_scrollback_only() { /* ... */ }
```

The first replaces the old focus-changing expectation in `window_app_mouse_wheel_scrolls_split_pane_under_cursor`: assert pane 2 remains active, pane 1's stable viewport and owner snapshot change, and pane 2's viewport/snapshot/title state does not. The second starts from two independent selections/overlays and proves only the target projection/reconciliation changes while `render_snapshot()` contains the updated inactive presentation. The third compares handled/result semantics for equivalent active and inactive panes with no history. The final two cover both split orientations. In the active-left case enable reporting plus a matching writer-producing wheel assignment on the active main-screen pane with history: scrollbar wheel changes only its stable viewport and emits no bytes. In the active-right case put the active pane in alternate screen with the same binding: the result/state must match a direct `handle_mouse_wheel(delta)` call, with no assignment, report, or arrow bytes. Neither case changes focus or touches the underlying/inactive pane.

**Step 2: Run the scrollback tests and witness RED**

Run:

```text
cargo test -p rssh-app window_app_wheel_scrolls_inactive_pane_without_focus_transfer -- --nocapture
cargo test -p rssh-app window_app_wheel_refreshes_only_target_selection_overlay_and_composite -- --nocapture
cargo test -p rssh-app window_app_wheel_inactive_pane_without_history_matches_active_noop -- --nocapture
cargo test -p rssh-app window_app_wheel_active_scrollbar_ -- --nocapture
```

Expected: first test reports active pane changed or inactive viewport unchanged; the refresh test observes active-owner state mutation or a stale inactive composite; scrollbar tests enter assignment/reporting/alternate handling or target the underlying pane.

**Step 3: Add owner-local state access and refresh helpers**

Implement private helpers that branch explicitly on `pane_id == active_pane_id`:

```rust
fn pane_runtime_ref(&self, pane_id: rssh_core::PaneId) -> Option<&TerminalRuntime>;
fn pane_ui_ref(&self, pane_id: rssh_core::PaneId) -> Option<&PaneUiState>;
fn set_pane_scrollback_offset(&mut self, pane_id: rssh_core::PaneId, offset: usize) -> bool;
fn scroll_pane_viewport_lines(&mut self, pane_id: rssh_core::PaneId, lines: isize) -> bool;
fn refresh_wheel_target_owner(&mut self, pane_id: rssh_core::PaneId);
```

For inactive owners, update `PaneRuntime.ui.stable_viewport`, reconcile selection/overlay against that same terminal, and rebuild `PaneRuntime.snapshot`. Mark the composite frame dirty (`frame_needs_full_repaint` or the existing equivalent) so the next `render_snapshot` includes it. For the active owner, reuse existing active helpers and title/status refresh behavior. Do not clear or re-project another pane's selection.

Because mutable access to `self` and `pane_runtimes` overlaps, remove/reinsert one inactive `PaneRuntime` for a scoped mutation if necessary; guarantee reinsertion before returning. Do not install it as active.

**Step 4: Write RED target reporting/writer tests**

Add:

```rust
#[test]
fn window_app_wheel_reports_local_cell_to_inactive_target_writer() { /* ... */ }

#[test]
fn window_app_wheel_reports_local_sgr_pixel_to_inactive_target_writer() { /* ... */ }

#[test]
fn window_app_wheel_reporting_scrolls_target_to_bottom_before_report() { /* ... */ }

#[test]
fn window_app_wheel_reporting_bypass_keeps_target_viewport_and_scrolls_normally() { /* ... */ }

#[test]
fn window_app_wheel_alternate_arrows_use_inactive_target_modes_and_writer() { /* ... */ }

#[test]
fn window_app_wheel_missing_target_writer_is_consumed_without_active_fallback() { /* ... */ }
```

Use different `SharedWriter`s for pane 1 and pane 2. Enable SGR cell reporting (`1006`) and SGR pixel/1016 reporting separately; assert bytes contain target-local coordinates, including a non-zero split column, tab-bar/frame offset, and configured padding. Put the target viewport above bottom before non-bypassed reporting and assert it is bottom plus refreshed before bytes are written. With the bypass modifier, assert no report, no forced bottom, modifier bits used only for assignment matching are removed, and ordinary target scrollback runs. For alternate screen, give active and target different application-cursor/Kitty state and prove target encoding/writer wins.

**Step 5: Run the reporting tests and witness RED**

Run:

```text
cargo test -p rssh-app window_app_wheel_reports_local_ -- --nocapture
cargo test -p rssh-app window_app_wheel_reporting_ -- --nocapture
cargo test -p rssh-app window_app_wheel_alternate_arrows_use_inactive_target_modes_and_writer -- --nocapture
cargo test -p rssh-app window_app_wheel_missing_target_writer_is_consumed_without_active_fallback -- --nocapture
```

Expected: the old path focuses the target and/or emits through active coordinates, modes, and writer; the pre-report viewport assertion fails.

**Step 6: Implement target-aware event runtime and writer paths**

Refactor `handle_window_mouse_wheel_with_current_delta` to resolve one `WheelHitTarget` after tab-bar handling and branch immediately:

```rust
match hit {
    WheelHitTarget::ActiveScrollbar { pane_id } => {
        debug_assert_eq!(pane_id, self.app_shell.active_pane_id());
        return Ok(self.handle_mouse_wheel(delta));
    }
    WheelHitTarget::PaneSurface(target) => { /* target-aware path below */ }
}
```

The scrollbar arm is the existing active stable-viewport behavior. It must execute before acquiring reporting/alternate modes or matching an assignment. It must not synthesize/clamp coordinates and must not write to any PTY. For `PaneSurface`, read reporting and alternate-screen modes from that target. Add target-specific equivalents for:

```rust
fn write_pty_bytes_to_pane_for_wheel(
    &mut self,
    pane_id: rssh_core::PaneId,
    bytes: &[u8],
) -> io::Result<()>;
fn handle_alternate_buffer_mouse_wheel_for_target(
    &mut self,
    target: WheelTarget,
    delta: MouseScrollDelta,
) -> io::Result<bool>;
fn encode_wheel_mouse_event_for_target(
    &self,
    target: WheelTarget,
    kind: WindowMouseEventKind,
    mode: MouseInputMode,
    modifiers: ModifiersState,
) -> Option<Vec<u8>>;
```

The existing writer semantics scroll the addressed pane to bottom only when actual terminal input is sent. The mouse-reporting path additionally performs the design-mandated target scroll-to-bottom before assignment lookup/report encoding, and refreshes even if no later bytes are encodable. Missing runtime returns `Ok(false)`; missing writer consumes a selected report/arrow as `Ok(true)` without writing to active.

**Step 7: Run GREEN and active-path regressions**

Run:

```text
cargo test -p rssh-app window_app_wheel_ -- --nocapture
cargo test -p rssh-app window_app_mouse_wheel_ -- --nocapture
cargo test -p rssh-app window_app_renders_active_and_inactive_pane_selections_together -- --nocapture
cargo test -p rssh-app window_app_pane_overlay_ -- --nocapture
cargo fmt --all -- --check
git diff --check
```

Expected: all target and existing active-pane wheel tests pass; no focus or pane-local UI regression.

**Step 8: Commit Task 1**

```text
git add crates/rssh-app/src/window.rs
git commit -m "feat: route wheel runtime operations to hovered pane"
```

Expected: one commit for runtime, viewport, reporting, writer, refresh helpers, and tests.

### Task 2: Dispatch Every Wheel Binding with an Exhaustive Target Context

**Files:**

- Modify: `crates/rssh-app/src/window.rs:85535-86590` (`command_palette_apply_command` and command-to-`AppAction` mapping)
- Modify: `crates/rssh-app/src/window.rs:87516-87555` (`handle_user_mouse_assignment`)
- Modify: `crates/rssh-app/src/window.rs:90019-90090` (wheel assignment order and fallback)
- Modify: `crates/rssh-app/src/window.rs:118496-118650` (`WindowCommand`, new exhaustive wheel-dispatch classification)
- Test: `crates/rssh-app/src/window.rs` unit-test module near existing `Multiple`, mouse-binding, pane action, copy/paste, and wheel-current-event tests

**Step 1: Write RED viewport and current-event binding tests**

Add:

```rust
#[test]
fn window_app_wheel_binding_viewport_actions_use_hovered_pane() { /* ... */ }

#[test]
fn window_app_wheel_binding_current_delta_uses_hovered_pane_and_current_event() { /* ... */ }

#[test]
fn window_app_wheel_binding_multiple_recursively_retains_target() { /* ... */ }
```

Cover `ScrollByLine`, `ScrollByPage`, `ScrollToTop`, `ScrollToBottom`, `ScrollToPrompt`, and `ScrollByCurrentEventWheelDelta`. Use `Multiple` with at least two target-dependent actions and assert both act on the same inactive owner while focus stays unchanged.

**Step 2: Write RED writer, pane-UI, mouse-cell, and pane-action binding tests**

Add:

```rust
#[test]
fn window_app_wheel_binding_writer_actions_use_hovered_pane_modes_and_writer() { /* ... */ }

#[test]
fn window_app_wheel_binding_copy_overlay_actions_use_hovered_pane_ui() { /* ... */ }

#[test]
fn window_app_wheel_binding_pane_actions_use_hovered_pane_id() { /* ... */ }

#[test]
fn window_app_wheel_binding_global_action_keeps_window_scope() { /* ... */ }

#[test]
fn window_app_wheel_binding_select_text_at_mouse_uses_hovered_local_cell() { /* ... */ }

#[test]
fn window_app_wheel_binding_extend_selection_uses_hovered_local_cell() { /* ... */ }

#[test]
fn window_app_wheel_binding_open_link_uses_hovered_snapshot_and_local_cell() { /* ... */ }

#[test]
fn wheel_action_io_error_includes_stable_command_and_app_error_context() { /* ... */ }
```

The writer matrix covers `SendString`, `SendPaste`, `SendKey`, clipboard paste, and primary-selection paste with distinct bracketed-paste/application-key/Kitty modes and writers. The pane-UI matrix covers selection source, `CopyTo*`, `ClearSelection`, Search/Copy Mode/Quick Select ownership, and a representative overlay mutation. The pane-action matrix covers a non-focus pane-scoped command such as close/reset/clear scrollback against the hovered id. The global test uses a harmless implemented global action such as font adjustment or reload-state observation and proves it does not silently retarget pane state.

Explicitly classify and exercise every mouse-position pane-local command family:

- `SelectTextAtMouseCursorCell`, `SelectTextAtMouseCursorWord`, `SelectTextAtMouseCursorLine`, `SelectTextAtMouseCursorBlock`, `SelectTextAtMouseCursorSemanticZone`, and `SelectTextAtMouseCursor(_)`;
- `ExtendSelectionToMouseCursorCell`, `ExtendSelectionToMouseCursorWord`, `ExtendSelectionToMouseCursorLine`, `ExtendSelectionToMouseCursorBlock`, `ExtendSelectionToMouseCursorSemanticZone`, and `ExtendSelectionToMouseCursor(_)`;
- `CompleteSelectionOrOpenLinkAtMouseCursor` and `CompleteSelectionOrOpenLinkAtMouseCursorTo(_)`;
- `OpenLinkAtMouseCursor`.

The three named RED tests are the minimum representative proof: selection starts in the inactive split at `WheelTarget.cell`, extension uses that owner's retained selection/overlay, and link lookup uses the hovered owner's snapshot/hyperlink context rather than `self.snapshot`/active mouse cell. Assert the window/global copy destination remains global and any resulting PTY or OS side effect follows the command's existing semantics; only its pane source/context changes.

**Step 3: Write RED direction/reference and creation tests**

Add:

```rust
#[test]
fn window_app_wheel_binding_direction_focus_uses_hovered_pane_as_reference() { /* ... */ }

#[test]
fn window_app_wheel_binding_by_index_keeps_tab_index_scope() { /* ... */ }

#[test]
fn window_app_wheel_binding_new_tab_keeps_explicit_creation_semantics() { /* ... */ }

#[test]
fn window_app_wheel_binding_split_uses_hovered_pane_as_source() { /* ... */ }
```

Use a three-pane layout where direction from inactive hovered pane differs from direction from the active pane. Assert routing alone does not focus, but the explicit focus command does. Assert `SplitPane` attaches to the hovered pane's topology/source and the created pane follows existing activation semantics.

**Step 4: Run the complete Task 2 RED matrix before any production edit**

Run:

```text
cargo test -p rssh-app window_app_wheel_binding_viewport_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_current_delta_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_multiple_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_writer_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_copy_overlay_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_pane_actions_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_global_action_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_select_text_at_mouse_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_extend_selection_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_open_link_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_direction_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_by_index_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_new_tab_ -- --nocapture
cargo test -p rssh-app window_app_wheel_binding_split_ -- --nocapture
cargo test -p rssh-app wheel_action_io_error_ -- --nocapture
```

Expected: the old path focuses/uses active viewport, writer, runtime, UI, snapshot, mouse cell, pane id, or direction/split reference. Record the failure from every category. Do not edit a production function, introduce the classifier, or add dispatcher plumbing until this complete RED matrix has run.

**Step 5: Add the stable result/error contract and closed command classifier**

Define these private contracts in the first dispatcher implementation; Task 3 extends their handling but must not replace them with an incompatible boolean API:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum WheelAssignmentMatch {
    None,
    DisableDefault,
    Command(WindowCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WheelCommandOutcome {
    Consumed,
}

fn wheel_action_io_error(
    command: &WindowCommand,
    error: AppShellError,
) -> io::Error {
    io::Error::other(format!(
        "wheel action '{}' failed: {error:?}",
        command.label()
    ))
}
```

If the actual error types require a mechanically different signature, keep the same contract: an ordinary matched command returns `io::Result<WheelCommandOutcome>`, never a lossy boolean; `AppShellError` becomes `io::Error` in one helper with stable command label plus source-variant context. The focused conversion test asserts `io::ErrorKind::Other` and the exact text `wheel action 'Close Workspace' failed: CannotCloseLastWorkspace`, so both the API category and diagnostic context cannot silently regress.

Define a private classification with no wildcard match:

```rust
enum WheelCommandClass {
    Viewport,
    Writer,
    PaneUi,
    PaneAction,
    ExplicitFocusOrCreation,
    Global,
    Composite,
    DisableDefault,
    Nop,
}
```

Add `WindowCommand::wheel_command_class(&self) -> WheelCommandClass` with an explicit arm for every current enum variant, plus:

```rust
fn apply_wheel_command_for_target(
    &mut self,
    target: WheelTarget,
    command: WindowCommand,
) -> io::Result<WheelCommandOutcome>;
```

The target-aware entry point must implement every pane-dependent command against `target.pane_id`; it may call `command_palette_apply_command` only for variants classified as truly window/tab/application/configuration global. `Multiple` recursively calls the target-aware entry point and stops at the first error. A future `WindowCommand` variant must fail compilation until classified. `Nop` is an ordinary matched command and returns `Consumed`; `DisableDefault` is represented by `WheelAssignmentMatch` and never reaches this dispatcher.

**Step 6: Implement all target-dependent command categories**

Factor owner-local command helpers rather than duplicating the entire command palette. Required behavior:

- viewport/prompt commands use target stable viewport and terminal history;
- writer/paste/key commands encode with target runtime state and write only to target writer;
- copy/selection/search/copy-mode/quick-select commands use target `PaneUiState` and target snapshot/terminal, while clipboard destinations remain global;
- pane-scoped `AppAction`s receive `target.pane_id` explicitly;
- direction-relative, current-pane-domain, and split-source calculations use `target.pane_id` as their reference;
- by-index commands retain tab/window index scope;
- `ActivatePaneDirection`, `ActivatePaneByIndex`, numbered pane activation, `NextPane`, `PreviousPane`, tab activation, `NewTab`, `SplitPane`, spawn/creation, and equivalent explicit commands retain their normal focus/creation effects;
- ordinary matched commands are consumed exactly once: success is `Ok(true)` at the event layer; errors propagate and never trigger reporting/default fallback.
- all `SelectTextAtMouseCursor*`, `ExtendSelectionToMouseCursor*`, `CompleteSelectionOrOpenLinkAtMouseCursor*`, and `OpenLinkAtMouseCursor` variants use `WheelTarget.cell` plus the hovered owner's snapshot, terminal, selection, overlay, and hyperlink context; none may call `mouse_cell_for_active_pane` or read `self.snapshot` as its source;
- every `AppShellError` from a matched action is converted only by `wheel_action_io_error`, retaining stable command/error context; no `eprintln!`-and-`false` path is allowed.

If an existing helper is hard-coded to active state, add a `*_for_pane` core and leave the old active wrapper for keyboard/palette callers. Do not alter non-wheel command behavior.

**Step 7: Implement reference-pane plumbing**

Add explicit reference-pane parameters to the narrow direction, split-source, current-pane-domain, and creation action builders/dispatch paths. Pass `target.pane_id` from the wheel dispatcher and the active pane id from existing keyboard/palette wrappers. Do not infer the reference by reading whichever pane is active at execution time.

**Step 8: Run GREEN and command regressions**

Run:

```text
cargo test -p rssh-app window_app_wheel_binding_ -- --nocapture
cargo test -p rssh-app window_app_palette_multiple_ -- --nocapture
cargo test -p rssh-app window_app_dispatches_palette_ -- --nocapture
cargo test -p rssh-app window_app_scroll_by_current_event_ -- --nocapture
cargo test -p rssh-app wheel_action_io_error_ -- --nocapture
cargo fmt --all -- --check
git diff --check
```

Expected: the entire pre-recorded RED matrix, stable error-context assertion, and existing keyboard/palette dispatch pass. Inspect the test log to confirm the selection, extension, open-link, writer, direction, split, nested action, and error cases all executed rather than being filtered to zero tests.

**Step 9: Commit Task 2**

```text
git add crates/rssh-app/src/window.rs
git commit -m "feat: dispatch wheel bindings in hovered pane context"
```

Expected: one commit for exhaustive target-aware command dispatch and tests.

### Task 3: Enforce DisableDefaultAssignment and Full Interaction Boundaries

**Files:**

- Modify: `crates/rssh-app/src/window.rs:87516-87595` (assignment match result and modifier matching)
- Modify: `crates/rssh-app/src/window.rs:90019-90129` (wheel precedence and suppression)
- Test: `crates/rssh-app/src/window.rs` unit-test module near existing default-mouse-binding and pane interaction tests

**Step 1: Write RED DisableDefaultAssignment matrix tests**

Add:

```rust
#[test]
fn window_app_wheel_disable_default_suppresses_inactive_scrollback_and_returns_false() { /* ... */ }

#[test]
fn window_app_wheel_disable_default_reporting_scrolls_bottom_but_emits_no_report() { /* ... */ }

#[test]
fn window_app_wheel_disable_default_alternate_emits_no_arrow() { /* ... */ }

#[test]
fn window_app_wheel_disable_default_bypass_matches_effective_modifiers() { /* ... */ }
```

Assert `Ok(false)`, no default scroll/report/arrow, unchanged focus, and restored `current_mouse_wheel_delta`. In non-bypassed reporting, assert the mandatory pre-assignment scroll-to-bottom still happens and refreshes the target. In bypass mode, remove only configured bypass bits before matching and do not perform reporting's forced-bottom step.

**Step 2: Run the suppression tests and witness RED**

Run:

```text
cargo test -p rssh-app window_app_wheel_disable_default_ -- --nocapture
```

Expected: current lookup excludes `DisableDefaultAssignment` and falls into report/default handling, or returns the wrong consumed status.

**Step 3: Complete DisableDefault handling on the existing typed contract**

Use the `WheelAssignmentMatch` and `WheelCommandOutcome` API introduced in Task 2; do not replace it with a boolean or a second incompatible result type. Match the full event, effective modifiers, target reporting state, and target alternate-screen state. The wheel handler evaluates `WheelAssignmentMatch` after target reporting pre-scroll and before terminal reporting/defaults. `DisableDefault` skips dispatch and all later behavior and returns `Ok(false)`. `Command(command)` calls `apply_wheel_command_for_target` and maps `WheelCommandOutcome::Consumed` to `Ok(true)`. Ordinary `AppShellError`s must already be converted through `wheel_action_io_error`; propagate them after refresh and do not fall through. Preserve the old button/drag helper semantics for non-wheel events.

**Step 4: Write RED interaction-boundary tests**

Add:

```rust
#[test]
fn window_app_wheel_keeps_inactive_focus_title_and_active_scrollbar_state() { /* ... */ }

#[test]
fn window_app_wheel_does_not_replace_independent_pane_selections_or_overlays() { /* ... */ }

#[test]
fn window_app_wheel_preserves_focus_follows_mouse_click_and_swallow_semantics() { /* ... */ }

#[test]
fn window_app_wheel_tab_bar_precedes_pane_routing() { /* ... */ }

#[test]
fn window_app_wheel_separator_and_no_hit_return_false_without_state_change() { /* ... */ }

#[test]
fn window_app_wheel_zoomed_layout_routes_only_visible_pane() { /* ... */ }
```

Capture active pane, active title/status, active scrollbar/viewport, selection/overlay owner state, tab id, and pane count before and after. Test cursor move with `pane_focus_follows_mouse` enabled separately from wheel, and click/swallow separately from wheel. Include zero and horizontal-only deltas in no-hit/unhandled assertions.

**Step 5: Run the boundary tests and witness RED**

Run:

```text
cargo test -p rssh-app window_app_wheel_keeps_inactive_focus_ -- --nocapture
cargo test -p rssh-app window_app_wheel_does_not_replace_ -- --nocapture
cargo test -p rssh-app window_app_wheel_preserves_focus_follows_ -- --nocapture
cargo test -p rssh-app window_app_wheel_tab_bar_precedes_ -- --nocapture
cargo test -p rssh-app window_app_wheel_separator_and_no_hit_ -- --nocapture
cargo test -p rssh-app window_app_wheel_zoomed_layout_ -- --nocapture
```

Expected: any remaining implicit activation, active-state reuse, or misplaced precedence is exposed. If all pass before production changes, record that as existing coverage and add the smallest missing assertion from the design matrix rather than manufacturing a failure.

**Step 6: Close only the observed boundary gaps**

Adjust event ordering and owner-local refresh, not click/move behavior. The final order is: install delta guard; tab bar; resolve `WheelHitTarget`; return directly through active stable-viewport handling for `ActiveScrollbar`; for `PaneSurface` only, obtain local geometry/runtime, perform reporting pre-scroll, assignment, reporting, disabled-default handling, then target alternate arrows or target stable scrollback. A no-hit or missing-runtime result is `Ok(false)` with no fallback.

**Step 7: Run GREEN and complete interaction regressions**

Run:

```text
cargo test -p rssh-app window_app_wheel_ -- --nocapture
cargo test -p rssh-app window_app_mouse_wheel_ -- --nocapture
cargo test -p rssh-app window_app_pane_focus_follows_mouse_ -- --nocapture
cargo test -p rssh-app window_app_clicking_inactive_pane_ -- --nocapture
cargo test -p rssh-app window_app_swallow_mouse_click_ -- --nocapture
cargo test -p rssh-app window_app_scrollbar_ -- --nocapture
cargo fmt --all -- --check
git diff --check
```

Expected: all focused behavior and legacy pointer semantics pass.

**Step 8: Commit Task 3**

```text
git add crates/rssh-app/src/window.rs
git commit -m "fix: preserve wheel routing interaction boundaries"
```

Expected: one commit for suppression/precedence/boundary tests and minimal fixes.

### Task 4: Prove Restoration/Error Semantics, Update Parity Claims, and Run Full Verification

**Files:**

- Modify: `crates/rssh-app/src/window.rs:90019-90026` (`current_mouse_wheel_delta` lifetime/finalizer)
- Modify: `crates/rssh-app/src/window.rs:192543-192590` or test helper area (reusable failing writer)
- Modify: `docs/architecture.md:284-288`
- Modify: `docs/architecture.md:1318-1326`
- Test: `crates/rssh-app/src/window.rs` unit-test module

**Step 1: Write RED success/unhandled/error finalizer tests**

Add:

```rust
#[test]
fn window_app_wheel_restores_previous_delta_after_success_and_unhandled() { /* ... */ }

#[test]
fn window_app_wheel_restores_delta_and_refreshes_target_after_assignment_error() { /* ... */ }

#[test]
fn window_app_wheel_restores_delta_and_refreshes_target_after_pty_error() { /* ... */ }

#[test]
fn window_app_wheel_missing_runtime_returns_false_without_active_fallback() { /* ... */ }

#[test]
fn window_app_wheel_reporting_unencodable_returns_false_without_default_scroll() { /* ... */ }
```

Seed `current_mouse_wheel_delta` with a distinct prior value and assert exact restoration. Use an existing failing App Shell action (for example an action whose invariant returns `AppShellError`) as an ordinary matched command and a `FailingWriter` for PTY I/O. The assignment-error assertion must observe the same `io::ErrorKind::Other` and exact command/source context produced by `wheel_action_io_error`; top-level wheel routing may not replace it with a generic message. Before each error, arrange a reporting target above bottom so the test proves owner refresh/rebuild occurs before `Err`. Remove only the addressed inactive `pane_runtimes` entry for the missing-runtime test; assert active writer/UI remains untouched. For unencodable reporting use a valid reporting mode plus a coordinate/protocol condition that existing encoding rejects, then assert no scrollback fallback.

**Step 2: Run the finalizer tests and witness RED**

Run:

```text
cargo test -p rssh-app window_app_wheel_restores_ -- --nocapture
cargo test -p rssh-app window_app_wheel_missing_runtime_ -- --nocapture
cargo test -p rssh-app window_app_wheel_reporting_unencodable_ -- --nocapture
```

Expected: at least the error-path refresh or restoration assertion fails before the finalizer is centralized. A failure must exercise the intended path, not panic while building the fixture.

**Step 3: Centralize delta restoration and dirty-target refresh**

Keep `handle_window_mouse_wheel` as the sole delta lifetime owner. Use a closure/finalizer pattern that restores the exact previous `Option<MouseScrollDelta>` on `Ok(true)`, `Ok(false)`, and every `Err`. Add a scoped target-dirty guard or one exit wrapper around `handle_window_mouse_wheel_with_current_delta` so any pre-report scroll or target command mutation triggers `refresh_wheel_target_owner` before returning, including errors. Avoid unsafe code and avoid a guard that holds `&mut self` across dispatch.

**Step 4: Run GREEN and the full focused matrix**

Run:

```text
cargo test -p rssh-app window_app_wheel_ -- --nocapture
cargo test -p rssh-app window_app_mouse_wheel_ -- --nocapture
cargo test -p rssh-app window_app_pane_focus_follows_mouse_ -- --nocapture
cargo test -p rssh-app window_app_renders_active_and_inactive_pane_selections_together -- --nocapture
```

Expected: all new and legacy wheel/focus/selection tests pass.

**Step 5: Update the bounded parity documentation**

In both inactive-wheel backlog locations in `docs/architecture.md`, replace the open-item wording with a bounded completed statement. Record that vertical wheel routing uses hovered-pane cell/pixel coordinates, runtime/UI/writer and target-aware bindings without implicit focus; tab bar, active scrollbar, click, move, and explicit focus/creation command semantics remain distinct. Keep the existing disclaimer that this is not general mouse routing, full App Shell v2, font shaping, or general WezTerm parity.

**Step 6: Run formatting and documentation diff checks**

Run:

```text
cargo fmt --all -- --check
git diff --check
git diff -- docs/architecture.md crates/rssh-app/src/window.rs
```

Expected: formatting and whitespace checks exit zero; the diff contains only this slice and its tests/docs.

**Step 7: Run the unskipped app suite and record the known fixture result separately**

Run:

```text
cargo test -p rssh-app
```

Expected in a checkout without `refs/wezterm/docs/colorschemes/data.json`: exactly these two environment-dependent tests fail, and no others:

- `builtin_color_scheme_lookup_covers_pinned_wezterm_names_and_aliases`
- `builtin_color_scheme_lookup_matches_all_pinned_wezterm_palette_data`

If the fixture exists, expect the entire suite to pass. Any different failure is a code failure and must be debugged before continuing. Do not fabricate, download, or junction a fixture merely to turn this gate green.

**Step 8: Run the app suite with only the two known fixture tests skipped**

Run:

```text
cargo test -p rssh-app -- --skip builtin_color_scheme_lookup_covers_pinned_wezterm_names_and_aliases --skip builtin_color_scheme_lookup_matches_all_pinned_wezterm_palette_data
```

Expected: all remaining `rssh-app` tests pass with zero failures.

**Step 9: Run the complete workspace/all-targets suite without skips and record evidence**

Run:

```text
cargo test --workspace --all-targets
```

Expected in a checkout without `refs/wezterm/docs/colorschemes/data.json`: all targets build and run, and exactly the same two `rssh-app` palette-fixture tests named in Step 7 fail; no core, terminal, renderer, SSH, PTY, bin, example, or other target fails. If the fixture exists, expect the whole command to pass. Preserve this output separately from the app-only evidence; any additional failure blocks completion.

**Step 10: Run the complete workspace/all-targets suite with only the two known fixture tests skipped**

Run:

```text
cargo test --workspace --all-targets -- --skip builtin_color_scheme_lookup_covers_pinned_wezterm_names_and_aliases --skip builtin_color_scheme_lookup_matches_all_pinned_wezterm_palette_data
```

Expected: every workspace target passes with zero failures. Do not replace this gate with package-only tests.

**Step 11: Run affected and adjacent crate suites explicitly**

Run:

```text
cargo test -p rssh-core
cargo test -p rssh-terminal
cargo test -p rssh-renderer
cargo fmt --all -- --check
git diff --check
```

Expected: every command exits zero. Although the implementation is app-local, these suites guard App Shell pane actions, terminal mode/coordinate assumptions, and composite rendering contracts.

**Step 12: Commit Task 4**

```text
git add crates/rssh-app/src/window.rs docs/architecture.md
git commit -m "test: verify inactive pane wheel parity"
```

Expected: one commit with finalizer/error tests, minimal fixes, and bounded parity documentation.

**Step 13: Prepare the final review evidence**

Run:

```text
git status --short --branch
git log --oneline --decorate -6
git diff --check HEAD~5..HEAD
git diff --stat 1e75b034..HEAD
```

Expected: clean feature worktree, Task 0-4 commits visible after the plan commit, zero diff-check errors, and only the intended app-shell/tests/docs files changed from baseline `1e75b034`.

Dispatch a fresh final spec reviewer against the approved design and this plan, then a fresh final quality reviewer only after spec Ready. Require both to inspect the complete `1e75b034..HEAD` diff and the recorded full-suite outputs. Fix and re-review every critical/important finding before integration; rerun the narrow affected tests plus Steps 7-11 after any code change.

## Completion Checklist

- Wheel over an inactive pane never focuses it unless the matched command explicitly focuses or creates/activates something.
- Tab bar and active window-right scrollbar precedence match the approved design.
- Target-local cell and SGR pixel/1016 coordinates use the hovered pane's true origin.
- Reporting pre-scroll, bypass modifiers, alternate arrows, default scrollback, selection/overlay refresh, snapshots, and PTY writes all use the target owner.
- Every wheel-reachable `WindowCommand` is exhaustively classified; pane-relative commands use the hovered pane as reference and global/by-index commands keep their established scope.
- `DisableDefaultAssignment`, missing runtime/writer, unencodable reporting, ordinary command errors, PTY errors, and nested `Multiple` follow the specified return/fallback semantics.
- `current_mouse_wheel_delta` restores on every exit; inactive updates become visible without changing active title/status/scrollbar/UI.
- Cursor-move focus-following, click-to-focus/swallow, tab wheel, zoom, separator, active-pane wheel, and disabled-default behavior do not regress.
- Focused tests, unskipped app and workspace/all-targets evidence, two-skip app and workspace/all-targets suites, explicit core/terminal/renderer suites, formatting, diff checks, per-task spec/quality reviews, and final reviews are all recorded.
