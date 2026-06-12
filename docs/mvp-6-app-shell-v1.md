# MVP 6: App Shell v1

MVP 6 introduces an internal process-local shell model so WezTerm-like tabs,
panes, and workspaces exist as first-class state. It keeps startup compatible
with a single default local PTY pane, while adding process-local per-pane
runtime storage for tabs and split panes.

## Completed Scope

- `rssh-core` now owns typed identifiers and models for app shell state:
  - `WindowId`, `WorkspaceId`, `TabId`, and `PaneId`
  - `AppShell`, `Workspace`, `Tab`, `Pane`, and `PaneLaunch`
  - `AppAction` for tab/pane/workspace operations
  - `AppShellError` with typed invalid-action and guard errors
- `rssh-core` starts with a deterministic shell baseline:
  - one workspace
  - one tab
  - one pane
- `rssh-app` initializes the same baseline from the startup `PtyCommand`.
- `rssh-app` app-shell action dispatch maps from typed actions to updated
  app state.
- `rssh-app` keyboard handling recognizes app-shell shortcuts before PTY input:
  - `Ctrl+Shift+T` new tab
  - `Ctrl+Shift+W` close tab
  - `Ctrl+Shift+]` next tab
  - `Ctrl+Shift+[` previous tab
  - `Ctrl+Tab` next tab
  - `Ctrl+Shift+Tab` previous tab
  - `Ctrl+PageUp` previous tab
  - `Ctrl+PageDown` next tab
  - `Ctrl+Shift+PageUp` move tab left
  - `Ctrl+Shift+PageDown` move tab right
  - `Ctrl+Shift+Alt+PageUp` move tab left
  - `Ctrl+Shift+Alt+PageDown` move tab right
  - `Ctrl+Shift+1..9` activate tab index (`1`..`8` by position, `9` last)
  - `Ctrl+Shift+Alt+\"` split pane right
  - `Ctrl+Shift+Alt+%` split pane down
  - `Ctrl+Shift+Alt+Arrow` resize active pane in the arrow direction
  - `Ctrl+Shift+Arrow` activate the neighboring pane in that direction
  - `Ctrl+Shift+Z` toggle active pane zoom
  - `Ctrl+Shift+D` split pane right
  - `Ctrl+Shift+E` split pane down
- `rssh-core` tracks the last active tab and exposes an `ActivateLastTab`
  action matching WezTerm's no-op behavior when no previous active tab exists.
- `rssh-core` exposes `ActivateTabIndex` for WezTerm-style `ActivateTab`
  semantics: zero-based positive indices select from the left, and negative
  indices select from the right. `rssh-app` routes `Ctrl+Shift+1..9` through
  this action, with `9` mapped to `-1`.
- `rssh-core` exposes `ActivateTabRelative` for wrapping relative tab activation
  and `ActivateTabRelativeNoWrap` for first/last clamping. `rssh-app` routes the
  default tab navigation shortcuts and command-palette Next/Previous Tab entries
  through those app-shell actions.
- `rssh-core` applies `MoveTabRelative` by reordering the active tab within the
  current workspace while preserving that tab as active.
- `rssh-core` exposes WezTerm-style `MoveTab` for moving the active tab to a
  zero-based absolute index, preserving the active tab and rejecting out-of-range
  indices. `rssh-app` exposes command-palette Move Tab To 1..4 entries.
- `rssh-core` supports WezTerm's close-tab selection policy: callers can keep
  the default left-neighbor activation or request last-active-tab activation
  when closing the active tab.
- `rssh-app` includes a minimal command palette (`Ctrl+Shift+P`) for quick
  execution of tab/pane/workspace actions, including Activate Last Tab and
  directional pane activation.
- `rssh-app` includes a quick-select overlay (`Ctrl+Shift+Space`) that detects
  common URL/path/hash/IP/email patterns including WezTerm's non-http URL
  schemes (`git@`, `git://`, `ssh://`, `ftp://`), markdown URLs, diff paths,
  docker SHA values, paths, colors, UUID/IPFS/SHA hashes, IPv4/IPv6, hex
  addresses, and long numbers. It supports keyboard navigation including
  `Ctrl+N`/`Ctrl+P`, PageDown/PageUp page-wise movement, WezTerm's Enter
  PriorMatch binding, and WezTerm-style quick-select labels: lowercase labels
  copy the match to ClipboardAndPrimarySelection, uppercase labels paste it
  into the pane.
- `rssh-app` includes a pane-select overlay from the command palette. It labels
  panes with the WezTerm default selection alphabet (`a`, `s`, `d`, ...),
  activates the selected pane when a label is typed, and exits on `Esc` or
  `Ctrl+g`.
