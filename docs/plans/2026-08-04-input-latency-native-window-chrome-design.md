# Input Latency and Native Window Chrome Design

## Goal

Make keyboard input return to the native event loop without waiting on PTY I/O, while preserving ordered, lossless pane input. On Windows, keep the integrated borderless title bar and ask the operating system for rounded corners and a native drop shadow.

## Root cause

`NativeWindowApp::handle_keyboard_input_event` currently reaches `write_pty_bytes`, which calls `write_all` and `flush` directly on the winit event-loop thread. ConPTY or another PTY backend can block either operation, so keyboard messages, redraw requests, and other window events wait behind the write.

The default Windows configuration uses integrated title buttons and therefore creates an undecorated winit window. The renderer paints a subtle inner frame, but the operating system is never asked to provide an undecorated-window shadow or a rounded-corner preference.

## Approved approach

Each pane owns a FIFO input queue and a dedicated writer thread. The event-loop thread copies an encoded input payload into the queue and immediately returns. The worker remains the only owner of the blocking PTY writer, performs `write_all` plus `flush` in order, and reports completion or failure back through `WindowUserEvent`. Existing metrics continue to describe completed writes rather than queue time. Pane shutdown transfers the sender and both I/O threads through the existing bounded cleanup/reaper path, so no writer worker is detached or silently leaked.

On Windows, undecorated integrated-titlebar windows opt into winit's native undecorated shadow and request `CornerPreference::Round` after creation. Decorated windows keep the operating-system default. Windows versions that do not support corner preference retain the current square fallback; Linux and macOS behavior is unchanged.

## Error handling and ordering

- The unbounded standard-library channel keeps key, paste, mouse-report, and terminal-response payloads in strict send order without blocking the UI thread.
- A disconnected queue returns `BrokenPipe` to the existing input error path.
- Worker write failures are delivered with pane id and runtime generation, so stale failures from restarted panes are ignored.
- Cleanup drops the queue sender, closes the PTY through the existing session lifecycle, and joins or transfers both reader and writer workers to the process-lifetime reaper within the existing deadline.

## Verification

- A blocking synthetic writer proves UI-side enqueue returns within a small deterministic budget while the worker is blocked.
- A FIFO test proves multiple payloads are written and flushed in order.
- Cleanup tests prove the writer worker remains owned until completion or reaper transfer.
- Windows configuration tests prove only undecorated integrated-titlebar windows request native shadow and rounded corners.
- Focused native-window tests, the workspace suite, formatting checks, and a real Windows screenshot/interaction pass verify the result.
