# MVP 2: Local Terminal Path

MVP 2 turns the terminal core into a local, runnable terminal path. It is still a
console-hosted prototype, not the final GPU desktop window, but it proves the
critical runtime chain: app input -> PTY -> local shell -> terminal byte stream
-> terminal grid.

## Completed Scope

- `rssh-pty` uses `portable-pty` to open the platform PTY backend.
- Windows uses ConPTY through the same `PtySession` boundary.
- `PtyCommand` models default shell, custom program, arguments, and working
  directory.
- `PtySize` validates terminal dimensions.
- `PtySession` supports spawn, read, write, resize, wait, kill, and owned stream
  extraction for threaded runtime loops.
- `rssh-app local` starts the default platform shell in a PTY.
- When no explicit size is provided, `rssh-app local` sizes the PTY from the
  current host console.
- `rssh-app local --cols N --rows N` starts with an explicit PTY size.
- `rssh-app local -- <program> [args...]` starts a custom program in the same
  PTY path.
- Keyboard input is encoded for common terminal keys:
  - UTF-8 text
  - Enter, Backspace, Tab, Escape
  - Shift+Tab
  - Ctrl+Space, Ctrl+A through Ctrl+Z, and common control-symbol keys
  - arrow keys, Home, End, Insert, Delete, Page Up, Page Down
  - F1 through F12
- Resize events are forwarded to the PTY.
- PTY output is streamed to the host console.
- The app answers the basic cursor-position query `ESC[6n` with `ESC[1;1R` so
  Windows shells can finish startup handshakes.
- A real PTY integration test feeds local shell output into `rssh-terminal` and
  asserts the terminal grid receives the marker text.

## Run

From the repository root:

```powershell
cargo run -p rssh-app -- local
```

Run a specific local program through the PTY:

```powershell
cargo run -p rssh-app -- local -- cmd.exe
```

Run with a fixed PTY size:

```powershell
cargo run -p rssh-app -- local --cols 120 --rows 30
```

## Verification

Default checks:

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Real local PTY smoke checks:

```powershell
cargo test -p rssh-pty local_pty_supports_interactive_shell_roundtrip -- --ignored --nocapture
cargo test -p rssh-app local_pty_output_feeds_terminal_grid -- --ignored --nocapture
```

## Acceptance Metrics

- App startup: `rssh-app local` starts a local shell without crashing.
- PTY round trip: a command written into the spawned shell is observed in PTY
  output within 5 seconds.
- Terminal ingestion: PTY output containing a marker is visible in
  `rssh-terminal` grid state within 5 seconds.
- Input coverage: unit tests cover printable UTF-8, Enter, Ctrl+C, and arrow
  key encoding, plus Shift+Tab and F1-F12.
- Control-sequence response: unit tests cover normal output, `ESC[6n`, and
  split `ESC[6n` chunks.
- Regression gate: workspace tests and clippy must pass before merging.

## Explicit Non-Scope

- Native GPU window.
- Full VT/xterm compatibility.
- Scrollback.
- Mouse reporting.
- Clipboard and bracketed paste.
- Tab/session profile UI.
- SSH network connection.

## Next Milestone

MVP 3 should replace the console-hosted display with the first native window and
renderer path:

1. Add a `winit` app shell.
2. Feed PTY output into `rssh-terminal` continuously.
3. Render terminal grid cells through the renderer boundary.
4. Keep the current PTY integration tests as the runtime smoke gate.