- `rssh-core` exposes `ActivatePaneByIndex` for WezTerm-style current-tab pane
  index activation. `rssh-app` exposes command-palette Activate Pane 1..4
  entries and ignores invalid pane indices.
- `rssh-core` exposes `RotatePanes` for WezTerm-style clockwise and
  counter-clockwise pane identity rotation while preserving split positions and
  size deltas. `rssh-app` exposes command-palette Rotate Panes Clockwise and
  Rotate Panes Counter Clockwise entries.
- `rssh-app` includes pane-select swap modes from the command palette:
  `SwapWithActive` exchanges the active pane's layout position with the selected
  pane and focuses the selected pane, while `SwapWithActiveKeepFocus` keeps focus
  on the original active pane after the exchange.
- `rssh-app` includes a pane-select `MoveToNewTab` mode from the command
  palette. Selecting a pane moves it into a newly created tab in the same
  workspace and activates that tab.
- `rssh-app` includes a pane-select `MoveToNewWindow` mode from the command
  palette. Selecting a pane removes it from the current split layout and records
  a pending native-window request with its own tab and active pane.
- Pending `MoveToNewWindow` requests can be consumed into an independent
  app-shell/native-window app state while transferring the detached pane runtime
  snapshot.
- `rssh-app window` now runs through a multi-window manager that materializes
  detached `MoveToNewWindow` app states as additional native OS windows.
- PTY reader events now carry the app-shell `WindowId` plus `PaneId`, so
  independent windows do not rely on globally unique pane IDs for event routing.
- `rssh-app` includes copy mode (`Ctrl+Shift+X`) with Vim-like movement and copy
  actions (Space/`v`, `V`, `y`, `Enter`, cursor movement keys,
  Home/End/`^`/`$`, etc.).
- Copy mode supports WezTerm-style semantic-zone movement across retained
  scrollback with `z`/`Shift+Z`, plus typed Prompt/Input/Output zone movement
  via `Alt+P`, `Alt+I`, and `Alt+O`/`Alt+Z`, backed by OSC 133 zones.
- Copy mode keeps source-row selection anchors, so selections that span the
  live viewport and retained scrollback can be copied with `y`.
- Copy mode `y` follows WezTerm's default CopyTo
  ClipboardAndPrimarySelection, then ScrollToBottom and Close behavior.
- Copy mode supports WezTerm-style Cell selection through Space/`v`, Line
  selection through uppercase no-modifier or shifted `V`, and rectangular block
  selection through `Ctrl+V`.
- Copy mode vertical movement (`j`/`k`, arrows, PageUp/PageDown, `Ctrl+B/F`,
  and `Ctrl+U/D`) moves through retained scrollback using source-row cursor
  coordinates.
- Copy mode supports WezTerm-style `MoveToStartOfNextLine` through Enter and
  character CR (`\r`) events.
- Copy mode supports WezTerm-style scrollback top/bottom movement through
  `g`/`Shift+G`.
- Copy mode supports WezTerm-style viewport top/middle/bottom movement through
  `H`/`M`/`L`, including uppercase no-modifier key events from the default key
  table.
- Copy mode supports WezTerm-style content-aware line start/end movement through
  `^`/`Alt+m` and `$`/End, landing on the first/last non-space cell in the
  current source row.
- Copy mode supports WezTerm-style word movement through `w`, `b`, `e`, Tab,
  Shift+Tab, Alt+Left/Right, and Alt+F/B, including movement across retained
  source rows.
- Copy mode supports WezTerm-style jump-to-char movement through `f`, `t`, `F`,
  `T`, `;`, and `,` on the current source row.
- Copy mode supports WezTerm-style selection-end movement through `o` and `O`.
- Ordinary copy-mode close follows WezTerm's `ScrollToBottom` then `Close`
  default behavior before exiting the overlay.
- Copy mode and copy-mode search close on both Escape key events and character
  ESC (`\u{1b}`) events, clearing copy-mode search status from the window title.
- Copy mode and copy-mode search allow global command-palette and app-shell
  shortcuts such as `Ctrl+Shift+P` and `Ctrl+Shift+T` to fall through from the
  overlay, matching WezTerm key-table fallback behavior.
- Copy mode keeps copy-mode state while searching, with `/`/`?` search input and
  WezTerm-style next/prior match navigation via Down/`Ctrl+N` and
  Up/Enter/CR/`Ctrl+P`, plus page-wise match navigation via PageDown/PageUp and
  `Ctrl+R` match-type cycling across case-sensitive, case-insensitive, and
  regex search.
- Ordinary `Ctrl+F` search supports WezTerm-style search table navigation via
  Down/Up, `Ctrl+N`/`Ctrl+P`, PageDown/PageUp, `Ctrl+R` match-type cycling,
  `Ctrl+U` clear-pattern, character ESC close, and initial query prefill from
  the current selection's first line.
