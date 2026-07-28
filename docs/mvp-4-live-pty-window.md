# MVP 4: Live PTY Window

MVP 4 connects the native renderer to a live local PTY session. The window is no
longer a static renderer demo: PTY output is read on a background thread, fed
through the shared terminal runtime, converted into render snapshots, and drawn
in the native `winit` window.

## Completed Scope

- `rssh-app::terminal_runtime` owns the shared path from PTY bytes to
  `rssh-terminal::Terminal`.
- The runtime filters terminal cursor position, device-attribute, status,
  window state/position, window/screen pixel-size, character-cell pixel-size,
  text-area size, screen character-size, icon-label, window-title, and OSC color
  queries, plus xterm XTGETTCAP terminal-capability and DECRQSS state queries,
  plus XTVERSION queries, then returns responses that are written back to the
  PTY. XTGETTCAP and DECRQSS responses handle 7-bit DCS, UTF-8 C1 DCS/ST, and
  legacy raw C1 DCS/ST query framing. XTGETTCAP responses include modern
  style/color templates, dynamic
  `co`/`li` plus official `cols`/`lines` column and row counts from the current
  runtime size, `it=8` tab interval, and `pairs=32767`,
  tmux/xterm cursor style and cursor color templates, and
  foundational cursor/screen/style/color capabilities such as `clear`, `cup`,
  `home`, `civis`/`cnorm`/`cvvis`, WezTerm `smcup`/`rmcup` with
  alternate-screen and title-stack save/restore, WezTerm `sgr`/`sgr0`, common
  SGR and standout styles, conditional `setaf`/`setab`, and the WezTerm SGR
  mouse templates (`kmous`/`XM`/`xm`). Standard and DEC private
  cursor-position responses use the
  current terminal grid cursor.
  Equivalent UTF-8 C1 CSI query forms are handled through the same runtime path,
  with legacy raw C1 CSI compatibility retained.
  Runtime query matching does not inspect
  inside OSC or ST-terminated control-string payloads, so query-like bytes
  embedded in title, DCS, SOS, PM, or APC content are not answered as standalone
  terminal probes. Incomplete OSC and ST-terminated control strings are retained
  across PTY chunks so that split payloads keep the same protection.
- `rssh-app::terminal_input` owns terminal key encoding for text, control keys,
  navigation keys, and common editing keys.
- `rssh-app::terminal_modes` owns shared PTY-side input mode tracking for the
  console and native-window runtimes, including 7-bit CSI, UTF-8 C1 CSI, and
  legacy raw C1 CSI private mode toggles, ANSI insert/replace mode
  (`CSI 4 h/l`), DECRQM
  private-mode status query reporting for tracked input, screen reverse-video,
  cursor visibility, cursor blinking, Meta-key, auto-wrap, origin,
  alternate-screen, and private cursor save modes, plus ANSI mode status query
  reporting for insert/replace mode (`CSI 4 $ p`). `RIS` resets tracked mode
  state to defaults.
  Mode-like bytes embedded inside unrelated OSC or ST-terminated control-string
  payloads are ignored, including split payloads.
- `rssh-app local` reuses the shared key encoder instead of maintaining a
  separate input mapping.
- `rssh-app window` starts the platform default shell in a local PTY,
  `rssh-app start` is a WezTerm-style alias for the same native-window startup
  path, and
  `rssh-app window <program> [args...]`,
  `rssh-app window -- <program> [args...]`, `rssh-app window -e <program>
  [args...]`, or `rssh-app start <program> [args...]` starts a custom command
  in the same native window runtime.
- `rssh-app window --cwd PATH` and `rssh-app start --cwd PATH` set the initial
  child process working directory for the default shell or custom command.
- `rssh-app window --workspace NAME` and `rssh-app start --workspace NAME` name
  the initial app-shell workspace
  instead of using the default `default` workspace.
- `rssh-app window --class CLASS` and `rssh-app start --class CLASS` request the
  native window class name on Windows. X11/Wayland class/app-id application
  remains future parity work.
