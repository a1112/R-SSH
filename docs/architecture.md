# R-SSH Native Architecture

R-SSH will be a native Rust SSH terminal client. The design separates terminal
state, connection I/O, rendering, and product UI so each layer can be tested and
replaced independently.

## Goals

- Provide an XShell-like desktop SSH client with native performance.
- Keep the terminal core independent from SSH, local PTY, and rendering.
- Use GPU rendering for dense terminal output and large scrollback.
- Support Windows first, without blocking later Linux and macOS support.
- Store session metadata locally and secrets through OS-backed secure storage.

## Non-Goals

- Do not build on Electron, WebView, or `xterm.js` for the main terminal view.
- Do not shell out to `ssh.exe` as the primary connection engine.
- Do not vendor third-party terminal projects into this repository.
- Do not implement every XShell feature before the terminal and SSH loop is
  stable.

## Layers

```text
Application shell
  Tabs, panes, session tree, command palette, settings, logs

Session runtime
  Owns terminal instances, connection tasks, resize events, lifecycle, replay

Connection layer
  SSH channel, local PTY, future serial/Telnet, SFTP, tunnels

Terminal core
  VT parser, terminal grid, scrollback, selection, hyperlinks, mouse modes

Renderer
  Font shaping, glyph atlas, damage tracking, GPU draw batches, presentation

Storage and security
  SQLite metadata, known hosts, DPAPI/Keychain/Secret Service, audit logs
```

## Primary Data Flow

```text
SSH or PTY byte stream
  -> terminal parser
  -> terminal grid and scrollback mutation
  -> damage regions
  -> renderer batches
  -> GPU presentation

keyboard, mouse, paste, resize
  -> input encoder
  -> active connection channel
  -> remote shell or local PTY
```

## Crate Boundaries

- `rssh-core`: shared domain types, terminal session and application-shell state
  (`Workspace`, `Tab`, `Pane`, actions, IDs) used before runtime ownership.
- `rssh-terminal`: VT parser boundary, grid, scrollback, selection, and input
  encoding.
- `rssh-renderer`: renderer state, damage tracking, font atlas, and future
  `wgpu` integration.
- `rssh-ssh`: SSH session abstraction. Start with `russh`; keep `libssh2`
  compatibility isolated behind this crate if needed.
- `rssh-pty`: local shell support through Windows ConPTY and Unix PTY.
- `rssh-app`: desktop entry point, native window, and user-facing action dispatch
  for app-shell state transitions.

## App Shell Status

- Completed in v1: deterministic shell state model in `rssh-core` for one startup
  workspace, tab, and pane with typed IDs and tab/pane/workspace actions.
- Completed in v1: `rssh-app` startup initializes this model from the local PTY
  command and updates the native window title with workspace/tab/pane state.
- Completed in v1: the native window renders a basic tab bar plus right/down
  split panes, with process-local per-pane runtime storage, click-to-focus, and
  pane-local wheel routing.
- Completed in v1: tab bar entries include a clickable close marker that closes
  non-final tabs or requests native-window shutdown when the final tab is closed.
- Completed in v1: the tab bar includes a clickable new-tab button that reuses
  the app-shell `NewTab` action path.
- Completed in v1: tab state can carry an explicit title, and tab bar labels
  prefer that explicit title before falling back to each tab's active-pane
  terminal title when OSC 0/1/2 or Sun OSC L/l title state is available.
- Completed in v1: the command palette includes Rename Tab with
  `rename tab <title>` query input, writing explicit titles for the active tab.
- Completed in v1: app-shell state tracks the last active tab and command
  palette dispatch exposes Activate Last Tab, no-oping when no previous active
  tab exists.
- Completed in v1: app-shell state exposes WezTerm-style zero-based
  `ActivateTab` index semantics through `ActivateTabIndex`, including negative
  indices for right-to-left tab selection.
- Completed in v1: app-shell state exposes WezTerm-style `ActivateTabRelative`
  wrapping plus `ActivateTabRelativeNoWrap` clamping, and the command palette
  includes wrapping and no-wrap Next/Previous Tab entries.
