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
- `PtyCommand` sets `TERM=xterm-256color` and `COLORTERM=truecolor` by default
  for spawned PTY child processes, with explicit per-command overrides
  available before spawn.
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
- PTY-side input modes are tracked through the shared app mode tracker, so the
  console path recognizes both 7-bit CSI (`ESC[?…h/l`) and 8-bit C1 CSI
  (`0x9b ? … h/l`) private mode toggles. Mode-like bytes embedded inside
  unrelated OSC or ST-terminated control-string payloads are ignored, including
  when the payload is split across PTY chunks.
- The same mode tracker recognizes ANSI insert/replace mode (`CSI 4 h/l`),
  including the 8-bit C1 CSI form.
- The console path answers DECRQM private-mode status queries
  (`CSI ? <mode> $ p`) for tracked terminal modes, including application cursor
  keys (`1`), origin mode (`6`), auto-wrap (`7`), cursor visibility (`25`),
  alternate-screen modes (`47`/`1047`/`1049`), private cursor save/restore
  (`1048`), mouse reporting (`1000`/`1002`/`1003`), SGR mouse (`1006`), focus
  reporting (`1004`), bracketed paste (`2004`), and synchronized output
  (`2026`). `RIS` (`ESC c`) resets tracked mode state to defaults. Unknown
  modes return an xterm-style unknown status.
- The console path also answers ANSI DECRQM mode status queries
  (`CSI <mode> $ p`) for insert/replace mode (`4`), including the C1 CSI form;
  unknown ANSI modes return an xterm-style unknown status.
- The console output filter handles xterm synchronized output
  (`ESC[?2026h/l`) by consuming the mode markers, buffering visible host-console
  writes while the mode is enabled, continuing to update its mirror terminal and
  answer terminal queries, and flushing buffered bytes when the mode resets,
  when `RIS` resets terminal modes, or when the PTY output stream ends.
- `rssh-app local --mouse` allows terminal applications to enable and disable
  host mouse capture and focus events through xterm PTY output modes, then
  forwards active reports as xterm mouse and focus sequences. Mouse mode
  granularity follows xterm `1000` button, `1002` button-event, and `1003`
  any-event reporting. Mouse encoding uses legacy `CSI M` by default and SGR
  extended coordinates after `ESC[?1006h`.
- Resize events are forwarded to the PTY.
- PTY output is streamed to the host console.
- `rssh-app local --log PATH` writes visible terminal output to a session log
  file while still streaming raw output to the host console; non-visible control
  sequences such as OSC title updates and BEL are omitted from the
  visible-output log.
- The console output filter holds incomplete OSC control strings across PTY
  chunks before writing them to the host console, and drops incomplete OSC
  control strings during EOF flush so half-written OSC bytes do not pollute the
  host console.
- Terminal-query matching in the console output filter does not inspect inside
  OSC or ST-terminated control-string payloads, so query-like bytes embedded in
  titles, DCS, SOS, PM, or APC content are passed through as payload rather than
  answered as terminal probes.
- The console output filter also holds incomplete CSI control sequences across
  PTY chunks before writing them to the host console, and drops incomplete CSI
  sequences during EOF flush.
- The same chunk-boundary handling applies to ST-terminated DCS/SOS/PM/APC
  control strings, preventing half-written control strings from reaching the
  host console before their ST terminator arrives.
- The app answers standard and DEC private cursor-position queries (`ESC[6n`
  and `ESC[?6n`) with the current mirrored terminal cursor position so shells
  and TUI programs can complete position handshakes. Equivalent 8-bit C1 CSI
  query forms (`0x9b 6n` and `0x9b ? 6n`) are handled the same way.
- The app also answers primary device attributes `ESC[c`, secondary device
  attributes `ESC[>c`, and terminal status `ESC[5n` instead of leaking those
  queries to the host console; equivalent C1 CSI forms are also answered.
- The app answers text-area size query `ESC[18t` with
  `ESC[8;<rows>;<columns>t` and screen character-size query `ESC[19t` with
  `ESC[9;<rows>;<columns>t`, including equivalent C1 CSI query forms.
- The app answers xterm window pixel-size query `ESC[14t` with
  `ESC[4;<pixel-height>;<pixel-width>t`, window-state query `ESC[11t` with
  `ESC[1t`, window-position query `ESC[13t` with `ESC[3;0;0t`, screen
  pixel-size query `ESC[15t` with `ESC[5;<pixel-height>;<pixel-width>t`, and
  character-cell pixel-size query `ESC[16t` with `ESC[6;16;8t`; equivalent C1
  CSI forms are handled too.
- The app answers xterm icon-label query `ESC[20t` and window-title query
  `ESC[21t` from the mirrored terminal title state, including equivalent C1 CSI
  forms.
