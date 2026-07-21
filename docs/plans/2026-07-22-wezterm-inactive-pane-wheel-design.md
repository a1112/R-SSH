# WezTerm Inactive-Pane Wheel Routing Design

## Goal and evidence

Route a vertical wheel event to the pane under the pointer without implicitly
changing the active pane. At pinned WezTerm
`093bf6bf2b82b929ed80c04fd54ebc80464f715e`,
`wezterm-gui/src/termwindow/mouseevent.rs:670-704` handles `VertWheel` against
the hovered pane's local coordinates and state, then invalidates the window,
but does not call `set_active_idx`. `Press` and `Move` deliberately have
different focus semantics and are outside this wheel-specific change. The same
path subtracts the matched pane's cell origin before dispatch. Later in the
handler, a mouse-grabbed pane that is not bypassed is first scrolled to the
bottom, before mouse-assignment lookup or terminal mouse reporting.

R-SSH currently calls `focus_pane_for_mouse_position` near the start of
`handle_window_mouse_wheel`. That helper dispatches `ActivatePane` before
mouse reporting, custom bindings, alternate-screen translation, or scrollback
handling, so merely wheeling over an inactive pane changes focus.

## Architecture

Separate pane hit-testing from pane focusing. Wheel handling first resolves a
typed hit: either a pane surface or the active window-right scrollbar overlay.
A pane-surface hit constructs a target-aware wheel context. The context owns
the target pane id and its `PaneRenderRect`, and provides access to either the
active or an inactive pane's runtime, pane-local UI state (including the stable
viewport), selection and overlay snapshot state, PTY writer, and mouse
coordinates local to that pane. It is the sole source of pane-dependent state
for that event. A scrollbar hit is distinct and has no pane-local cell or pixel
coordinate.

Both coordinate protocols are pane-local. Cell coordinates subtract the
matched `PaneRenderRect` row and column. SGR pixel/1016 coordinates subtract
the pane's true pixel origin, derived from `frame_content_pixel_left`,
`terminal_pixel_top`, the rect column, and the rect row below
`terminal_frame_row_offset`; they must not use the window terminal origin for
an inactive split.

Constructing and using the wheel target context must never change focus. The
routing layer itself does not dispatch pane/tab activation or create a new
target; a user-selected binding may still do so through that command's
existing semantics:

- `pane_focus_follows_mouse` remains owned by cursor-move handling.
- Press/click keeps the existing click-to-focus and click-swallow behavior.
- Tab-bar wheel handling retains priority over pane routing.
- The window-right scrollbar is an active-pane overlay and is not subtracted
  from any `PaneRenderRect`. Wheel routing therefore performs an explicit
  `scrollbar_hit_test` before pane hit-testing. A hit runs the existing active
  stable-viewport wheel behavior and stops; it does not become a pane-surface
  target, synthesize or clamp local coordinates, match a user mouse assignment,
  report terminal mouse input, or translate alternate-screen arrows. The pane
  geometrically underneath the overlay is not consulted.
- Zoomed layouts use the one visible pane as the only pane target; split
  separators are not pane targets.

This is a focused wheel-routing layer, not a general rewrite of all mouse
events. Treating `ActiveScrollbar` as an early window-overlay branch is the
concrete preservation of the already-approved active-scrollbar contract, not a
new behavior surface: it keeps the existing active stable-viewport operation,
leaves focus unchanged, and prevents pane-surface protocols from being applied
to a window-level overlay.

## Event flow

Each window wheel event follows this order:

1. Save the previous `current_mouse_wheel_delta`, install the current event's
   delta under a scope guard or closure finalizer, and restore the previous
   value on success, `false`, and every error return.
2. If the pointer is in the tab bar, run the existing tab-wheel path and stop.
3. If the pointer hits the active window-right scrollbar overlay, run the
   existing active stable-viewport wheel behavior and stop without consulting
   the underlying pane or entering assignment/reporting/alternate-arrow paths.
   Otherwise hit-test the visible pane layout without changing focus. If no
   pane surface is hit, return `false`.
4. For a pane-surface hit, convert both cell and pixel positions to coordinates local to the target
   `PaneRenderRect`, then obtain that pane's runtime, UI state,
   selection/overlay state, and writer.
5. Read mouse-reporting mode from the target. If reporting is enabled and not
   bypassed, first scroll that target pane to the bottom and refresh its
   owner-local UI/snapshot state. If reporting is bypassed, do not scroll to
   the bottom; remove the configured bypass modifier bits for assignment
   matching and continue as a non-reporting event.
6. Resolve the custom wheel assignment using the target pane's context,
   effective modifiers, mouse-reporting state, alternate-screen state, and the
   current event's wheel delta. Handle `DisableDefaultAssignment` as the
   suppression control described below; execute any ordinary matched command
   through the target-aware dispatcher.
7. If no assignment matched and reporting remains enabled, encode both cell
   and, for SGR pixel/1016, pixel coordinates relative to the target pane and
   write the report to the target pane's PTY.
8. Otherwise preserve disabled-default-binding behavior. If defaults are
   enabled, translate wheel input in the target pane's alternate screen to
   arrow keys, or scroll that target pane's stable viewport.

Custom mouse actions execute with the hovered pane as their pane context, so
pane-dependent actions and the current wheel delta refer to the target that
received the event. This context alone must not focus the pane. A user
explicitly bound command retains its existing semantics even when those
semantics activate a pane or tab, or create and activate a new pane/tab.

Mouse reports and alternate-screen arrows always use the target pane's PTY
writer, application cursor/keypad modes, and Kitty keyboard state; they must
never fall through to the active pane's writer.