- Completed in v1: app-shell `MoveTabRelative` reorders the active tab within
  the current workspace while keeping that tab active.
- Completed in v1: app-shell `MoveTab` reorders the active tab to a zero-based
  absolute tab index and returns a typed out-of-range error, with command-palette
  entries for Move Tab To 1 through 4.
- Completed in v1: app-shell CloseTab handling can select either the default
  left-neighbor tab or the previous active tab, matching WezTerm's
  close-tab selection policy surface.
- Completed in v1: split separators can be dragged with the mouse to update the
  same app-shell resize deltas used by keyboard and command-palette pane resize
  actions.
- Completed in v1: pane resize actions are represented in app-shell state and
  routed from native-window keyboard shortcuts and command palette entries.
- Completed in v1: pane zoom state is represented in app-shell state and
  rendered by the native window as a full-tab pane until toggled off, explicitly
  unzoomed through WezTerm-style `SetPaneZoomState`, or unzoomed before
  switching to another pane.
- Completed in v1: pane-select Activate mode renders labels over pane regions,
  accepts label input to focus a pane, and supports `Esc`/`Ctrl+g` cancellation.
- Completed in v1: app-shell state exposes WezTerm-style `ActivatePaneByIndex`,
  with command-palette entries for pane indices 1 through 4.
- Completed in v1: app-shell state exposes WezTerm-style `RotatePanes` for
  clockwise/counter-clockwise pane identity rotation while preserving split
  positions and size deltas, with command-palette entries for both directions.
- Completed in v1: pane-select swap modes exchange active/selected pane layout
  positions and support both selected-pane focus and keep-active-focus behavior.
- Completed in v1: pane-select MoveToNewTab mode moves the selected pane into a
  new tab in the same workspace and activates that tab.
- Completed in v1: pane-select MoveToNewWindow mode removes the selected pane
  from the current split layout and records a pending native-window request with
  its own tab and active pane.
- Completed in v1: pending MoveToNewWindow requests can be consumed into an
  independent app-shell/native-window app state while transferring the detached
  pane runtime snapshot.
- Completed in v1: the native window entry point now runs through a
  multi-window manager that materializes detached MoveToNewWindow app states as
  additional OS windows.
- Completed in v1: PTY reader events carry the app-shell `WindowId` plus
  `PaneId`, so independent windows can route events without relying on globally
  unique pane IDs.
- Completed in v1: ClosePane follows WezTerm-style lifecycle cascading by
  closing a single-pane tab when another tab exists, while final tab/pane close
  actions request native-window shutdown from the window manager.
- In-progress after v1: pane focus UI, pane-local scrollbar/selection polish,
  platform focus policy for newly materialized windows, richer split drag
  affordances, custom tab formatting, external CLI/mux tab-title control, and
  mux/domain runtime orchestration.
- Implemented in v1: minimal `Ctrl+Shift+P` command palette dispatch for tab/pane
  workspace actions.
- Implemented in v1: command-palette `ClearSelection` clears active-window
  selection state and refreshes rendered selection highlights.
- Implemented in v1: command-palette Clear Scrollback maps to WezTerm-style
  `ClearScrollback('ScrollbackOnly')`, clearing active-pane history on the
  output side while preserving the viewport.
- Implemented in v1: command-palette Clear Scrollback And Viewport maps to
  WezTerm-style `ClearScrollback('ScrollbackAndViewport')`, clearing active-pane
  history plus the viewport while preserving the prompt/cursor row as the new
  first visible line.
- Implemented in v1: command-palette Copy To Clipboard maps the active
  selection to WezTerm-style `CopyTo('Clipboard')` behavior.
- Implemented in v1: command-palette Copy To Primary Selection and Copy To
  Clipboard And Primary Selection map to WezTerm-style
  `CopyTo('PrimarySelection')` and `CopyTo('ClipboardAndPrimarySelection')`
  routing. The native platform PrimarySelection backend remains a later
  platform-adapter task.