- `rssh-app` exposes WezTerm-style `ClearSelection` through the command palette,
  clearing the active window selection and refreshing the rendered highlight.
- `rssh-app` exposes WezTerm-style `ClearScrollback('ScrollbackOnly')` through
  the command palette, clearing active-pane history on the output side while
  preserving the viewport.
- `rssh-app` exposes WezTerm-style
  `ClearScrollback('ScrollbackAndViewport')` through the command palette,
  clearing active-pane history plus the viewport while preserving the
  prompt/cursor row as the new first visible line.
- `rssh-app` exposes WezTerm-style `CopyTo('Clipboard')` through the command
  palette as Copy To Clipboard for the active selection.
- `rssh-app` exposes WezTerm-style `CopyTo('PrimarySelection')` and
  `CopyTo('ClipboardAndPrimarySelection')` through the command palette routing
  layer. The actual OS PrimarySelection backend is still a platform-adapter
  follow-up.
- `rssh-app` exposes WezTerm-style `PasteFrom('Clipboard')` through the command
  palette as Paste From Clipboard into the active pane.
- `rssh-app` exposes WezTerm-style `PasteFrom('PrimarySelection')` through the
  command palette routing layer, and classifies `Ctrl+Insert`/`Shift+Insert`
  the same way as WezTerm's PrimarySelection defaults.
- `rssh-app` exposes WezTerm-style `ResetTerminal` through the command palette,
  injecting RIS (`ESC c`) on the active pane output side.
- `rssh-app` exposes WezTerm-style scrollback navigation through the command
  palette: Scroll To Top, Scroll To Bottom, Scroll Page Up/Down, and Scroll Line
  Up/Down.
- `rssh-terminal` records OSC 133 `A`/`N`/`P` prompt rows, and `rssh-app`
  exposes WezTerm-style Scroll To Previous Prompt and Scroll To Next Prompt
  through the command palette for the active pane.
- `rssh-terminal` records OSC 133 Prompt/Input/Output semantic zones across
  retained scrollback and the visible grid, including line-scoped `I` input
  markers, and can extract text from semantic zones or retained row/column
  regions while unwrapping soft-wrapped physical rows into logical lines.
  WezTerm-style Lua pane APIs and configurable key-table bindings remain future
  work.
- `rssh-terminal` records WezTerm shell-integration OSC 133 `D`
  command-finished metadata with retained row, exit status, and `aid`.
- `rssh-terminal` records OSC 7 and iTerm2 `OSC 1337;CurrentDir` current
  working directory metadata. `rssh-app` syncs it into per-pane launch metadata
  for active and inactive panes so new tabs/splits inherit the cwd, and decodes
  `file://` cwd URIs before spawning local PTYs.
- `rssh-terminal` base64-decodes iTerm2/WezTerm `OSC 1337;SetUserVar`
  metadata into terminal user vars. `rssh-app` syncs those user vars into
  per-pane app-shell metadata for active and inactive panes and dispatches a
  typed native-window user-var change hook when a stored pane value changes.
- `rssh-terminal` base64-decodes iTerm2 `OSC 1337;SetBadgeFormat` metadata into
  terminal badge format state. `rssh-app` syncs that badge metadata per pane for
  active and inactive panes.
