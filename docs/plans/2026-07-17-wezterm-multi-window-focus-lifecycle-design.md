# WezTerm Multi-Window Focus and Lifecycle Design

**Date:** 2026-07-17

**Status:** Approved

## Goal

Complete the next bounded App Shell v2 slice by making native multi-window
focus transitions and application hiding match the pinned WezTerm behavior.
The implementation must preserve the existing `ActivateWindow`,
`ActivateWindowRelative`, `ActivateWindowRelativeNoWrap`, `SpawnWindow`, and
`MoveToNewWindow` action surfaces while making focus state authoritative,
cross-window consistent, and observable exactly once through Lua and terminal
focus reporting.

## Upstream Contract

The pinned WezTerm source at commit
`093bf6bf2b82b929ed80c04fd54ebc80464f715e` establishes these behaviors:

- `window-focus-changed` is emitted when a GUI window's focus state changes and
  receives the GUI window plus its active pane.
- focus changes are forwarded to the active pane and repaint the window.
- `ActivateWindow(n)` focuses the zero-based GUI window at `n` when present.
- `ActivateWindowRelative(delta)` selects relative to the current GUI window
  and wraps.
- `ActivateWindowRelativeNoWrap(delta)` does not cross either end of the GUI
  window list.
- `Hide` hides or minimizes the current window.
- on macOS, `HideApplication` calls the application-level hide operation.

The project uses winit 0.30.13, whose macOS
`ActiveEventLoopExtMacOS::hide_application()` exposes the required native
application-level operation directly.

## Current Gaps

The current native app has the action parsers and window activation requests,
but its runtime state is not yet a reliable multi-window model:

- every `NativeWindowApp` starts with `window_focused = true` before the OS has
  reported focus;
- `handle_focus_changed` dispatches Lua callbacks, status updates, and PTY focus
  sequences even when the state did not change;
- `NativeWindowManager` does not track which OS window owns focus, so two app
  instances can temporarily claim to be focused;
- newly materialized pending windows are redrawn but are not explicitly shown,
  unminimized, and focused;
- `HideApplication` records an app-local request and minimizes only the current
  window, rather than invoking the macOS application-level hide operation;
- closing a focused window does not clear manager focus state because no such
  manager state exists.

## Approaches Considered

### App-local idempotence only

Initialize each app as unfocused, suppress duplicate transitions, and request
focus after window creation. This is small, but it cannot enforce a single
focused window across multiple `NativeWindowApp` instances.

### Manager-coordinated focus state

Make `NativeWindowManager` the coordinator for OS-window focus ownership while
keeping pane/Lua/PTY side effects inside `NativeWindowApp`. This gives a single
cross-window invariant without introducing mux state. This is the selected
approach.

### Full WezTerm-style GUI/mux window registry

Introduce a GUI window registry tied to mux windows and workspace
reconciliation. This is the eventual direction, but it couples this slice to a
mux implementation that does not exist yet and would make focus correctness
harder to verify independently.

## Architecture

`NativeWindowManager` owns the process-local focus coordinator. It stores an
optional focused winit window id and handles `WindowEvent::Focused` before the
event is delegated to an individual app.

`NativeWindowApp` remains responsible for the effects of a real focus
transition:

- updating `window_focused` and click-to-focus state;
- dispatching the typed native focus handler and bounded-static
  `window-focus-changed` callback;
- refreshing status text;
- writing terminal focus-reporting bytes when DEC focus reporting is enabled;
- requesting redraw through the normal window-event path.

The app transition method becomes idempotent. It reports whether a transition
occurred and performs no callback, status, or PTY work when the requested state
equals the current state.

This separation keeps platform/window ordering in the manager and terminal
semantics in the app.

## Focus Event Flow

All apps start unfocused. Actual focus begins only after a real
`WindowEvent::Focused(true)`.

When the manager receives `Focused(true)` for window B:

1. If window A is recorded as focused and differs from B, submit one synthetic
   `false` transition to A.
2. Submit the real `true` transition to B.
3. Record B as the focused window.
4. Delegate the remaining redraw/event work normally.

If the OS later sends `Focused(false)` for A, A's idempotent transition absorbs
it without a second Lua event or PTY sequence. This handles platform-dependent
focus event ordering while preserving exactly one observable transition.

When the manager receives `Focused(false)` for the recorded focused window, it
clears the manager record and submits the transition. A `false` event for any
other window is still submitted to that app but is normally an idempotent
no-op.

