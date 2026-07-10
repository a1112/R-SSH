# Split Resize Cursor Design

## Goal

Match WezTerm's visible split-resize affordance by showing a horizontal or
vertical resize cursor when the pointer is over a pane separator and while that
separator is being dragged.

## Upstream behavior

WezTerm maps a horizontal pane split to a left-right resize cursor and a
vertical pane split to an up-down resize cursor. R-SSH already hit-tests pane
separators and resizes them by dragging, but it leaves the native pointer shape
unchanged.

## Architecture

Track the current native mouse cursor icon alongside the existing cursor
visibility state in `NativeWindowApp`. Route icon changes through one setter so
headless tests can inspect state and a live winit window receives the same
update through `Window::set_cursor`.

Derive the desired split cursor from the existing `PaneSplitResizeDrag`:

- `Left` or `Right` means a horizontal (`EwResize`) cursor.
- `Up` or `Down` means a vertical (`NsResize`) cursor.

On pointer movement, update the icon after calculating the current terminal
cell. An active split drag takes precedence over hit testing so the resize
cursor remains stable when the pointer leaves the one-cell separator. Without
an active drag, use `split_resize_drag_at_mouse_position` to choose a resize
icon or restore the default cursor.

## State transitions

- Enter or move over a separator: show the corresponding resize cursor.
- Move away without dragging: restore the default cursor.
- Press the left button over a separator: begin the existing drag and retain
  the separator's resize cursor.
- Move outside the separator while dragging: keep the resize cursor.
- Release the drag: recompute from the current pointer position, restoring the
  default when it is no longer over a separator.
- Leave the native window: clear pointer coordinates and restore the default
  cursor.

`hide_mouse_cursor_when_typing` continues to control visibility only. Moving
the pointer makes it visible and applies the current icon, while hiding it does
not discard the remembered icon.

## Error handling

Cursor icon updates are infallible winit calls. Missing pointer coordinates,
missing pane layout, a zoomed single pane, or a pointer outside all separators
all resolve to the default cursor. Existing split resize errors and logging are
unchanged.

## Testing

Add headless app tests that verify:

1. Left/right split separators select `EwResize`.
2. Up/down split separators select `NsResize`.
3. Moving away restores the default icon.
4. An active drag retains its resize icon outside the separator.
5. Releasing outside the separator and leaving the window restore the default.
6. Existing separator dragging still changes pane geometry.

Update the WezTerm parity gap document to record the new affordance without
claiming broader App Shell v2 completion.