- Implemented in v1: command-palette Paste From Clipboard maps the configured
  clipboard reader into the active pane as WezTerm-style `PasteFrom('Clipboard')`
  behavior.
- Implemented in v1: command-palette Paste From Primary Selection maps to
  WezTerm-style `PasteFrom('PrimarySelection')` routing, and default
  `Ctrl+Insert`/`Shift+Insert` shortcut classification now matches WezTerm's
  PrimarySelection defaults.
- Implemented in v1: OSC 52 and iTerm2 `OSC 1337;Copy=;base64` clipboard writes
  are extracted from ESC plus UTF-8 C1 OSC/ST active and inactive pane output,
  with legacy raw C1 compatibility, and routed through the same clipboard
  writer/policy path used for OSC52 clipboard writes.
- Implemented in v1: WezTerm-documented OSC 9 notification text and OSC 777
  `notify` title/body events are extracted from ESC plus UTF-8 C1 OSC/ST active
  and inactive pane output, with legacy raw C1 compatibility, and routed through
  the native-window notification handler. Native OS toast integration remains
  future platform-adapter work.
- Implemented in v1: ConEmu-style `OSC 9;4;st;pr` progress reports update
  terminal-runtime progress state as None, percentage, error, or indeterminate
  from ESC plus UTF-8 C1 OSC/ST forms, are not misrouted as OSC 9 notifications,
  and sync into active/inactive app-shell pane metadata. Lua pane API exposure
  and tab/status formatting remain future parity work.
- Implemented in v1: ASCII BEL from active and inactive pane output is counted
  in metrics and dispatched through a typed native-window bell hook with the
  originating pane id. Lua event wiring and audible/visual bell configuration
  remain future parity work.
- Implemented in v1: native window focus changes still write CSI focus-reporting
  sequences to the PTY when requested and now dispatch a typed focus-change hook
  with the active pane id plus focused/unfocused state. Lua event wiring remains
  future parity work.
- Implemented in v1: successful native window resizes update terminal/runtime
  dimensions and then dispatch a typed resize hook with active pane id, pixel
  size, and terminal rows/columns. Lua event wiring and fullscreen dimension
  metadata remain future parity work.
- Implemented in v1: ctrl-clicked OSC 8 hyperlinks dispatch a typed open-uri
  hook with the active pane id and URI before the default opener runs. Returning
  `false` suppresses the default opener; Lua event wiring and full
  `CompleteSelectionOrOpenLinkAtMouseCursor` coverage remain future parity work.
- Implemented in v1: command-palette Reset Terminal injects RIS (`ESC c`) into
  the active pane output side, matching WezTerm-style `ResetTerminal`.
- Implemented in v1: command-palette scrollback navigation covers
  WezTerm-style Scroll To Top/Bottom, Scroll Page Up/Down, and Scroll Line
  Up/Down actions for the active viewport.
- Implemented in v1: command-palette Scroll To Previous Prompt and Scroll To
  Next Prompt use OSC 133 `A`/`N`/`P` prompt row markers to jump the active
  viewport between retained prompt rows.
- Implemented in v1: `rssh-terminal` records OSC 133 Prompt/Input/Output
  semantic types on retained cells, exposes semantic zone queries over
  scrollback plus the visible grid, and handles `I` input markers as
  line-scoped input until cursor movement.
- Implemented in v1: `rssh-terminal` records WezTerm shell-integration OSC 133
  `D` command-finished metadata with retained row, exit status, and `aid`.
- Implemented in v1: `rssh-terminal` exposes retained row/column text extraction
  for semantic zones and general regions, unwrapping soft-wrapped physical rows
  into logical-line text. WezTerm-style Lua pane APIs and configurable key-table
  bindings remain future parity work.
- Implemented in v1: `rssh-terminal` records OSC 7 and iTerm2
  `OSC 1337;CurrentDir` current working directory metadata. `rssh-app` syncs
  that metadata into per-pane launch models for active and inactive panes so new
  tabs/splits inherit it and local PTY spawns receive a decoded filesystem cwd.