Closing or dropping the recorded focused window clears the manager record. The
manager does not guess which existing window the platform will focus next;
the next real `Focused(true)` establishes the successor.

## Window Materialization and Activation

The startup window is explicitly shown, unminimized, and sent a platform focus
request after successful creation and PTY startup. Focus state is not changed
until the OS confirms it.

Pending windows created by `SpawnWindow` or `MoveToNewWindow` are materialized
in queue order. When multiple windows are pending in one event-loop pass, only
the last successfully materialized window receives the explicit focus request.
This avoids focus thrashing and makes the newest requested window the intended
foreground window.

`ActivateWindow*` keeps the current deterministic ordering by logical
`rssh_core::WindowId`. It shows and unminimizes the selected window, requests
platform focus, and waits for the real focus event before changing observable
state. Out-of-range absolute activation and no-wrap boundary activation remain
no-ops.

## Hide and Application Lifecycle

`Hide` remains window-local and continues to minimize the current window.

`HideApplication` becomes a manager-consumed request:

- on macOS, the manager calls winit's native
  `ActiveEventLoopExtMacOS::hide_application()`;
- on other platforms, the action remains a documented no-op, matching the
  upstream contract that defines this action for macOS;
- the app does not preemptively mutate focus state; resulting OS focus events
  drive the normal transition flow.

`QuitApplication`, close confirmation, `quit_when_all_windows_are_closed`, and
PTY exit behavior keep their current paths. This slice only integrates focus
ownership with those paths by clearing stale manager focus state when a window
is removed.

## Error Handling

- A focus request for a missing or not-yet-materialized target returns false and
  leaves manager state unchanged.
- Failure to materialize a pending window retains the existing event-loop error
  path and does not record it as focused.
- Failure while forwarding focus-reporting bytes is logged through the existing
  window-event error path; manager ownership still follows the OS event so a
  transient PTY write error cannot produce two focused windows.
- Duplicate and out-of-order focus events are accepted and reduced to
  idempotent transitions rather than treated as errors.
- Platform-specific application hiding is guarded with `cfg(target_os =
  "macos")`; non-macOS builds do not import macOS extension traits.

## Testing Strategy

Tests are split between pure transition semantics, app side effects, and
manager lifecycle behavior.

### App transition tests

- a new `NativeWindowApp` is initially unfocused;
- `false -> true -> false` emits exactly two typed focus changes;
- repeated `true` and repeated `false` emit no duplicate typed/Lua events;
- terminal focus reporting writes one `CSI I` and one `CSI O`, with duplicates
  suppressed;
- `mouse_click_may_focus_window` is set only on a real false-to-true transition;
- static `window-focus-changed` status setters observe the exact new state once.

### Manager/coordinator tests

Extract the focus transition decision into a small pure helper keyed by logical
test window ids so it can be tested without constructing native OS windows.
Cover:

- first focus acquisition;
- A-to-B focus transfer and synthetic A blur;
- duplicate B focus;
- late A blur after transfer;
- current-window blur;
- closing focused and non-focused windows;
- no state mutation for an invalid activation target.

Native manager tests continue to verify logical window ordering and the
wrap/no-wrap index helpers.

### Lifecycle and regression tests

- pending-window batching marks only the final materialized window for focus;
- application-hide requests are consumed exactly once;
- macOS compilation covers the native application-hide call while other targets
  compile the no-op path;
- existing `ActivateWindow*`, `SpawnWindow`, `MoveToNewWindow`, notification
  suppression, click swallowing, close, and quit tests remain green;
- `cargo test -p rssh-app` and `cargo test --workspace` are the final gates.

## Non-Goals

- mux/domain window registration or remote-window orchestration;
- pane focus visual redesign;
- pane-local selection or scrollbar polish;
- arbitrary Lua callback execution;
- changing logical window ordering or workspace semantics;
- synthesizing a successor focus target when a focused window closes;
- broader macOS application menu or activation-policy work.

## Completion Criteria

This slice is complete when current-state tests prove:

1. native focus state starts from OS truth and remains exclusive across managed
   windows;
2. every real focus transition produces exactly one Lua/native/status/PTY
   observation;
3. new windows request foreground activation without preemptively claiming
   focus;
4. `ActivateWindow*` preserves its existing selection semantics;
5. macOS `HideApplication` uses the native application-level API;
6. window removal cannot leave a stale focused-window record;
7. focused and full workspace regression suites pass.