- `rssh-app window --position X,Y`, `--position screen:X,Y`,
  `--position main:X,Y`, `--position active:X,Y`, and
  `--position <monitor>:X,Y` request an initial native window screen position.
  `main:` is relative to the primary monitor origin, `active:` is relative to
  the active monitor when the platform exposes one and otherwise falls back to
  the primary monitor origin, and named monitor forms such as `HDMI-1:10,20`
  are relative to the matching monitor origin.
- `rssh-app window` and `rssh-app start` accept WezTerm startup compatibility flags
  `--no-auto-connect`, `--always-new-process`, and `--new-tab` as no-ops while
  R-SSH has no GUI daemon or auto-connected mux domains.
- `rssh-app window --domain local` and `rssh-app start --domain local` select
  the current local PTY domain, and `--attach` is accepted as a no-op while
  R-SSH has no mux domain attachment yet. Remote or named mux domains remain
  future parity work.
- Native-window PTY child processes inherit the shared `PtyCommand` terminal
  environment defaults: `TERM=xterm-256color` and `COLORTERM=truecolor`.
- PTY output is read on a background thread and delivered to the UI thread with
  `winit::EventLoopProxy`.
- PTY output updates the terminal runtime and rebuilds
  `TerminalRenderSnapshot` from the live terminal, including visible cursor
  state, cursor shape, xterm 256-color indexed cell colors, and OSC 8
  hyperlink metadata. UTF-8 C1 OSC/ST and legacy raw C1 OSC 8 hyperlinks are
  tracked without exposing their control bytes as visible output.
- The native window can activate OSC 8 hyperlink cells with `Ctrl` + left
  click when PTY mouse reporting is inactive, opening the URL through the
  platform default handler.
- The command palette exposes WezTerm-style `CompleteSelection`,
  `OpenLinkAtMouseCursor`, and `CompleteSelectionOrOpenLinkAtMouseCursor`: an
  active mouse selection is completed into ClipboardAndPrimarySelection,
  otherwise the OSC 8 hyperlink under the mouse cursor is opened through the
  same open-uri hook/default opener path used by ctrl-click.
- Native window title follows OSC `0`/`2` title updates from the active shell.
- `winit` keyboard events are encoded and written to the active PTY writer,
  including Alt-prefixed text and Shift/Alt/Ctrl-modified navigation,
  editing, and function keys.
- The native window supports clipboard paste through WezTerm-style `Super+V`,
  `Ctrl+Shift+V`, the dedicated `Paste` key, and `Shift+Insert`; pasted text is
  wrapped with bracketed paste markers while the PTY has enabled `ESC[?2004h`.
  Plain `Ctrl+V` remains available to the active PTY application.
- The native window handles PTY-side OSC 52 clipboard writes by default,
  decoding base64 payloads into the system clipboard while ignoring `?` read
  queries in the default WezTerm-style write-only policy. Explicit
  `--osc52 read-write` enables query responses with base64-encoded clipboard
  content for compatibility. The shared runtime recognizes 7-bit OSC
  52 (`ESC]52;...`), UTF-8 C1 OSC/ST (`U+009D`/`U+009C`), and legacy raw C1 OSC
  52 (`0x9d52;...`) forms. OSC 52-like bytes embedded inside unrelated OSC or
  ST-terminated control-string payloads are ignored by the clipboard tracker,
  including split payloads.
- `rssh-app window --osc52 off|write|read-write` controls whether PTY-side
  OSC 52 clipboard writes and read queries are allowed; the default is `write`.
- The native window supports basic local text selection when PTY mouse
  reporting is inactive; selected text is highlighted and can be copied with
  `Ctrl+Shift+C` or `Ctrl+Insert`. A double click selects the contiguous
  non-whitespace word under the cursor, and a triple click selects the whole
  visual line.
- The native window supports scrollback search with WezTerm-style `Super+F` and
  `Ctrl+Shift+F`; plain `Ctrl+F` remains available to the active PTY
  application. Search navigation follows the WezTerm search table with
  `Enter`/Down/`Ctrl+N` for the next match, Up/`Ctrl+P` for the previous match,
  PageDown/PageUp page-wise match movement, `Ctrl+R` match-type cycling, and
  `Esc` to exit search mode. Search is literal by default; use
  `literal:<text>` when the text itself starts with a reserved search prefix, or
  `regex:<pattern>` to run a regular expression search, where invalid regex
  input and zero-width regex matches behave as no match. Matches can span visual
  row boundaries across scrollback and the live grid, scroll the viewport into
  history, and use the selection highlight.
