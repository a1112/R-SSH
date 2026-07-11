# Active Pane Split Scrollbar Design

## Goal

Match WezTerm's current split-pane scrollbar behavior: when scrollbars are
enabled, keep one scrollbar at the right edge of the window and bind it to the
active pane even while a split layout is visible.

## Upstream behavior

Pinned WezTerm deliberately has a single scrollbar rather than one scrollbar
per pane. Its pane renderer draws that scrollbar only for the active pane and
uses the active pane's viewport for scrolling. The upstream source calls a
future per-pane scrollbar a TODO, so R-SSH must not add one scrollbar to every
pane as part of this parity slice.

R-SSH already has the same single-scrollbar rendering and input model, but
`NativeWindowApp::scrollback_scrollbar` currently returns `None` whenever a
visible split layout exists. That guard hides otherwise working behavior.

## Architecture

Keep the existing window-level `ScrollbackScrollbar`. Remove only the split
layout exclusion from `NativeWindowApp::scrollback_scrollbar`, leaving the
`enable_scroll_bar` check intact.

The method already reads history length, row count, and scrollback offset from
the currently installed runtime. Pane focus changes save the previous pane's
runtime state and install the newly active pane's runtime, so the existing data
flow naturally makes the scrollbar follow the active pane without new shared
state or renderer changes.

The existing framebuffer render path continues to place the thumb at the
window's right edge. Existing scrollbar hit testing and dragging continue to
update `self.scrollback_offset`, which belongs to the active runtime and is
saved back to that pane when focus changes.

## Behavior and boundaries

- With `enable_scroll_bar = true`, show the scrollbar whenever the active pane
  has enough history to produce one, including in a split layout.
- Switching active panes changes the scrollbar geometry to reflect the new
  pane's history and saved offset.
- Clicking or dragging the scrollbar changes only the active pane's offset;
  inactive panes retain their saved offsets.
- With scrollbars disabled or no scrollable history, keep returning `None`.
- Keep exactly one window-right scrollbar. Do not add pane-local tracks or
  change pane layout sizing.

## Error handling

No new fallible operations are introduced. Invalid or empty scrollbar
geometry continues to resolve to `None` through `ScrollbackScrollbar::new`.
Existing bounds checks for frame dimensions and pointer coordinates remain the
input safety boundary for hit testing and dragging.

## Testing

Add focused headless tests that verify:

1. A split layout no longer suppresses an enabled scrollbar for an active pane
   with history.
2. Focusing another pane makes the scrollbar reflect that pane's independent
   history and saved offset.
3. Clicking or dragging the window-right scrollbar updates the active pane and
   leaves the inactive pane's stored offset unchanged.
4. Existing single-pane rendering and disabled/no-history behavior remain
   unchanged.

Update the WezTerm parity gap document to record this exact behavior without
claiming per-pane scrollbar support or broader App Shell v2 completion.