- Implemented in v1: `rssh-terminal` base64-decodes iTerm2/WezTerm
  `OSC 1337;SetUserVar` metadata into terminal user vars. `rssh-app` syncs
  those values into per-pane app-shell metadata for active and inactive pane
  runtimes and emits a typed native-window user-var change hook when a stored
  pane value changes.
- Implemented in v1: `rssh-terminal` base64-decodes iTerm2
  `OSC 1337;SetBadgeFormat` metadata into terminal badge format state.
  `rssh-app` syncs that value into per-pane app-shell metadata for active and
  inactive pane runtimes.
- Implemented in v1: `rssh-terminal` records iTerm2/WezTerm
  `OSC 1337;File=...` inline image metadata and decoded payload bytes at the
  current retained-history cursor position. `rssh-renderer` carries those image
  items through live, scrollback, and overlaid pane render snapshots, and draws
  PNG, JPEG, and GIF payloads into the RGBA framebuffer with delay-aware
  animated GIF frame selection by elapsed render time, cell/`px` dimensions,
  and damage-region redraw coverage. The same image
  snapshot path now covers the Kitty Graphics Protocol direct `a=T` subset for
  single-block and chunked, uncompressed and zlib-compressed raw RGB/RGBA plus
  encoded image payloads, regular-file `t=f` simple-file transfers with
  optional `O`/`S` file slicing, temporary-file `t=t` transfers with guarded
  `tty-graphics-protocol` temp-file deletion, plus minimal `a=t,i=<id>`
  stored-image transmission, `a=t,I=<number>` terminal-assigned image-number
  uploads with `i`/`I` OK responses, and `a=p` placement by image id or image
  number at the current cursor.
  Basic source rectangles (`x`/`y`/`w`/`h`) are propagated for direct and
  stored-placement image cropping, and `X`/`Y` target pixel offsets shift direct
  and stored placements relative to the placement cell; placements that specify
  only `c` or only `r` derive the other cell axis from the source image or
  source-rectangle aspect ratio. Basic direct `a=q`
  support queries return `OK`/`EINVAL`, stored-image existence queries and
  stored placements return `OK` or `ENOENT` for present/missing image ids or
  image numbers, Kitty `q=1`/`q=2` response suppression is honored, `i`/`I`
  mutual exclusion is enforced, direct/stored placements advance the cursor by
  the placement cell rectangle unless `C=1` suppresses movement, and basic
  placement ids are tracked so repeated `(image id, placement id)` pairs
  replace old placements.
  Basic `a=d` deletion covers all live viewport visible Kitty placements while
  retaining scrollback placements, image-id placement deletion, image-number
  placement deletion, image-id range deletion,
  `(image id, placement id)` pair deletion, and cursor-cell, explicit-cell,
  visible-column, visible-row, z-index, and cell-plus-z-index deletion. These
  position-oriented deletes leave Unicode-placeholder-derived renders intact;
  the derived render is removed when the underlying placeholder cell is
  overwritten or erased. The
  renderer applies Kitty z-index layer ordering, drawing negative z-index images
  below text, z-index values below `i32::MIN / 2` below non-default cell
  backgrounds, and non-negative z-index images above text in ascending z order,
  with Kitty image id breaking ties for overlapping same-z images.
  Terminal erase display paths remove affected inline-image
  placements for `CSI 2J`, drop scrollback inline images for `CSI 3J`, and
  rebase retained visible image rows after scrollback clearing. Alternate-screen
  `?1049` switches isolate inline-image placements between the main and
  alternate buffers, restoring main placements on exit and discarding alternate
  placements. Scroll operations move inline-image placements with affected text
  rows and drop placements that leave the scrolled region. Basic Sixel DCS `q`
  image payloads with RGB/HLS palette
  definitions, raster-attribute `Ph`/`Pv` pixel dimensions with clipping to the
  declared size, DCS `P2` transparent/opaque background mode, repeat
  introducers, carriage returns, and sixel newlines are normalized into raw
  RGBA inline images, advance the cursor to the next terminal line, and draw
  through the same snapshot path. Automatic animated GIF
  refresh/invalidation scheduling, Kitty shared-memory transfers, remaining
  richer placement controls, broader query responses beyond current direct
  payload and stored-image existence checks, full Sixel protocol coverage, and
  remote sync remain future parity work.
