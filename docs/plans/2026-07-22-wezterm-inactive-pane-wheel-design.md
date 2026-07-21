# WezTerm Inactive-Pane Wheel Routing Design

## Goal and evidence

Route a vertical wheel event to the pane under the pointer without implicitly
changing the active pane. At pinned WezTerm
`093bf6bf2b82b929ed80c04fd54ebc80464f715e`,
`wezterm-gui/src/termwindow/mouseevent.rs:670-704` handles `VertWheel` against
the hovered pane's local coordinates and state, then invalidates the window,
but does not call `set_active_idx`. `Press` and `Move` deliberately have
different focus semantics and are outside this wheel-specific change.

R-SSH currently calls `focus_pane_for_mouse_position` near the start of
`handle_window_mouse_wheel`. That helper dispatches `ActivatePane` before
mouse reporting, custom bindings, alternate-screen translation, or scrollback
handling, so merely wheeling over an inactive pane changes focus.

## Architecture

Separate pane hit-testing from pane focusing. Wheel handling first resolves a
`PaneMouseCell` without activating it, then constructs a target-aware wheel
context for that pane. The context provides access to either the active or an
inactive pane's runtime, pane-local UI state (including the stable viewport),
selection and overlay snapshot state, PTY writer, and mouse coordinates local
to that pane.

The wheel context is temporary and must not change the window's active pane.
Ordinary wheel handling never dispatches `ActivatePane`:

- `pane_focus_follows_mouse` remains owned by cursor-move handling.
- Press/click keeps the existing click-to-focus and click-swallow behavior.
- Tab-bar wheel handling retains priority over pane routing.
- The window-right scrollbar remains bound to the active pane and is not
  reinterpreted as a hovered-pane wheel target.
- Zoomed layouts use the one visible pane as the only pane target; split
  separators are not pane targets.

This is a focused wheel-routing layer, not a general rewrite of all mouse
events.

## Event flow

Each window wheel event follows this order:

1. Save the previous `current_mouse_wheel_delta`, install the current event's
   delta, and restore the previous value on every return path.
2. If the pointer is in the tab bar, run the existing tab-wheel path and stop.
3. Hit-test the visible pane layout without changing focus. If no pane is hit,
   return `false`.
4. Convert the pointer position to coordinates local to the target pane and
   obtain that pane's runtime, UI state, selection/overlay state, and writer.
5. Resolve the custom wheel assignment using the target pane's mouse-reporting
   mode, bypass modifiers, alternate-screen state, and the current event's
   wheel delta.
6. If reporting is enabled and not bypassed, encode the pane-local mouse event
   and write it to the target pane's PTY.
7. Otherwise preserve disabled-default-binding behavior. If defaults are
   enabled, translate wheel input in the target pane's alternate screen to
   arrow keys, or scroll that target pane's stable viewport.

Custom mouse actions execute with the hovered pane as their pane context, so
pane-dependent actions and the current wheel delta refer to the target that
received the event. This context alone must not focus the pane. An action may
change focus only when its existing, explicit semantics dispatch
`ActivatePane`.

Mouse reports and alternate-screen arrows always use the target pane's PTY
writer, application cursor/keypad modes, and Kitty keyboard state; they must
never fall through to the active pane's writer.

## State refresh

After an inactive pane changes, refresh that pane owner's state in place:

- update its stable viewport against its own terminal;
- re-project its selection and reconcile its overlay against the new viewport;
- rebuild its pane snapshot; and
- rebuild/invalidate the composite window snapshot so the changed pane is
  visible.

The active pane's runtime, active UI state, selection, overlay, focus, title,
status, and active scrollbar remain unchanged. Active title/status work is
performed only when the active target's state actually changes. The active
pane path continues to use the same state and refresh semantics as before.

## Boundaries and error behavior

- No pane hit returns `false` without changing focus or pane state.
- Zero and horizontal-only deltas retain their existing unhandled behavior.
- `disable_default_mouse_bindings`, mouse-reporting modes, and
  `bypass_mouse_reporting_modifiers` are evaluated for the target pane while
  preserving their current precedence.
- An inactive pane with no scrollback consumes or rejects the event exactly as
  the equivalent active pane would; it does not become active as a fallback.
- Selection and overlay state are pane-local. Updating one pane must not clear,
  move, or re-project another pane's selection.
- Zoomed-pane hit-testing, tab-bar wheel routing, active-pane wheel behavior,
  click-to-focus, focus-follows-mouse, split resizing, and active scrollbar
  behavior must not regress.

This slice does not generalize `Press`, `Move`, drag, or click routing, and does
not include font shaping work.

## Verification matrix

Focused tests cover:

- scrolling an inactive pane's scrollback without changing the active pane;
- preserving independent active/inactive selections while refreshing only the
  target projection and overlay;
- routing mouse-report wheel bytes to the target PTY writer, including local
  pane coordinates;
- bypass-modifier behavior on the inactive target;
- alternate-screen arrow translation through the target writer;
- custom wheel bindings, hovered-pane action context, explicit focus actions,
  and event-scoped `current_mouse_wheel_delta` restoration;
- `pane_focus_follows_mouse` on cursor movement without wheel-induced focus;
- tab-bar priority, the active window-right scrollbar, zoomed panes, split
  separators, and an inactive pane with no history;
- active-pane wheel compatibility and both enabled and disabled default mouse
  bindings.

Run the focused app-shell tests followed by the complete app/workspace suite.
