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
- `PtySession` supports spawn, read, write, resize, wait, try-wait, kill, child
  exit status, and owned stream extraction for threaded runtime loops.
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
  - Alt+text as ESC-prefixed text
  - arrow keys, Home, End, Insert, Delete, Page Up, Page Down
  - F1 through F12
  - Shift/Alt/Ctrl-modified navigation, editing, and function keys as xterm CSI
    modifier sequences
  - application cursor key mode from PTY-side `ESC[?1h` and `ESC[?1l`
  - application keypad mode from PTY-side `ESC=` and `ESC>` for keypad-tagged
    number/operator keys
- Paste events are forwarded to the PTY as UTF-8 bytes by default. When the
  PTY-side application enables xterm bracketed paste with `ESC[?2004h`, paste
  events are wrapped as `ESC[200~...ESC[201~` until `ESC[?2004l`.
- `rssh-app local --mouse` allows terminal applications to enable and disable
  host mouse capture and focus events through xterm PTY output modes, then
  forwards active reports as xterm SGR mouse and focus sequences. Mouse mode
  granularity follows xterm `1000` button, `1002` button-event, and `1003`
  any-event reporting.
- Resize events are forwarded to the PTY.
- PTY output is streamed to the host console.
- The app answers standard and DEC private cursor-position queries (`ESC[6n`
  and `ESC[?6n`) with the current mirrored terminal cursor position so shells
  and TUI programs can complete position handshakes.
- The app also answers primary device attributes `ESC[c`, secondary device
  attributes `ESC[>c`, and terminal status `ESC[5n` instead of leaking those
  queries to the host console.
- The app answers text-area size query `ESC[18t` with
  `ESC[8;<rows>;<columns>t`.
- `rssh-app local -- <program> [args...]` propagates the child process exit code
  back to the host process.
- After a fast child-process exit, `rssh-app local` briefly drains PTY reader
  output so final command output is not dropped before returning the exit code.
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

Run with mouse/focus reporting enabled:

```powershell
cargo run -p rssh-app -- local --mouse
```

Mouse and focus events are forwarded only after the PTY-side application enables
the relevant xterm modes, such as `ESC[?1000h`, `ESC[?1002h`, `ESC[?1003h`, or
`ESC[?1004h`.

Mouse movement reporting follows the active xterm mode: `1000` reports button
and wheel events, `1002` adds drag events, and `1003` also reports motion
without buttons.

Bracketed paste wrapping follows PTY-side `ESC[?2004h` and `ESC[?2004l`
automatically.

Application cursor key mode follows PTY-side `ESC[?1h` and `ESC[?1l`
automatically for unmodified arrow keys.

Application keypad mode follows PTY-side `ESC=` and `ESC>` automatically. When
the host input reports keypad-origin keys, number/operator keypad keys are sent
as SS3 application-keypad sequences.

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
cargo test -p rssh-pty local_pty_reports_child_exit_status -- --ignored --nocapture
cargo test -p rssh-app local_pty_output_feeds_terminal_grid -- --ignored --nocapture
cargo test -p rssh-app local_app_drains_output_after_fast_child_exit -- --ignored --nocapture
cargo run -p rssh-app -- local -- cmd.exe /C exit 7
```

## Acceptance Metrics

- App startup: `rssh-app local` starts a local shell without crashing.
- PTY round trip: a command written into the spawned shell is observed in PTY
  output within 5 seconds.
- Terminal ingestion: PTY output containing a marker is visible in
  `rssh-terminal` grid state within 5 seconds.
- Input coverage: unit tests cover printable UTF-8, raw paste, bracketed paste,
  Enter, Ctrl+C, arrow key encoding, application cursor keys, modified
  navigation/editing/function keys, Alt+text, Shift+Tab, F1-F12, SGR mouse, and
  focus events.
- Exit propagation: real PTY smoke tests cover non-zero child exit status.
- Fast-exit output drain: ignored integration tests repeatedly run
  `rssh-app local --mouse -- <echo command>` and verify the final output marker
  is present every time.
- Control-sequence response: unit tests cover normal output, dynamic `ESC[6n`
  and `ESC[?6n`, `ESC[c`, `ESC[>c`, `ESC[5n`, `ESC[18t`, and split
  response-query chunks.
- Mouse/focus negotiation: unit tests cover split and combined PTY mode
  sequences for xterm mouse and focus reporting, including `1000`/`1002`/`1003`
  reporting granularity.
- Bracketed paste negotiation: unit tests cover xterm `ESC[?2004h/l` tracking
  and wrapped paste encoding.
- Application cursor key negotiation: unit tests cover xterm `ESC[?1h/l`
  tracking and SS3 arrow-key encoding.
- Application keypad negotiation: unit tests cover xterm/VT `ESC=` and `ESC>`
  tracking plus SS3 keypad encoding for keypad-tagged input.
- Regression gate: workspace tests and clippy must pass before merging.

## Explicit Non-Scope

- Native GPU window.
- Full VT/xterm compatibility.
- Scrollback.
- Clipboard.
- Tab/session profile UI.
- SSH network connection.

## Next Milestone

MVP 3 should replace the console-hosted display with the first native window and
renderer path:

1. Add a `winit` app shell.
2. Feed PTY output into `rssh-terminal` continuously.
3. Render terminal grid cells through the renderer boundary.
4. Keep the current PTY integration tests as the runtime smoke gate.