## Target-aware command dispatch

`handle_user_mouse_assignment` currently delegates to
`command_palette_apply_command`, whose viewport, UI, terminal-mode, and writer
operations largely read active-pane fields. The wheel path therefore uses one
target-aware command-dispatch interface rather than temporarily activating the
hovered pane or swapping active fields.

Every implemented `WindowCommand` reachable from a wheel binding is
exhaustively classified by that interface:

- viewport actions such as `ScrollBy*`, `ScrollTo*`, prompt scrolling, and
  current-event delta operate on the target pane's stable viewport;
- terminal writes such as `SendString`, `SendPaste`, `SendKey`, and paste
  commands use the target writer and the target runtime's encoding modes;
- selection, copy, search, copy-mode, quick-select, and other pane overlays use
  the target's pane-local UI and snapshot; global clipboard destinations remain
  global, but their source is the target pane;
- pane-scoped app actions receive the target pane id explicitly, while truly
  window, tab, application, or configuration actions retain their existing
  global semantics; and
- focus and creation commands retain their existing `WindowCommand`/
  `AppAction` effects. This includes `ActivatePaneDirection`,
  `ActivatePaneByIndex`, `ActivatePane1` through `ActivatePane8`, `NextPane`,
  `PreviousPane`, tab activation, `NewTab`, `SplitPane`, and equivalent spawn
  or creation actions.

Direction-relative and target-relative commands use the hovered pane as their
reference pane, matching WezTerm's `perform_key_assignment(&hovered_pane, ...)`
semantics. They must not resolve direction, split source, current-pane domain,
or other pane-relative input from the previously active pane. By-index and
truly global commands keep their established tab/window scope.

Composite actions such as `Multiple` recursively retain the same target. The
classification is a closed/exhaustive match (or an equivalent typed target
dispatcher), so a new pane-dependent command cannot silently fall through to
active-pane behavior. This slice must cover all currently implemented
pane-dependent commands that a wheel binding can invoke; it must not use a
small allowlist with active-pane fallback. Temporary activate/restore and
runtime/UI swapping are forbidden because they expose focus, title, event, and
error-path side effects.

`DisableDefaultAssignment` is a separate mouse-binding control result, not an
ordinary target command. If its full event/modifier/reporting/alternate-screen
predicate matches, do not call the target dispatcher and do not run terminal
reporting, alternate-screen arrow translation, or default scrollback. Restore
the event delta, leave focus unchanged, and return `Ok(false)`, matching the
existing button-binding convention that disabling a default is not reported
as a consumed action. The non-bypassed reporting scroll-to-bottom from event
step 5 still precedes assignment lookup and therefore still applies; no
subsequent default scroll is allowed.

## State refresh

After an inactive pane changes, refresh that pane owner's state in place:

- update its stable viewport against its own terminal;
- re-project its selection and reconcile its overlay against the new viewport;
- rebuild its pane snapshot; and
- rebuild/invalidate the composite window snapshot so the changed pane is
  visible.

This refresh also runs when the mandatory mouse-reporting scroll-to-bottom is
the only mutation, and before returning an error from a later assignment or
PTY write. A target-dirty finalizer may centralize that guarantee.

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
- A pane hit whose owner runtime cannot be resolved returns `Ok(false)` and
  never falls back to the active pane. A resolved runtime with no PTY writer
  preserves the existing disconnected-pane no-op: a selected report,
  alternate-arrow path, or writer action is consumed as `Ok(true)`.
- No matching custom assignment continues to reporting/default handling.
  `DisableDefaultAssignment` follows its special `Ok(false)` suppression path.
  Any ordinary matched assignment is consumed: success returns `Ok(true)`,
  while action or PTY I/O failure is returned as `Err` after target refresh
  and delta restoration. It must not trigger the default action as a second
  behavior.
- Reporting selected but not encodable returns `Ok(false)` without falling
  through to scrollback. Default fallback occurs only when no assignment
  matched, reporting is inactive or bypassed, and default bindings are enabled.
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
- routing mouse-report wheel bytes to the target PTY writer with both local
  cell coordinates and local SGR pixel/1016 coordinates;
- scrolling a reporting target to bottom before assignment/report, including
  inactive owner-local refresh, while bypass leaves that viewport in place and
  follows the non-reporting scrollback path;
- alternate-screen arrow translation through the target writer;
- custom wheel bindings across viewport, writer/paste/key, pane-local
  copy/overlay, pane-scoped, window/global, explicit-focus, and nested action
  categories, all retaining the hovered-pane context;
- direction-relative focus from an inactive hovered pane, by-index focus, and
  representative global and creation actions including `NewTab` and
  `SplitPane`, proving that routing itself does not focus while explicit
  bindings retain their established effects;
- `DisableDefaultAssignment` over an inactive pane in non-reporting,
  reporting, and alternate-screen states: it returns `Ok(false)`, preserves
  focus, emits no report or arrow, performs no default scrollback, and restores
  the current delta;
- successful, unhandled, missing-runtime, missing-writer, assignment-failure,
  and PTY-I/O-error results, including `current_mouse_wheel_delta` restoration
  and target refresh on every error path;
- `pane_focus_follows_mouse` on cursor movement without wheel-induced focus;
- tab-bar priority; active scrollbar early routing with active-left/inactive-right
  and active-right split layouts, including proof that assignments, reports,
  alternate arrows, and synthetic local coordinates are bypassed; zoomed panes;
  split separators; and an inactive pane with no history;
- active-pane wheel compatibility and both enabled and disabled default mouse
  bindings.

Run the focused app-shell tests followed by the complete app/workspace suite.