- Implemented in v1: `rssh-app` responds to xterm cell/window size queries and
  WezTerm/iTerm2-compatible `OSC 1337;ReportCellSize` requests using the native
  fixed cell pixel dimensions.
- Implemented in v1: `rssh-app` answers XTGETTCAP `Sync` queries with the
  WezTerm synchronized-output terminfo template for the existing
  `ESC[?2026h/l` path.
- Implemented in v1: XTGETTCAP style/color replies include WezTerm-style
  overline, strikethrough, default-color, palette-reset, `sgr`/`sgr0`,
  standout, and conditional select-color templates in addition to italic,
  underline, underline-color, and true-color templates.
- Implemented in v1: cursor blink mode `?12` is tracked in the terminal core
  and shared mode tracker, DECRQM reports it for ESC/C1 CSI forms, and XTGETTCAP
  exposes WezTerm `civis`/`cnorm`/`cvvis` cursor visibility/blink templates.
- Implemented in v1: Meta-key mode `?1034` is tracked in the shared runtime
  mode tracker, DECRQM reports it for ESC/C1 CSI forms, and XTGETTCAP exposes
  WezTerm `km`/`smm`/`rmm` Meta-key capabilities.
- Implemented in v1: XTGETTCAP replies expose the remaining official WezTerm
  boolean names (`am`, `bce`, `ccc`, `hs`, `mc5i`, `mir`, `msgr`, `npc`, `Su`,
  `xenl`) plus `flash`, printer (`mc0`/`mc4`/`mc5`), memory-lock
  (`meml`/`memu`), and `rs1` reset templates.
- Implemented in v1: XTGETTCAP replies expose implemented tab-stop/backtab,
  erase-character, repeat-character, scroll-region, indexed scroll,
  Backspace/BackTab/keypad-enter, SGR mouse templates (`kmous`/`XM`/`xm`),
  shifted navigation/editing, and WezTerm
  `kf13`-`kf63` modified function-key templates.
- Implemented in v1: XTGETTCAP replies expose dynamic `co`/`li` and official
  WezTerm `cols`/`lines` numeric names from the current runtime size, plus
  `it=8` for the default tab interval and `pairs=32767`.
- Implemented in v1: XTGETTCAP replies expose WezTerm basic control,
  save/restore, and ACS character-set templates (`bel`, `cr`, `ind`, `ri`,
  `sc`, `rc`, `smacs`, `rmacs`) using sequences already handled by the parser.
- Implemented in v1: XTGETTCAP replies expose WezTerm cursor-position and
  device-attribute query templates (`u6`, `u7`, `u8`, `u9`) for the existing
  CSI query response paths.
- Implemented in v1: XTGETTCAP replies expose WezTerm title/status-line and
  palette-initialization templates (`tsl`, `fsl`, `dsl`, `initc`) for existing
  OSC title and OSC 4 color handling paths.
- Implemented in v1: `rssh-terminal` handles DECSTR `CSI ! p` soft reset for
  insert/replace mode, origin mode, scroll region, G0 character set, and
  saved-cursor state without clearing cells; app runtime and console filtering
  track ESC/C1 DECSTR for mode reports and expose XTGETTCAP `is2`/`rs2`
  reset/init templates.
- Implemented in v1: XTGETTCAP replies expose WezTerm keypad transmit
  templates (`smkx`, `rmkx`) for existing application cursor-key and
  application-keypad mode tracking/input paths.
- Implemented in v1: XTGETTCAP replies expose WezTerm keypad-center `kb2`,
  and console input encodes `KeypadBegin` as `ESC O E` while preserving keypad
  digit 5 as `ESC O u` in application-keypad mode.
