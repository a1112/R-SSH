# WezTerm Pane-Local Selection Design

## Goal

Make ordinary mouse and action-driven text selection belong to the pane that
created it. Each pane must retain its own selection while focus moves between
panes or tabs, inactive panes must continue to render their own selection, and
selection commands must affect only the active pane.

This slice follows the GUI selection ownership in WezTerm commit
`093bf6bf2b82b929ed80c04fd54ebc80464f715e` without claiming stable scrollback
selection coordinates or complete pane-local ownership for search, copy mode,
quick select, and other overlays.

## Upstream contract

The pinned WezTerm source stores GUI state in
`TermWindow::pane_state: HashMap<PaneId, PaneState>`. Each `PaneState` owns its
own `Selection` and viewport. Selection reads, writes, clearing, extraction,
and rendering are always keyed by `PaneId`.

The relevant visible contract is:

- switching focus does not copy one pane's selection into another pane;
- returning to a pane restores that pane's selection;
- multiple panes can display different selections simultaneously;
- clear, copy, and extend operations target the selected pane;
- inactive pane colors, including selection colors, are transformed through
  `inactive_pane_hsb`;
- selection is GUI-window state, so moving a pane to another GUI window does
  not carry the source window's selection state into the destination window.

WezTerm represents selection rows with stable scrollback indices and uses
terminal sequence numbers to invalidate only intersecting dirty selections.
Those behaviors require a later coordinate and damage-tracking slice.

## Current R-SSH boundary

`NativeWindowApp` currently stores one window-wide `selection` and
`selecting` flag. Terminal state, snapshots, and scrollback viewport are
swapped between the active slots and per-pane `PaneRuntime` values when pane
focus changes.

The window-wide selection causes several mismatches:

- a newly focused pane can observe the prior pane's selection coordinates;
- commands cannot distinguish selections created in different panes;
- inactive PTY output replaces that pane's snapshot with an unselected base
  snapshot;
- the lifecycle for close, tab movement, and detach cannot express the
  upstream GUI-window selection boundary.

The renderer already composes a snapshot per pane and applies
`inactive_pane_hsb` to inactive panes. No renderer or core API change is
required.

## Approaches considered

### Store selection alongside each `PaneRuntime` (selected)

Add `selection: Option<WindowSelection>` to `PaneRuntime`. The active pane
continues to use the existing `NativeWindowApp::selection` slot; selection is
moved between the active slot and the corresponding `PaneRuntime` in the same
places that already swap terminal, snapshot, and viewport state.

This gives selection one authoritative owner, reuses the existing pane
lifecycle, and lets close and tab movement follow the runtime naturally.
`selecting` remains a window-level mouse-drag transient and is never restored.

### Add an independent `HashMap<PaneId, WindowSelection>`

This resembles the upstream `PaneState` map more directly, but it duplicates
the creation, retention, close, pending-window, and detach lifecycle already
implemented by `pane_runtimes`. In the current architecture, the separate map
can drift from the runtime topology without providing a behavioral advantage.

### Refactor all GUI pane state into a new `PaneState`

A larger refactor could combine runtime, selection, search, copy mode, quick
select, viewport, bell, and mouse state. That may be a useful long-term shape,
but it expands this slice beyond ordinary selection and makes the parity
behavior harder to review independently.

## State model and lifecycle

`PaneRuntime` gains:

```rust
selection: Option<WindowSelection>,
```

The state rules are:

1. A newly created pane starts with no selection.
2. `take_active_runtime` moves `NativeWindowApp::selection` into the outgoing
   runtime and clears the active slot.
3. `install_active_runtime` restores that runtime's selection into the active
   slot and leaves the runtime slot empty while active.
4. Every active-pane change cancels `selecting`; a mouse drag cannot continue
   across panes, tabs, or windows.
5. Closing an inactive pane drops its runtime and selection together.
6. Closing an active pane stores or drops its outgoing runtime through the
   existing topology synchronization, then restores the surviving active
   pane's own selection.
7. Moving a pane to a new tab within the same GUI window preserves its
   selection because the runtime remains owned by the same `NativeWindowApp`.
8. Before a pending pane is materialized as a new GUI window, its selection is
   cleared. Terminal state, scrollback, and viewport still transfer.