- `rssh-terminal` records iTerm2/WezTerm `OSC 1337;File=...` inline image
  metadata and decoded payload bytes at the current retained-history cursor
  position; `rssh-renderer` carries those image items through live, scrollback,
  and overlaid pane render snapshots and draws PNG/JPEG/GIF payloads into the
  framebuffer with delay-aware animated GIF frame selection by elapsed render
  time, cell/`px` dimensions, and damage-region redraw coverage. The same path
  supports the Kitty Graphics Protocol direct `a=T`
  subset for single-block and chunked, uncompressed and zlib-compressed raw
  RGB/RGBA plus encoded image payloads, regular-file `t=f` simple-file
  transfers with optional `O`/`S` file slicing, temporary-file `t=t` transfers
  with guarded `tty-graphics-protocol` temp-file deletion, plus minimal
  `a=t,i=<id>` stored-image transmission, `a=t,I=<number>` terminal-assigned
  image-number uploads with `i`/`I` OK responses, and `a=p` placement by image
  id or image number at the current cursor. Basic source rectangles
  (`x`/`y`/`w`/`h`) are propagated for
  direct and stored-placement image cropping, and `X`/`Y` target pixel offsets
  shift direct and stored placements relative to the placement cell. Basic
  direct `a=q` support queries return `OK`/`EINVAL`, stored placements return
  `OK` or `ENOENT` for present/missing image ids or image numbers, stored-image
  existence queries return `OK`/`ENOENT`, Kitty `q=1`/`q=2` response
  suppression is honored, `i`/`I` mutual exclusion is enforced, direct/stored
  placements advance the cursor by the placement cell rectangle unless `C=1`
  suppresses movement, and placement ids are tracked so
  repeated
  `(image id, placement id)` pairs replace old placements. Basic `a=d` deletion
  covers all visible Kitty placements,
  image-id placement deletion, image-number placement deletion, image-id range
  deletion, `(image id, placement id)` pair deletion, and cursor-cell,
  explicit-cell, visible-column, visible-row, z-index, and cell-plus-z-index
  deletion. The renderer applies Kitty z-index layer ordering, drawing negative
  z-index images below text and non-negative z-index images above text in
  ascending z order. Terminal erase display paths remove affected inline-image
  placements for `CSI 2J`, drop scrollback inline images for `CSI 3J`, and
  rebase retained visible image rows after scrollback clearing. Alternate-screen
  `?1049` switches isolate inline-image placements between main and alternate
  buffers, restoring main placements on exit and discarding alternate placements.
  Scroll operations move inline-image placements with affected text rows and
  drop placements that leave the scrolled region. Basic Sixel DCS `q` payloads
  with RGB/HLS
  palette definitions, color selection, raster-attribute `Ph`/`Pv` pixel
  dimensions, repeat introducers, carriage returns, and sixel newlines are
  normalized into raw RGBA inline images and rendered through the same snapshot
  path. Automatic animated GIF refresh/invalidation scheduling, Kitty
  shared-memory transfers, remaining richer placement controls, broader query
  responses beyond current direct payload and stored-image existence checks,
  full Sixel protocol coverage, sixel scrolling/pan edge cases, and pane sync
  remain later parity work.
- `rssh-app` extracts OSC 52 and iTerm2 `OSC 1337;Copy=;base64` clipboard writes
  from ESC plus UTF-8 C1 OSC/ST active and inactive pane output, retaining
  legacy raw C1 compatibility and reusing the existing OSC52 write policy and
  clipboard writer path.
- `rssh-app` extracts WezTerm-documented OSC 9 notification text and OSC 777
  `notify` title/body events from ESC plus UTF-8 C1 OSC/ST active and inactive
  pane output, retaining legacy raw C1 compatibility and dispatching them
  through the native-window notification handler. The native OS toast backend
  remains a later platform-adapter task.
- `rssh-app` records WezTerm-documented ConEmu-style `OSC 9;4;st;pr` progress
  state as None, percentage, error, or indeterminate from ESC plus UTF-8 C1
  OSC/ST forms, does not treat progress reports as OSC 9 notifications, and
  syncs active/inactive pane progress into app-shell pane metadata. Lua pane API
  exposure and tab/status formatting remain later parity work.
- `rssh-app` counts ASCII BEL events from active and inactive pane output and
  dispatches them through a typed native-window bell hook with the originating
  pane id. Lua event wiring and audible/visual bell configuration remain later
  parity work.
- `rssh-app` preserves CSI focus-reporting writes on window focus changes and
  dispatches a typed native-window focus-change hook with the active pane id plus
  focused/unfocused state. Lua event wiring remains later parity work.
- `rssh-app` dispatches a typed native-window resize hook after successful
  terminal/runtime resize, carrying the active pane id, pixel size, and terminal
  rows/columns. Lua event wiring and fullscreen dimension metadata remain later
  parity work.
- `rssh-app` dispatches a typed native-window open-uri hook for ctrl-clicked OSC
  8 hyperlinks before invoking the default opener. Returning `false` suppresses
  the default opener; Lua event wiring and full
  `CompleteSelectionOrOpenLinkAtMouseCursor` action coverage remain later parity
  work.
- `rssh-app` answers WezTerm/iTerm2-compatible `OSC 1337;ReportCellSize`
  queries with the current fixed cell pixel dimensions, alongside the existing
  xterm cell/window size query responses.