- Implemented in v1: `rssh-app` tracks xterm OSC 4/10/11/12 color changes and
  answers color queries, including WezTerm-style multiple OSC 4 palette
  index/color update pairs and index query pairs in one sequence, plus
  `#RGB`/`#RRGGBB` hex color specs and RGBA `OSC 10`/`11`/`12` dynamic colors.
- Implemented in v1: `rssh-terminal` records OSC 0/1/2 title updates and Sun
  OSC L/l aliases without rendering those control bytes, giving the app shell
  WezTerm-compatible active-pane title fallback state.
- Implemented in v1: `rssh-terminal` saves and restores the tracked terminal
  title through xterm title-stack `CSI 22;0;0t` and `CSI 23;0;0t`, and
  XTGETTCAP `smcup`/`rmcup` expose WezTerm's alternate-screen plus title-stack
  templates.
- Implemented in v1: `rssh-terminal` handles DECALN `ESC # 8` screen alignment
  display by filling the visible grid with `E` cells and resetting
  margins/origin mode.
- Implemented in v1: `rssh-terminal` ignores WezTerm-documented non-printing
  C0 controls while preserving BEL/BS/HT/LF/VT/FF/CR/ESC special handling.
- Implemented in v1: `rssh-terminal` models WezTerm SGR mode 6 RGBA colors for
  foreground, background, and underline color state; `rssh-renderer` preserves
  alpha in RGBA pixel conversion, and app-shell DECRQSS SGR responses serialize
  alpha-bearing colors.
- Implemented in v1: `rssh-terminal` models WezTerm SGR 73/74/75 vertical-align
  state for superscript, subscript, and baseline; `rssh-renderer` offsets the
  glyph baseline, and app-shell DECRQSS SGR responses serialize active 73/74
  state.
- Implemented in v1: `rssh-app` answers WezTerm-documented DECRQSS `"` `p`
  conformance-level and `s` left/right-margin queries in both native runtime and
  console output filter paths. The `s` query reports the modeled DECSLRM state,
  and DECRQM reports DECLRMM `?69` for ESC and C1 CSI forms.
- Implemented in v1: copy mode can move between semantic zones across retained
  scrollback with WezTerm-style `z`/`Shift+Z` bindings and typed
  Prompt/Input/Output filters backed by OSC 133 zones.
- Implemented in v1: copy mode stores source-row selection anchors, so `y` can
  copy selections that span the live viewport and retained scrollback.
- Implemented in v1: copy mode `y` follows WezTerm's default CopyTo
  ClipboardAndPrimarySelection, then ScrollToBottom and Close behavior.
- Implemented in v1: copy mode supports WezTerm-style Cell selection with
  Space/`v`.
- Implemented in v1: copy mode supports WezTerm-style Line selection with
  uppercase no-modifier or shifted `V`.
- Implemented in v1: copy mode supports WezTerm-style rectangular block
  selection with `Ctrl+V`.
- Implemented in v1: copy mode vertical movement and page movement use
  source-row cursor coordinates, so `j`/`k`, arrows, PageUp/PageDown,
  `Ctrl+B/F`, and `Ctrl+U/D` can traverse retained scrollback.
- Implemented in v1: copy mode supports WezTerm-style
  `MoveToStartOfNextLine` through Enter and character CR (`\r`) events.
- Implemented in v1: copy mode `g`/`Shift+G` move to scrollback top/bottom.
- Implemented in v1: copy mode `H`/`M`/`L` move to viewport top/middle/bottom
  for both shifted and uppercase no-modifier default key-table events.
- Implemented in v1: copy mode `^`/`Alt+m` and `$`/End move to the first/last
  non-space cell in the current source row, matching WezTerm-style
  content-aware line start/end.
- Implemented in v1: copy mode word movement supports WezTerm-style
  `MoveForwardWord`, `MoveBackwardWord`, and `MoveForwardWordEnd` defaults
  across retained source rows.