- The native window tracks PTY-side application cursor key mode (`ESC[?1h/l`)
  and sends SS3 arrow-key sequences while it is enabled.
- The native window tracks PTY-side application keypad mode (`ESC=` / `ESC>`)
  and sends SS3 keypad sequences for physical numpad keys while it is enabled.
- The native window tracks PTY-side focus reporting (`ESC[?1004h/l`) and
  reports focus gained/lost events back to the active PTY.
- The shared runtime tracks PTY-side synchronized output mode
  (`ESC[?2026h/l`), reports it through DECRQM private-mode status queries, and
  delays render damage while the mode is enabled so the native window can refresh
  once the PTY-side application resets the mode or sends `RIS`.
- `winit` resize events are converted to terminal cell geometry; the live
  terminal grid, PTY size, render buffer, and text-area size query response are
  updated together.
- The native window can render the terminal scrollback viewport and mouse-wheel
  events move that viewport up or down through available history.
- `Shift+PageUp` and `Shift+PageDown` navigate the native scrollback viewport
  while unmodified page/navigation keys remain available to the active PTY
  application.
- The native window draws a right-edge scrollback scrollbar while history is
  available; users can click or drag it to move the viewport, and the thumb
  also moves with mouse-wheel, Shift page/navigation, and search-driven
  viewport changes. The title remains reserved for shell title and search
  status.
- The native window tracks PTY-side xterm mouse modes (`1000`/`1002`/`1003`)
  and extended mouse protocols (`1005`/`1006`/`1015`), then forwards button,
  wheel, drag, and any-motion events as legacy, UTF-8, SGR, or urxvt mouse
  reports when reporting is enabled. `1016` SGR-pixels is not declared because
  the window input path currently reports terminal cells rather than pixel
  coordinates.
- `rssh-app window --metrics` and `--metrics-json` print startup, PTY
  processing, terminal damage, snapshot update/rebuild, full/dirty
  render-frame, PTY input-write, and bell-event counters plus p95 timings when
  the window run exits.
- `rssh-app window --log PATH` writes visible native-window terminal output to
  a session log file, omitting non-visible terminal control sequences such as
  OSC title updates and BEL.
- `rssh-app window --frames N` still works as an automated native-window smoke
  check.
- `rssh-app profile NAME --file PATH` can start a `kind = "window"` TOML
  profile with the same custom command, OSC 52 policy, metrics, frame limit,
  and log options as direct `window` startup. Window profiles export their
  loaded profile name as `RSSH_PROFILE` for `\(session.profileName)` badge
  interpolation.

## Run

Open the live native PTY window:

```powershell
cargo run -p rssh-app
```

Equivalent explicit command:

```powershell
cargo run -p rssh-app -- window
```

Show the native window startup options:

```powershell
cargo run -p rssh-app -- window --help
```

Disable PTY-side OSC 52 clipboard access:

```powershell
cargo run -p rssh-app -- window --osc52 off
```

Automated window smoke:

```powershell
cargo run -p rssh-app -- window --frames 3
```

Automated window smoke with metrics:

```powershell
cargo run -p rssh-app -- window --frames 30 --metrics
cargo run -p rssh-app -- window --frames 30 --metrics-json
```

Run a custom command inside the native window:

```powershell
cargo run -p rssh-app -- window --frames 120 --metrics -- cmd.exe /K echo window-smoke
```

Write a native-window session log:

```powershell
cargo run -p rssh-app -- window --frames 120 --metrics --log window.log -- cmd.exe /K echo window-log-smoke
```

Start the same native window path from a reusable profile:

```powershell
cargo run -p rssh-app -- profile window-smoke --file examples/rssh-profiles.toml
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
cargo run -p rssh-app -- window --frames 120 --metrics -- cmd.exe /K echo window-smoke
cargo run -p rssh-app -- window --frames 120 --metrics-json -- cmd.exe /K echo window-smoke
cargo run -p rssh-app -- window --frames 120 --metrics --log window.log -- cmd.exe /K echo window-log-smoke
cargo run -p rssh-app -- profile window-smoke --file examples/rssh-profiles.toml
```

