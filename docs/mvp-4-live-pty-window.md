# MVP 4: Live PTY Window

MVP 4 connects the native renderer to a live local PTY session. The window is no
longer a static renderer demo: PTY output is read on a background thread, fed
through the shared terminal runtime, converted into render snapshots, and drawn
in the native `winit` window.

## Completed Scope

- `rssh-app::terminal_runtime` owns the shared path from PTY bytes to
  `rssh-terminal::Terminal`.
- The runtime filters terminal cursor position, device-attribute, status,
  text-area size, and screen character-size queries, then returns responses
  that are written back to the PTY. Standard and DEC private cursor-position
  responses use the current terminal grid cursor.
- `rssh-app::terminal_input` owns terminal key encoding for text, control keys,
  navigation keys, and common editing keys.
- `rssh-app local` reuses the shared key encoder instead of maintaining a
  separate input mapping.
- `rssh-app window` starts the platform default shell in a local PTY.
- PTY output is read on a background thread and delivered to the UI thread with
  `winit::EventLoopProxy`.
- PTY output updates the terminal runtime and rebuilds
  `TerminalRenderSnapshot` from the live terminal, including visible cursor
  state.
- Native window title follows OSC `0`/`2` title updates from the active shell.
- `winit` keyboard events are encoded and written to the active PTY writer,
  including Alt-prefixed text and Shift/Alt/Ctrl-modified navigation,
  editing, and function keys.
- The native window tracks PTY-side application cursor key mode (`ESC[?1h/l`)
  and sends SS3 arrow-key sequences while it is enabled.
- `winit` resize events are converted to terminal cell geometry; the live
  terminal grid, PTY size, render buffer, and text-area size query response are
  updated together.
- `rssh-app window --frames N` still works as an automated native-window smoke
  check.

## Run

Open the live native PTY window:

```powershell
cargo run -p rssh-app
```

Equivalent explicit command:

```powershell
cargo run -p rssh-app -- window
```

Automated window smoke:

```powershell
cargo run -p rssh-app -- window --frames 3
```

Console-hosted local PTY remains available:

```powershell
cargo run -p rssh-app -- local
```

## Verification

Default checks:

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Native window smoke:

```powershell
cargo run -p rssh-app -- window --frames 3
```

MVP 4 tests cover:

- window text, control, navigation, Alt-text, and modified key encoding
- console path reuse of the shared key encoder
- PTY output feeding into the shared terminal runtime
- terminal runtime resize updates the grid and text-area size response
- terminal response filtering for dynamic cursor position, device-attribute, status,
  text-area size, and screen character-size queries
- native window title state from OSC `0`/`2` PTY output
- application cursor key mode tracking for native window input
- window pixel dimensions are converted to terminal rows and columns
- native window snapshot rebuilds from runtime output, including cursor state

## Metrics Design

The current MVP uses tests and smoke checks as completion gates. The next
instrumentation layer should record these metrics:

- Time to first PTY byte: process spawn to first output chunk.
- Time to first rendered PTY cell: process spawn to first non-empty snapshot.
- PTY chunk processing time: bytes received to terminal grid update.
- Render time per frame: snapshot to `pixels.render()` completion.
- Input write latency: key event received to PTY writer flush.
- Steady idle CPU: open shell with no input.
- Burst throughput: sustained bytes parsed and rendered per second.
- Memory footprint: baseline window, active shell, and future scrollback sizes.

Recommended MVP 5 targets:

- First rendered PTY cell under 500 ms on a normal local shell.
- PTY chunk processing p95 under 2 ms for 8 KiB chunks.
- Frame render p95 under 16 ms at the default 80x24 grid.
- Idle CPU under 3% after the shell prompt is visible.

## Explicit Non-Scope

- SSH protocol sessions in the native window.
- Scrollback rendering and search.
- Selection.
- Mouse input.
- GPU text shaping, glyph atlas caching, and font fallback.
- Persistent session profiles.

## Next Milestone

MVP 5 should replace the minimal bitmap-font renderer with a production-grade
text rendering path and add basic terminal UX:

1. Track dirty regions instead of rebuilding the full snapshot each chunk.
2. Add scrollback storage and viewport rendering.
3. Start collecting the metrics listed above in smoke and benchmark commands.