`selecting` is not stored in `PaneRuntime`. It represents an in-progress mouse
capture rather than persistent pane state. Switching focus preserves the
selection geometry produced so far but sets `selecting` to `false`.

## Snapshot and rendering flow

Stored pane snapshots remain base terminal snapshots for ordinary selection.
Selection is applied at the presentation boundary:

1. obtain the pane's base terminal snapshot;
2. overlay that pane's ordinary selection using `selection_fg_color` and
   `selection_bg_color`;
3. apply `foreground_text_hsb`;
4. apply text and window background opacity;
5. for inactive panes, apply `inactive_pane_hsb`;
6. apply minimum contrast, compose cursor, visual bell, hyperlink rules, and
   viewport placement using the existing order.

The active pane already has mode-specific selection colors for copy mode and
quick select. Its snapshot must not receive a second ordinary-selection
overlay during split composition. A shared helper will distinguish the plain
inactive selection overlay from the existing active snapshot rebuild path.

`handle_inactive_pane_output` currently replaces `runtime.snapshot` with a
fresh `terminal_runtime_snapshot`. Because ordinary selection remains separate
from that base snapshot and is applied during `render_snapshot`, inactive PTY
output cannot erase the visible selection.

## Command and input semantics

Existing ordinary selection operations continue to use the active
`NativeWindowApp::selection` slot:

- SelectTextAtMouseCursor, ExtendSelectionToMouseCursor, mouse drag,
  double-click, triple-click, and block selection update only the active pane;
- ClearSelection clears only the active pane;
- CopyTo, CopyAndClose, CompleteSelection, and selected-text extraction read
  only the active pane;
- click-to-focus and focus-follows-mouse expose the target pane's previously
  stored selection after synchronization;
- switching panes while dragging ends the drag without clearing the selection
  already formed in the source pane.

Search, copy mode, quick select, pane select, and other overlays keep their
existing window-level controller lifecycle in this slice. Pane switches must
continue to use their established exit/clear paths so an active-only overlay
is not applied to a newly focused pane.

## Error and invariant handling

The implementation introduces no new fallible external interface. Internal
invariants are:

- the active pane's selection exists only in
  `NativeWindowApp::selection`;
- an inactive pane's selection exists only in its `PaneRuntime`;
- installing a runtime consumes its stored selection;
- taking a runtime clears the active slot and active drag;
- a runtime crossing a GUI-window boundary has no selection;
- a snapshot receives an ordinary selection overlay at most once.

Tests should expose violations through visible cell colors, selected text, or
state assertions rather than adding silent recovery that could conceal
duplicate ownership.

## Testing

The test matrix will prove:

1. pane A and pane B can hold different selections and restore the correct
   selected text after repeated focus changes;
2. active and inactive selections render simultaneously;
3. inactive PTY output rebuilds the base snapshot without erasing that pane's
   selection;
4. inactive selection colors are overlaid before `inactive_pane_hsb`;
5. switching while `selecting` cancels the drag but preserves the source
   selection;
6. ClearSelection and copy operations affect only the active pane;
7. closing active and inactive panes removes only the closed pane's state and
   restores the surviving selection;
8. a new split pane starts with no selection;
9. MovePaneToNewTab preserves selection inside the same GUI window;
10. MovePaneToNewWindow clears selection in the destination while retaining
    terminal content and viewport state;
11. single-pane and split rendering do not apply selection twice.

Regression gates include focused pane selection tests, pane focus tests,
inactive-pane HSB tests, split scrollbar tests, `cargo test -p rssh-app`,
`cargo test --workspace`, `cargo fmt --all -- --check`, and
`git diff --check`.

## Completion boundary

This slice completes ordinary selection ownership and rendering per pane for
the pinned WezTerm behavior. It does not complete:

- stable scrollback-row selection coordinates;
- sequence-number and dirty-line-aware selection invalidation;
- pane-local search, copy-mode, quick-select, or overlay controllers;
- cross-GUI-window selection transfer;
- per-pane scrollbars, which the pinned WezTerm source does not implement;
- the remaining mux registry, external CLI, Lua callback, domain, protocol, or
  renderer parity work.