MVP 4 tests cover:

- window text, control, navigation, Alt-text, and modified key encoding
- native window clipboard paste encoding and paste shortcut detection
- OSC 52 clipboard extraction from PTY output, native window clipboard writes,
  and explicit read-write clipboard query responses, including UTF-8 C1 OSC/ST
  forms, legacy raw C1 OSC 52 write/query forms, and split C1 OSC 52 payloads;
  tests also cover OSC
  52-like bytes inside unrelated OSC and ST-terminated control-string payloads
- C1 CSI cursor, device/status, window state/position, window/screen pixel-size,
  character-cell size, text-area/screen size, and title query responses in the
  shared terminal runtime
- OSC default foreground/background, cursor, and indexed palette color query
  responses in the shared terminal runtime, including tracked OSC color-setting
  state, multi-index `OSC 4` queries, `rgb:`, `#RGB`/`#RRGGBB`, and
  WezTerm-style RGBA dynamic color specs, dynamic foreground/background reset
  with `OSC 110`/`OSC 111`, cursor-color reset with `OSC 112`, indexed-palette
  reset with `OSC 104`, stream-ordered response state, and ignored OSC
  color-setting bytes embedded inside ST-terminated control-string payloads
  split across PTY chunks
- OSC 8 hyperlink metadata in the shared terminal runtime, including UTF-8 C1
  OSC/ST and legacy raw C1 forms, renderer snapshot propagation, native-window
  Ctrl-click activation, and visible-output filtering
- OSC 9 notification text and OSC 777 `notify` title/body events from ESC plus
  UTF-8 C1 OSC/ST active/inactive pane output, with legacy raw C1 compatibility,
  dispatched through the native-window notification handler; native OS toast
  integration remains future work
- XTGETTCAP terminal-capability query responses for 7-bit DCS, UTF-8 C1 DCS/ST,
  and legacy raw C1 DCS/ST queries covering colors, terminal name, true-color
  markers, official WezTerm booleans, Meta-key boolean/templates
  (`km`/`smm`/`rmm`), OSC 52 template support, italic style templates,
  styled/colored underline templates, tmux/xterm cursor style and cursor color
  templates, WezTerm `Sync` synchronized-output template, overline,
  strikethrough, default-color, and palette-reset templates, foundational
  cursor/screen/style/color capabilities including WezTerm `sgr`/`sgr0`,
  standout, and conditional select-color templates, line/display editing
  controls, WezTerm control/save-restore capabilities,
  cursor-position/device-attribute query templates, title/status-line,
  title-stack, palette-initialization, printer, and memory-lock templates,
  reset/init templates (`rs1`, `is2`, `rs2`),
  tab-stop/erase/repeat/scroll-region templates, SGR mouse templates, application cursor plus base
  and modified function-key capabilities, WezTerm keypad transmit templates,
  Backspace/BackTab/keypad-center/keypad-enter plus shifted navigation/editing
  key capabilities, WezTerm ACS metadata, current columns/rows through `co`/`li`
  and official `cols`/`lines` plus `it=8` and `pairs=32767`, and unknown
  capability fallback
- DECRQSS state query responses for 7-bit DCS, UTF-8 C1 DCS/ST, and legacy raw
  C1 DCS/ST queries covering current SGR style, including faint, italic,
  blink, double underline, colon-separated underline style, underline color,
  concealed text, and overline, cursor shape, scroll-region state,
  conformance-level state, and modeled DECSLRM left/right-margin state
- XTVERSION query responses for `CSI > q`, `CSI > 0 q`, and C1 CSI forms
- DECRQM private-mode status query responses for application cursor keys,
  screen reverse-video, origin, auto-wrap, cursor blinking, Meta-key, cursor
  visibility, DECLRMM left-right margin mode, alternate-screen modes, mouse
  reporting, extended mouse protocols, focus, bracketed paste, synchronized
  output, private cursor save, `RIS` defaults, and unknown modes, including
  mode-like bytes embedded inside OSC or ST-terminated control-string payloads
- DECRQM ANSI-mode status query responses for insert/replace mode (`CSI 4 $ p`)
  and unknown ANSI modes, including C1 CSI forms