- `rssh-app` tracks kitty keyboard progressive-enhancement flags from
  `CSI = flags ; mode u` plus `CSI > flags u` / `CSI < n u` in both native
  runtime and console filtering, consumes those negotiation sequences, applies
  replace/set/reset mode semantics, and answers `CSI ? u` with the current
  flags. When the kitty disambiguate flag is active, console and native-window
  input encode Ctrl/Alt ASCII character keys as `CSI-u` events while leaving
  plain text input on the legacy path; when the kitty report-all flag is active,
  plain text keys plus Enter/Tab/Backspace are encoded as canonical `CSI-u`
  events, and navigation/editing keys, F1-F12, and F13-F35 use kitty canonical
  functional-key forms under disambiguate/report-all modes. Keypad keys use
  kitty KP_* private-use codepoints when kitty keyboard flags request CSI-u
  reporting, and kitty private-use functional codes cover CapsLock, ScrollLock,
  NumLock, PrintScreen, Pause, and Menu/ContextMenu in console and
  native-window paths, plus media transport, track, record, and volume keys
  where the input backend exposes them. Kitty event-type reporting is supported
  for repeat/release events using `modifier:event` subfields, and
  associated-text third fields are encoded when flag 16 is active alongside
  report-all. Console and native-window
  text-key input report kitty alternate shifted key subfields when flag 4 is
  active, and native-window input additionally reports printable PC-101 physical
  base-layout subfields; broader alternate-key variants remain a later protocol
  slice. Native-window kitty modifier encoding includes Super/Cmd/Windows bits
  and console kitty modifier encoding includes crossterm-provided
  Super/Hyper/Meta plus CapsLock/NumLock state bits and modifier-key
  private-use codepoints for left/right Shift/Ctrl/Alt/Super/Hyper/Meta plus
  ISO level shifts, while xterm legacy modifier encoding remains
  shift/alt/control compatible.
  Xterm `modifyOtherKeys` is also tracked from
  `CSI > 4 ; N m`, answered through `CSI ? 4 m`, and used for modified
  other-key input in console and native-window paths.
- `rssh-app` tracks xterm OSC 4/10/11/12 color changes and answers color
  queries, including WezTerm-style multiple OSC 4 palette index/color pairs in
  one sequence, multi-index OSC 4 queries, `#RGB`/`#RRGGBB` hex color specs,
  and RGBA dynamic foreground/background/cursor color specs.
- `rssh-app` answers XTGETTCAP queries for the WezTerm `Sync`
  synchronized-output template, matching the already-supported xterm
  `ESC[?2026h/l` protocol.
- `rssh-app` answers XTGETTCAP queries for WezTerm-style overline,
  strikethrough, default-color, palette-reset, `sgr`/`sgr0`, standout, and
  conditional select-color terminfo templates alongside the existing italic,
  underline, underline-color, and true-color capabilities.
- `rssh-app` answers XTGETTCAP queries for tab-stop/backtab,
  erase-character, repeat-character, scroll-region, indexed scroll, Backspace,
  BackTab, keypad-enter, SGR mouse templates (`kmous`/`XM`/`xm`),
  shifted navigation/editing, and WezTerm `kf13`-`kf63`
  modified function-key templates that match the implemented parser and input
  encoder behavior.
- `rssh-app` answers XTGETTCAP `co`/`li` and official WezTerm `cols`/`lines`
  numeric names from the current runtime size, plus `it=8` for the default tab
  interval and `pairs=32767`.
- `rssh-app` answers XTGETTCAP queries for WezTerm basic control,
  save/restore, and ACS character-set templates (`bel`, `cr`, `ind`, `ri`,
  `sc`, `rc`, `smacs`, `rmacs`) using sequences handled by the terminal parser.
- `rssh-app` answers XTGETTCAP queries for WezTerm cursor-position and
  device-attribute query templates (`u6`, `u7`, `u8`, `u9`) matching the
  existing CSI query response paths.
- `rssh-app` answers XTGETTCAP `civis`, official WezTerm `cnorm`, and `cvvis`
  cursor visibility/blink templates backed by `?25` and `?12` mode tracking.
- `rssh-app` tracks xterm Meta-key mode `?1034` through ESC/C1 CSI private-mode
  toggles, reports it through DECRQM, and answers XTGETTCAP `km`/`smm`/`rmm`
  with WezTerm Meta-key capabilities.
- `rssh-app` answers the remaining official WezTerm XTGETTCAP boolean names
  (`am`, `bce`, `ccc`, `hs`, `mc5i`, `mir`, `msgr`, `npc`, `Su`, `xenl`) plus
  `flash`, printer (`mc0`/`mc4`/`mc5`), memory-lock (`meml`/`memu`), and `rs1`
  reset templates.
- `rssh-app` answers XTGETTCAP queries for WezTerm title/status-line and
  palette-initialization templates (`tsl`, `fsl`, `dsl`, `initc`) backed by the
  existing OSC title and OSC 4 color handling paths.
- `rssh-app` answers XTGETTCAP `is2` and `rs2` with WezTerm reset/init
  templates backed by DECSTR, mode reset, and numeric-keypad handling paths.
- `rssh-app` answers XTGETTCAP queries for WezTerm keypad transmit templates
  (`smkx`, `rmkx`) backed by the existing application cursor-key and
  application-keypad mode tracking/input paths.
- `rssh-app` answers XTGETTCAP `kb2` with the WezTerm keypad-center template
  and encodes console `KeypadBegin` as `ESC O E` while preserving keypad digit
  5 as `ESC O u` in application-keypad mode.