- The app answers xterm OSC color queries for default foreground (`OSC 10;?`),
  default background (`OSC 11;?`), cursor color (`OSC 12;?`), and indexed
  palette colors (`OSC 4;<n>;?`) using the current tracked OSC color state,
  falling back to the built-in xterm-compatible palette. OSC `10`, `11`, `12`,
  and `4` color-setting sequences update the tracked state. OSC `110`, `111`,
  and `112` reset dynamic foreground, background, and cursor color, and
  `OSC 104` resets one, multiple, or all indexed palette overrides. BEL, ST,
  and C1 ST terminators are preserved in responses. OSC color-setting bytes
  embedded inside unrelated OSC or ST-terminated control-string payloads are
  ignored by the color tracker, including when the payload is split across PTY
  chunks.
- The console path handles OSC 52 clipboard writes and read queries, decoding
  PTY-side base64 clipboard payloads into the system clipboard and answering
  `?` queries with base64-encoded clipboard content. Both 7-bit OSC 52
  (`ESC]52;...`) and C1 OSC 52 (`0x9d52;...`) forms are recognized, including
  BEL, ST, and C1 ST terminators. OSC 52 control sequences are removed from
  console display output. If PTY output ends in an incomplete OSC 52 sequence
  or partial OSC 52 prefix, the pending control bytes are dropped during flush
  instead of leaking to the host console. OSC 52-like bytes embedded inside
  split ST-terminated control-string payloads are not treated as clipboard
  operations.
- OSC 8 hyperlink sequences are consumed by the console output filter and fed
  into the mirrored terminal state, so hyperlink metadata is preserved without
  writing OSC 8 control bytes to the host console. Both 7-bit OSC 8 and C1 OSC
  8 forms are recognized, including split C1 OSC 8 payloads. If PTY output ends
  in an incomplete OSC 8 sequence or partial OSC 8 prefix, the pending control
  bytes are dropped during flush instead of leaking to the host console.
- `rssh-app local --osc52 off|write|read-write` controls whether PTY-side OSC
  52 clipboard writes and read queries are allowed. SSH sessions that use the
  OpenSSH-backed console runtime inherit the same policy through
  `rssh-app ssh --osc52 ...`.
- The app answers xterm XTGETTCAP terminal-capability queries
  (`DCS + q <hex-cap> ST`) for common compatibility probes, including
  `Co`/`colors = 256`, `TN = xterm-256color`, `RGB = RGB`, `Tc = 1`, `Ms` OSC
  52 clipboard template support, `sitm`/`ritm` italic style templates, `Smulx`
  styled underline, `Setulc` underline color, tmux/xterm cursor templates
  (`Cr`/`Cs`/`Se`/`Ss`), foundational cursor/screen/style capabilities
  (`clear`, `cup`, `home`, `civis`/`cnorm`, `smcup`/`rmcup`, `sgr0`, common
  SGR styles, `smul`/`rmul`, `setaf`/`setab`), and dynamic `co`/`li` column
  and row counts from the current PTY size. Unsupported capabilities return
  `DCS 0+r ST`, and C1 DCS/ST forms are handled too.
- The app answers DEC request status string queries (`DECRQSS`,
  `DCS $ q <selector> ST`) for current SGR style (`m`), including bold, faint,
  italic, blink, underline, double underline, colon-separated underline styles,
  conceal, strikethrough, overline, inverse video, foreground/background colors,
  and underline color; cursor shape (`SP q`); and scrolling region (`r`),
  preserving ST versus C1 ST
  response terminators and returning an invalid status for unsupported
  selectors.
- The app answers xterm version queries (`CSI > q` and `CSI > 0 q`) with a
  `DCS > | R-SSH <version> ST` response, including equivalent C1 CSI forms, so
  terminal programs can complete XTVERSION handshakes without leaking the query
  to the host console.
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

Show the console startup options:

```powershell
cargo run -p rssh-app -- local --help
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

Restrict OSC 52 clipboard access to writes only:

```powershell
cargo run -p rssh-app -- local --osc52 write
```

Write a session log:

```powershell
cargo run -p rssh-app -- local --log session.log -- powershell -NoProfile -Command "Write-Output logged-smoke"
```

Mouse and focus events are forwarded only after the PTY-side application enables
the relevant xterm modes, such as `ESC[?1000h`, `ESC[?1002h`, `ESC[?1003h`, or
`ESC[?1004h`.

Mouse movement reporting follows the active xterm mode: `1000` reports button
and wheel events, `1002` adds drag events, and `1003` also reports motion
without buttons.

Mouse coordinate encoding follows `ESC[?1006h` / `ESC[?1006l`: SGR mouse is
used while `1006` is enabled, otherwise the legacy `CSI M` form is used.

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
  navigation/editing/function keys, Alt+text, Shift+Tab, F1-F12, legacy and SGR
  mouse, and focus events.
- Terminal environment: unit tests cover default `TERM` and `COLORTERM` values,
  explicit overrides, and propagation into the PTY command builder.
- Exit propagation: real PTY smoke tests cover non-zero child exit status.
- Fast-exit output drain: ignored integration tests repeatedly run
  `rssh-app local --mouse -- <echo command>` and verify the final output marker
  is present every time.
- Control-sequence response: unit tests cover normal output, dynamic `ESC[6n`
  and `ESC[?6n`, `ESC[c`, `ESC[>c`, `ESC[5n`, `ESC[11t`, `ESC[13t`,
  `ESC[14t`, `ESC[15t`, `ESC[16t`, `ESC[18t`, `ESC[19t`, `ESC[20t`,
  `ESC[21t`, and split response-query chunks. C1 CSI equivalents for cursor,
  device/status, and size/window queries are also covered. Unit tests also
  cover query-like CSI bytes inside OSC control-string payloads so those bytes
  are not mistaken for standalone terminal probes.
- OSC query response: unit tests cover default foreground/background, cursor,
  and indexed palette color queries, including BEL, ST, and C1 OSC/ST forms.
  Unit tests also cover color-setting sequences followed by matching queries,
  dynamic foreground/background reset with `OSC 110`/`OSC 111`, cursor-color
  reset with `OSC 112`, indexed-palette reset with `OSC 104`, stream-ordered
  color response state, and color-setting bytes embedded inside ST-terminated
  control-string payloads split across PTY chunks.
- OSC 52 clipboard: unit tests cover console-path clipboard writes and
  clipboard query responses without writing OSC 52 control bytes to console
  output, including C1 OSC 52 write/query forms and split C1 OSC 52 payloads,
  plus `off` and `write` policy enforcement. Unit tests also cover OSC 52-like
  bytes embedded inside unrelated control-string payloads. EOF flushing is
  covered for incomplete OSC 52 sequences and partial prefixes.
- OSC 8 hyperlinks: unit tests cover full and split OSC 8 sequences, including
  C1 OSC 8 forms, verifying that console output omits the control bytes while
  the mirrored terminal keeps hyperlink metadata on linked cells. EOF flushing
  is covered for incomplete OSC 8 sequences and partial prefixes so half-written
  control bytes do not reach the host console.
- XTGETTCAP response: unit tests cover DCS and C1 DCS terminal-capability
  queries for colors, terminal name, true-color markers, OSC 52 clipboard
  template, italic style templates, styled/colored underline templates,
  tmux/xterm cursor style and cursor color templates, current columns/rows,
  foundational cursor/screen/style/color capabilities, and unknown capability
  fallback.
- DECRQSS response: unit tests cover current SGR, including faint, italic,
  blink, double underline, colon-separated underline style, underline color, and
  concealed text plus overline, cursor-shape, and scroll-region status queries
  in both DCS and C1 DCS forms.
- XTVERSION response: unit tests cover 7-bit and C1 CSI version queries and
  verify the query bytes are not written to visible output.
- Session logging: unit tests cover teeing visible terminal output to a log
  writer, omitting non-visible control sequences from the log while preserving
  them for the host console, and smoke checks can verify `--log` writes command
  output to disk.
- OSC chunking: unit tests cover split OSC title sequences so incomplete OSC
  control strings are held until terminated, plus EOF flushing for incomplete
  OSC control strings.
- CSI chunking: unit tests cover split and incomplete CSI sequences so ANSI
  control sequences are held until their final byte and dropped if EOF arrives
  first.
- ST-string chunking: unit tests cover split and incomplete DCS control strings
  so ST-terminated control strings are held until terminated and dropped if EOF
  arrives first.
- Mouse/focus negotiation: unit tests cover split and combined PTY mode
  sequences for xterm mouse and focus reporting, including `1000`/`1002`/`1003`
  reporting granularity, `1006` SGR protocol toggling, and C1 CSI private mode
  input toggles. Unit tests also cover mode-like bytes inside OSC and
  ST-terminated control-string payloads, plus DECRQM private-mode status
  queries for tracked input, display, alternate-screen/private-cursor, reset,
  and unknown modes.
- ANSI mode negotiation: unit tests cover insert/replace (`CSI 4 h/l`) tracking
  and ordinary DECRQM status queries (`CSI 4 $ p`) in both 7-bit and C1 CSI
  forms.
- Bracketed paste negotiation: unit tests cover xterm `ESC[?2004h/l` tracking
  and wrapped paste encoding.
- Synchronized output negotiation: unit tests cover xterm `ESC[?2026h/l`
  tracking, DECRQM status responses on the shared console/runtime path,
  console-side visible-output buffering until explicit reset, `RIS` reset, and
  EOF flushing.
- Application cursor key negotiation: unit tests cover xterm `ESC[?1h/l`
  tracking and SS3 arrow-key encoding.
- Application keypad negotiation: unit tests cover xterm/VT `ESC=` and `ESC>`
  tracking plus SS3 keypad encoding for keypad-tagged input.
- Regression gate: workspace tests and clippy must pass before merging.

## Explicit Non-Scope

- Native GPU window.
- Full VT/xterm compatibility.
- Scrollback.
- Tab/session profile UI.
- SSH network connection.

## Next Milestone

MVP 3 should replace the console-hosted display with the first native window and
renderer path:

1. Add a `winit` app shell.
2. Feed PTY output into `rssh-terminal` continuously.
3. Render terminal grid cells through the renderer boundary.
4. Keep the current PTY integration tests as the runtime smoke gate.