- native window OSC 52 policy parsing and write/query enforcement
- native window local selection text extraction, highlight overlay, mouse drag,
  double-click word selection, triple-click line selection, and copy shortcut
  detection
- native window literal, `literal:<text>`, and `regex:<pattern>` scrollback
  search across visual rows, zero-width regex filtering, next/previous
  navigation, and search shortcut detection
- native window custom startup command parsing and configured PTY command
  storage
- shared PTY command terminal environment defaults for `TERM` and `COLORTERM`
- native window log path parsing and visible PTY output logging
- native window session logs omit OSC title control sequences while still
  applying the title update to the window state
- native window TOML profile loading for frame limits, text/JSON metrics, OSC
  52 policy, custom commands, and log paths
- native window BEL event propagation into metrics without writing BEL bytes to
  the visible-output log
- terminal runtime damage propagation into native-window metrics and live
  bottom render-snapshot updates
- console path reuse of the shared key encoder
- PTY output feeding into the shared terminal runtime
- terminal runtime resize updates the grid and text-area size response
- terminal response filtering for dynamic cursor position, device-attribute, status,
  window state/position, window/screen pixel-size, text-area size, screen
  character-size, and title queries, including query-like bytes embedded inside
  OSC and ST-terminated control-string payloads split across runtime chunks
- native window title state from OSC `0`/`2` PTY output
- application cursor key mode tracking for native window input
- application keypad mode tracking for native window numpad input
- bracketed paste mode tracking for native window paste
- synchronized output plus display/private/ANSI insert mode tracking,
  private-mode and ANSI-mode status reporting, reset-on-`RIS`, `DECSTR`
  origin/insert soft reset, and delayed render-damage exposure until reset
- focus reporting mode tracking and native window focus event encoding
- C1 CSI private input mode and ANSI insert mode tracking in the shared
  local/window mode tracker
- window pixel dimensions are converted to terminal rows and columns
- native window snapshot rebuilds from runtime output, including cursor state
- native window cursor shape propagation for block, underline, and bar cursors
- renderer xterm 256-color palette mapping for indexed terminal colors
- native window scrollback viewport clamping and mouse-wheel movement
- native window Shift scrollback shortcuts without stealing unmodified page keys
- native window scrollback scrollbar overlay, including click/drag navigation
  and search-driven history viewport changes while the title remains reserved
  for search status
- native window xterm mouse-mode tracking and legacy/UTF-8/SGR/urxvt
  button/wheel/drag/motion report encoding

## Metrics

The current MVP uses tests and smoke checks as completion gates. Window runs can
now add `--metrics` for text output or `--metrics-json` for machine-readable
output. Both formats report:

- `first_pty_byte_ms`: process spawn timer to first PTY output chunk.
- `first_rendered_cell_ms`: process spawn timer to first non-empty render
  snapshot after PTY output.
- `pty_chunks` and `pty_bytes`: PTY output volume received by the UI runtime.
- `pty_chunk_process_p95_us`: p95 time from PTY output delivery through
  terminal runtime update, query responses, OSC 52 handling, title sync, and
  snapshot refresh.
- `damage_regions`: cumulative terminal damage regions reported by PTY output
  chunks.
- `damaged_cells`: cumulative width x height cell count across reported damage
  regions.
- `snapshot_damage_updates`: count of live bottom snapshot updates applied from
  terminal damage regions.
- `snapshot_rebuilds`: count of full render-snapshot rebuilds used for
  scrollback, selection, search, and other fallback paths.
- `render_frames` and `render_frame_p95_us`: successful framebuffer render
  count and p95 render-frame time.
- `full_render_frames` and `dirty_render_frames`: number of full framebuffer
  repaints and damage-scoped framebuffer updates.
- `input_writes`, `input_bytes`, and `input_write_p95_us`: PTY write volume and
  p95 write/flush duration for keyboard, paste, mouse, focus, and terminal
  response bytes.
- `bells`: PTY-side BEL events observed by the terminal runtime.

The current benchmark path can promote these metrics into thresholded gates:

- Workload isolation: `rssh-app bench --workload NAME` accepts
  `plain-scroll`, `ansi-scroll`, and `ansi-scroll-query`. The default remains
  `ansi-scroll-query` for compatibility. `plain-scroll` feeds plain text
  directly into the terminal parser and deliberately bypasses ANSI query
  filtering. `ansi-scroll` passes styled scrolling output through the runtime
  query filter without embedding response queries. `ansi-scroll-query` adds
  repeated cursor-position and text-area-size queries to expose the current
  repeated-scan cost.
- Deterministic work counters: JSON and text reports include
  `inspected_query_bytes`, the sum of candidate-buffer bytes presented to every
  legacy query matcher invocation; `scrolled_survivor_cell_clones`, the number
  of individual surviving grid cells cloned while scroll operations move rows;
  `history_row_relocations`, the number of surviving `Vec` scrollback rows
  relocated by prefix pruning; and `metadata_rebase_batches`, the number of
  non-empty history prunes that run the metadata rebase pass. All four counters
  are cumulative for one benchmark run and saturate at `u64::MAX`. They measure
  executed current implementation work, not elapsed-time estimates.
- Steady idle CPU: `rssh-app bench --json --idle-ms N` now samples the current
  app process during an idle window and reports idle CPU usage; add
  `--max-idle-cpu-percent N` to fail the command when it exceeds a budget.
- Burst throughput: `rssh-app bench --json` now reports deterministic
  terminal-runtime bytes parsed per second, p95 chunk processing latency,
  offscreen `PixelRenderer` p95 frame time, rendered pixels, and rendered pixel
  throughput without opening a GUI window; add `--min-throughput-bytes-per-sec`,
  `--max-chunk-p95-us`, or `--max-render-frame-p95-us` to turn those metrics
  into release gates.
- Memory footprint: `rssh-app bench --json --idle-ms N` now reports process
  resident memory, virtual memory, and accumulated CPU time; the remaining
  work is to compare baseline window, active shell, and future scrollback sizes
  before tightening `--max-process-memory-bytes`.

Use stable inputs when comparing implementations. For example, capture all
three 256 KiB workloads with the same chunk, render, idle, and terminal size:

```powershell
cargo +1.89.0 run --locked --release -p rssh-app -- bench --json --workload plain-scroll --bytes 262144 --chunk-size 8192 --render-frames 30 --idle-ms 200 --cols 120 --rows 30
cargo +1.89.0 run --locked --release -p rssh-app -- bench --json --workload ansi-scroll --bytes 262144 --chunk-size 8192 --render-frames 30 --idle-ms 200 --cols 120 --rows 30
cargo +1.89.0 run --locked --release -p rssh-app -- bench --json --workload ansi-scroll-query --bytes 262144 --chunk-size 8192 --render-frames 30 --idle-ms 200 --cols 120 --rows 30
```

Timing and process-resource fields vary by machine and run. Compare the four
work counters exactly, but treat timing fields as measured observations rather
than deterministic equality values.

Recommended MVP 5 targets:

- First rendered PTY cell under 500 ms on a normal local shell.
- PTY chunk processing p95 under 2 ms for 8 KiB chunks.
- Frame render p95 under 16 ms at the default 80x24 grid.
- Idle CPU under 3% after the shell prompt is visible.

## Explicit Non-Scope

- SSH protocol sessions in the native window.
- Advanced selection behavior such as selection across changing scrollback.
- GPU text shaping, glyph atlas caching, and font fallback.

## Next Milestone

MVP 5 should replace the minimal bitmap-font renderer with a production-grade
text rendering path and add basic terminal UX. The terminal core now has bounded
main-screen scrollback storage, the renderer can build scrollback viewport
snapshots, the native window can move that viewport with mouse-wheel input,
Shift page/navigation shortcuts, and click/drag scrollbar input, the right-edge
scrollbar now shows scrollback position in the framebuffer, and live bottom PTY
output can update the existing render snapshot and framebuffer cells from
terminal damage regions.

1. Carry damage regions through the future GPU/text renderer instead of
   repainting the entire frame.
2. Replace the minimal scrollbar overlay with a richer status area or native UI
   scrollbar once the production renderer lands.
3. Collect stable packaged-build baselines, then tighten the wide bench
   threshold gates into real release budgets.