- `rssh-core` includes explicit tab title state and a `SetTabTitle` action.
- `rssh-terminal` records OSC 0/1/2 titles plus Sun OSC L/l aliases, so
  WezTerm-compatible title/icon-title sequences can feed active-pane tab-title
  fallback labels.
- `rssh-terminal` saves and restores the tracked terminal title through xterm
  title-stack `CSI 22;0;0t` and `CSI 23;0;0t`, and `rssh-app` advertises the
  matching WezTerm `smcup`/`rmcup` alternate-screen templates.
- `rssh-terminal` implements DECALN `ESC # 8` screen alignment display by
  filling the visible grid with `E` cells and resetting margins/origin mode.
- `rssh-terminal` implements DECSTR `CSI ! p` soft reset, restoring
  insert/replace mode, origin mode, scroll region, G0 character set, and
  saved-cursor state without clearing visible cells or scrollback; app runtime
  and console filtering track ESC and C1 CSI forms for mode-status replies.
- `rssh-terminal` ignores WezTerm-documented non-printing C0 controls while
  preserving BEL/BS/HT/LF/VT/FF/CR/ESC special handling.
- `rssh-terminal` and `rssh-renderer` preserve WezTerm SGR mode 6 RGBA colors
  for foreground, background, and underline color state, and app-shell DECRQSS
  SGR responses serialize those alpha-bearing colors.
- `rssh-terminal` and `rssh-renderer` preserve WezTerm SGR 73/74/75
  vertical-align state for superscript, subscript, and baseline, and app-shell
  DECRQSS SGR responses serialize active 73/74 state.
- `rssh-app` answers WezTerm-documented DECRQSS `"` `p` conformance-level and
  `s` left/right-margin queries in both native runtime and console output
  filter paths. The left/right-margin response reports the modeled DECSLRM
  state, and DECRQM reports DECLRMM `?69` for ESC and C1 CSI forms.
- `rssh-app` includes a command-palette `Rename Tab` entry that sets an explicit
  title for the active tab using the current visible tab title as its base, and
  supports `rename tab <title>` queries for arbitrary user-entered titles.
- `rssh-app` reserves one native-window row for a basic tab bar, renders
  workspace/tab/pane-count state plus tab titles, prefers explicit tab titles,
  falls back to each tab's active-pane terminal title, activates tabs by mouse
  click, and exposes a clickable close marker per tab. Closing the final tab
  requests native-window shutdown through the same close lifecycle path as
  command palette close.
- The tab bar also exposes a clickable `+` button that creates and activates a
  new tab through the same app-shell `NewTab` action used by keyboard shortcuts
  and the command palette.
- `rssh-app` renders basic right/down pane split layouts by clipping and placing
  each pane snapshot into its app-shell split region with separator cells.
- `rssh-app` maps mouse clicks and wheel input to the pane under the cursor, so
  split panes can be focused and scrolled independently at the native-window
  layer.
- `rssh-app` supports pane resize actions through `Ctrl+Shift+Alt+Arrow` and
  command palette entries. Resize state is stored in the app-shell split model
  and applied when rendering split regions.
- `rssh-app` supports mouse drag resizing on rendered split separators, feeding
  drag distance back through the same app-shell resize action path.
- `rssh-app` supports pane zoom through `Ctrl+Shift+Z` and the command palette.
  `rssh-core` exposes both `TogglePaneZoom` and WezTerm-style
  `SetPaneZoomState` for explicit zoom/unzoom. The zoomed pane fills the tab
  terminal region until zoom is toggled off, explicitly unzoomed, or a
  pane-switch action unzooms before activating another pane.
- `rssh-app` now follows WezTerm-style close lifecycle behavior for command
  palette close actions: closing the last pane in a tab closes that tab when a
  neighboring tab exists, and closing the final tab/pane requests native-window
  shutdown from the window manager.
- Native window title surfaces app-shell state as `[workspace:X tab:Y pane:Z]` so
  smoke runs can verify transitions without opening multiple PTY sessions.

## Known Limitations

- Multi-PTY runtime orchestration is still basic and process-local; there is no
  mux server/client or remote domain attachment yet.
- The tab bar is basic text UI only; Lua/custom tab title formatting,
  external CLI/mux tab-title control, richer new-tab launcher behavior,
  pane-local scrollbar UI, richer split-drag affordances, and richer focus
  indicators are not yet implemented.
- Pane select Activate, swap, MoveToNewTab, and MoveToNewWindow action paths are
  implemented. MoveToNewWindow can now produce a detached native-window app
  state with the selected pane runtime, and the event loop can materialize it as
  an additional OS window. Platform focus/activation polish is still pending.
- No mux/domain model exists yet beyond action/state support.
- Command palette UX is minimal: discovery, fuzzy filtering, and configurable
  bindings are still pending.