- Implemented in v1: copy mode jump-to-char supports WezTerm-style
  `JumpForward`, `JumpBackward`, `JumpAgain`, and `JumpReverse` defaults on the
  current source row.
- Implemented in v1: copy mode supports WezTerm-style `MoveToSelectionOtherEnd`
  and `MoveToSelectionOtherEndHoriz` through `o`/`O`.
- Implemented in v1: ordinary copy-mode close follows WezTerm's
  `ScrollToBottom` then `Close` default behavior before exiting the overlay.
- Implemented in v1: copy mode and copy-mode search close on both Escape key
  events and character ESC (`\u{1b}`) events, clearing copy-mode search status
  from the window title.
- Implemented in v1: copy mode and copy-mode search allow global
  command-palette and app-shell shortcuts such as `Ctrl+Shift+P` and
  `Ctrl+Shift+T` to fall through from the overlay, matching WezTerm key-table
  fallback behavior.
- Implemented in v1: copy-mode search keeps copy mode active while entering
  `/`/`?` queries and supports WezTerm-style next/prior match navigation,
  including character CR as PriorMatch.
- Implemented in v1: copy-mode search supports WezTerm-style page-wise match
  navigation with PageDown/PageUp.
- Implemented in v1: copy-mode search supports WezTerm-style `Ctrl+R`
  match-type cycling across case-sensitive, case-insensitive, and regex search.
- Implemented in v1: ordinary `Ctrl+F` search supports WezTerm-style search
  table navigation with Down/Up, `Ctrl+N`/`Ctrl+P`, PageDown/PageUp,
  `Ctrl+R` match-type cycling, `Ctrl+U` clear-pattern, character ESC close, and
  initial query prefill from the current selection's first line.
- Implemented in v1: quick-select mode (`Ctrl+Shift+Space`) for common patterns
  (URLs including `git@`, `git://`, `ssh://`, and `ftp://`, markdown URLs, diff
  paths, docker SHA values, paths, colors, UUID/IPFS/SHA hashes, IPv4/IPv6, hex
  addresses, long numbers, emails), quick overlay navigation including
  `Ctrl+N`/`Ctrl+P`, PageDown/PageUp page-wise movement, WezTerm's Enter
  PriorMatch binding, and WezTerm-style label input where lowercase labels copy
  the match to ClipboardAndPrimarySelection and uppercase labels paste it into
  the pane.

## Technology Choices

Recommended initial stack:

- `winit` for cross-platform native windows and input.
- `wgpu` for GPU rendering across DirectX, Metal, Vulkan, and OpenGL backends.
- `cosmic-text` first for shaping and font fallback; evaluate HarfBuzz bindings
  if terminal compatibility requires lower-level control.
- `russh` first for pure Rust SSH; keep `libssh2` as an optional fallback.
- `portable-pty` as the first local PTY implementation reference, with direct
  platform adapters later if the abstraction becomes limiting.
- `rusqlite` or `sqlx` for local session metadata. Prefer `rusqlite` for a small
  desktop app unless async database access becomes useful.
- `keyring` or platform-specific APIs for secrets. Never store passwords or
  private key passphrases in SQLite.

## Error Handling

- Connection failures should keep the session tab visible with a clear terminal
  status line and retry action.
- Host key mismatch is a blocking security error and must never be auto-accepted.
- Parser errors should be counted and logged, but unknown escape sequences should
  not crash the session.
- Renderer device loss should rebuild renderer state from terminal grid and
  scrollback.
- Secret storage failures should degrade to "not saved" rather than plaintext
  persistence.

## Testing Strategy

- Unit-test terminal grid behavior and VT parser conformance with recorded
  byte streams.
- Use snapshot tests for terminal state after escape sequences.
- Use loopback SSH fixtures before touching real servers.
- Use local PTY integration tests gated by platform.
- Use renderer pixel/screenshot tests once `wgpu` is introduced.
- Use fuzzing for the VT parser before handling untrusted network streams at
  scale.