- No GPU text shaping/fallback or ligature parity work yet.
- OSC 133 support currently records and extracts semantic zones in the terminal
  core and copy mode can move between zones across retained scrollback,
  including typed Prompt/Input/Output movement; OSC 7 cwd metadata,
  `OSC 1337;SetUserVar`, and `OSC 1337;SetBadgeFormat` metadata are recorded
  and synced per pane, and `OSC 1337;File=...` inline image metadata/payload is
  retained by the terminal core, surfaced in render snapshots, and drawn for
  PNG/JPEG/GIF payloads with delay-aware animated GIF frame selection by elapsed
  render time plus Kitty direct single-block/chunked and zlib-compressed raw
  RGB/RGBA and encoded payloads, plus minimal Kitty stored
  `a=t`/placed `a=p` images by image id or image number, regular-file `t=f`
  simple-file transfers with optional `O`/`S` slicing, temporary-file `t=t`
  transfers with guarded deletion, placement-id replacement, placement cursor
  movement with `C=1` suppression, and basic `a=d`
  image/image-number/image-range/placement/cell/row/column/z-index
  deletion, source-rectangle cropping, `X`/`Y` target pixel offsets, z-index
  layer ordering, and basic Kitty `a=q` plus stored-image query and
  stored-placement `OK`/`ENOENT` response writeback with `q=1`/`q=2`
  suppression, terminal erase display cleanup for retained inline images,
  `?1049` alternate-screen image isolation, plus basic Sixel DCS `q`
  bitmap rendering
  with RGB/HLS palette and raster-attribute pixel dimensions, with typed
  native-window user-var change hooks for changed pane values, while
  Lua pane APIs/events, automatic animated GIF refresh/invalidation scheduling,
  Kitty shared-memory transfers, remaining richer placement controls, broader
  query responses beyond current direct payload and stored-image existence
  checks, full Sixel protocol coverage, badge rendering/status formatting, and
  configurable key tables are not implemented yet.
- Kitty keyboard negotiation state is tracked and queryable through both
  `CSI = flags ; mode u` and push/pop forms, and disambiguated Ctrl/Alt ASCII
  character keys plus report-all plain text keys and Enter/Tab/Backspace use
  `CSI-u`; navigation/editing keys, F1-F12, and F13-F35 also use kitty
  canonical functional-key forms under disambiguate/report-all modes, keypad
  keys use kitty KP_* private-use codepoints under CSI-u reporting, and
  CapsLock, ScrollLock, NumLock, PrintScreen, Pause, and Menu/ContextMenu use
  kitty private-use functional codepoints, as do media transport, track, record,
  and volume keys exposed by the active input backend. Repeat/release events use
  kitty event-type subfields when flag 2 is active. Associated-text third fields
  are emitted when flag 16 is active with
  report-all. Console and native-window printable text keys emit kitty alternate
  shifted subfields when flag 4 is active, with native-window input also
  emitting PC-101 physical base-layout subfields and Super/Cmd/Windows modifier
  bits, and console input including crossterm-provided Super/Hyper/Meta plus
  CapsLock/NumLock state modifier bits plus modifier-key private-use codepoints
  for left/right Shift/Ctrl/Alt/Super/Hyper/Meta and ISO level shifts; broader
  alternate-key variants are still pending. Xterm
  `modifyOtherKeys` mode 0/1/2 negotiation and modified other-key encoding are
  implemented for console and native-window input.

## Run

Start a native window (v1 behavior still starts with one PTY session):

```powershell
cargo run -p rssh-app -- window --frames 120
```

Start from a custom command:

```powershell
cargo run -p rssh-app -- window --frames 30 -- cmd.exe /C echo app-shell-smoke
```

## Verification

Run:

```powershell
cargo test -p rssh-core app_shell
cargo test -p rssh-core action_
cargo test -p rssh-core set_tab_title
cargo test -p rssh-core action_close_tab_can_select_last_active_tab
cargo test -p rssh-core action_move_tab_reorders
cargo test -p rssh-core activate_pane_direction
cargo test -p rssh-core action_rotate_panes
cargo test -p rssh-core action_set_pane_zoom_state
cargo test -p rssh-core swap_panes
cargo test -p rssh-core move_pane_to_new_tab
cargo test -p rssh-core move_pane_to_new_window
cargo test -p rssh-core pending_window_can_be_consumed
cargo test -p rssh-app window_app_starts_with_default_shell_state
cargo test -p rssh-app shortcut
cargo test -p rssh-app recognizes_default_tab_navigation_shortcuts
cargo test -p rssh-app recognizes_default_tab_move_shortcuts
cargo test -p rssh-app recognizes_default_tab_move_shortcuts_with_alt
cargo test -p rssh-app recognizes_default_alt_split_shortcuts
cargo test -p rssh-app recognizes_default_pane_navigation_shortcuts
cargo test -p rssh-app recognizes_default_pane_resize_shortcuts
cargo test -p rssh-app recognizes_default_pane_zoom_shortcut
cargo test -p rssh-app window_app_dispatches_new_tab_action
cargo test -p rssh-app tab_bar
cargo test -p rssh-app window_app_clicking_tab_bar_close_marker
cargo test -p rssh-app window_app_clicking_tab_bar_new_tab_button
cargo test -p rssh-app window_app_dispatches_palette_rename_tab_command
cargo test -p rssh-app window_app_dispatches_palette_clear_selection_command
cargo test -p rssh-app window_app_dispatches_palette_copy_to_clipboard_command
cargo test -p rssh-app window_app_dispatches_palette_copy_to_primary_selection_command
cargo test -p rssh-app window_app_dispatches_palette_copy_to_clipboard_and_primary_selection_command
cargo test -p rssh-app window_app_dispatches_palette_paste_from_clipboard_command
cargo test -p rssh-app window_app_dispatches_palette_paste_from_primary_selection_command
cargo test -p rssh-app maps_window_copy_shortcuts_to_wezterm_destinations
cargo test -p rssh-app maps_window_paste_shortcuts_to_wezterm_sources
cargo test -p rssh-app window_app_dispatches_palette_reset_terminal_command
cargo test -p rssh-terminal terminal_tracks_osc133_prompt_rows_across_scrollback
cargo test -p rssh-terminal terminal_tracks_osc133_semantic_zones
cargo test -p rssh-terminal terminal_resets_osc133_line_input_after_newline
cargo test -p rssh-terminal terminal_extracts_text_from_semantic_zone
cargo test -p rssh-terminal terminal_extracts_multiline_semantic_zone_from_scrollback
cargo test -p rssh-terminal terminal_text_from_region_unwraps_soft_wrapped_lines_across_scrollback
cargo test -p rssh-app window_copy_mode_moves_by_semantic_zone
cargo test -p rssh-app semantic_zone_type
cargo test -p rssh-app window_copy_mode_semantic_zone_movement_scrolls_into_scrollback
cargo test -p rssh-app window_copy_mode_selection_copies_across_scrollback_viewports
cargo test -p rssh-app window_copy_mode_ctrl_v_uses_block_selection
cargo test -p rssh-app window_copy_mode_vertical_movement_scrolls_across_scrollback
cargo test -p rssh-app window_copy_mode_page_movement_scrolls_across_scrollback
cargo test -p rssh-app window_copy_mode_g_and_shift_g_move_to_scrollback_extents
cargo test -p rssh-app window_copy_mode_uppercase_no_modifier_uses_wezterm_default_bindings
cargo test -p rssh-app window_copy_mode_line_content_movement_uses_non_space_cells
cargo test -p rssh-app window_copy_mode_word_movement_uses_wezterm_default_bindings
cargo test -p rssh-app window_copy_mode_word_movement_crosses_scrollback_rows
cargo test -p rssh-app window_copy_mode_jump_forward_repeat_and_reverse_use_wezterm_bindings
cargo test -p rssh-app window_copy_mode_jump_backward_uses_wezterm_bindings
cargo test -p rssh-app selection_other
cargo test -p rssh-app window_copy_mode_search_keeps_copy_mode_and_steps_matches
cargo test -p rssh-app window_copy_mode_search_page_navigation_skips_visible_page_matches
cargo test -p rssh-app window_copy_mode_search_cycles_match_type
cargo test -p rssh-app window_app_dispatches_palette_scrollback_navigation_commands
cargo test -p rssh-app window_app_dispatches_palette_scroll_to_prompt_commands
cargo test -p rssh-app window_app_dispatches_palette_activate_pane_left_command
cargo test -p rssh-app split_pane
cargo test -p rssh-app window_app_dragging_right_split_separator_resizes_panes
cargo test -p rssh-app pane_select
cargo test -p rssh-app pane_swap
cargo test -p rssh-app move_to_new_tab
cargo test -p rssh-app move_to_new_window
cargo test -p rssh-app consumes_pending_new_window
cargo test -p rssh-app window_manager_collects_detached_app
cargo test -p rssh-app window_app_palette_close
cargo test -p rssh-app palette
cargo test -p rssh-app window_title_reports_app_shell_state
cargo test -p rssh-app copy_mode
```

## Next Milestone

- App Shell v2: harden pane-local selection, scrollbars, and runtime lifecycle.
- Harden multi-window focus/lifecycle behavior, then add pane focus UI and mouse
  drag split resizing.
- Add mux/domain, protocol, and renderer parity work.
