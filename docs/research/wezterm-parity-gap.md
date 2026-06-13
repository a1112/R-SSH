# WezTerm Parity Gap Tracker (2026-06-09)

This tracker is scoped to MVP 6 (App Shell v1). It captures what is complete and
what remains before WezTerm-style parity in key UX/composition areas.

## App-Shell Parity

| Area | WezTerm baseline | R-SSH App Shell v1 | Status |
| --- | --- | --- | --- |
| Tabs | Dynamic tab model, selection, close, rename, relative/absolute movement, numbering, visible tab bar, explicit tab title, active-pane title fallback, close-tab last-active policy | Workspace/tab model with active ID/actions, indexed activation, explicit tab title state/action, relative and absolute tab-order movement, wrapping/no-wrap relative tab activation, command-palette Rename Tab including `rename tab <title>` queries, keyboard dispatch, and a rendered native-window tab bar that prefers explicit tab titles before falling back to active-pane terminal titles from OSC 0/1/2 and Sun OSC L/l, with click activation, close markers, a new-tab button, and app-shell support for default-left or last-active close selection policy | ✅ Partial UI |
| Tab/pane navigation keys | `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+PageUp/Down`, `Ctrl+Shift+1..9`, `ActivateTab`, `ActivateLastTab`, `ActivateTabRelative`, `ActivateTabRelativeNoWrap`, `ActivatePaneByIndex`, focus pane shortcuts, `Ctrl+Shift+Alt+Arrow` resize defaults, `Ctrl+Shift+Z` zoom | `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+PageUp/Down`, `Ctrl+Shift+1..9` via zero-based/negative `ActivateTabIndex`, command-palette Activate Last Tab, command-palette wrapping and no-wrap Next/Previous Tab, command-palette Activate Pane 1..4, `Ctrl+Shift+Arrow` directional pane activation with command-palette entries, move/resize-related defaults, `Ctrl+Shift+Alt+Arrow` resize, and `Ctrl+Shift+Z` zoom | ✅ |
| Panes | Split tree with active focus, indexed focus, focus navigation, resize, toggle/set zoom, pane rotation, pane-select modal, per-pane state | Pane model with ordered focus list, index activation, split metadata/deltas, zoomed-pane state with toggle plus explicit `SetPaneZoomState`, pane rotation preserving split positions/size deltas, per-pane snapshots, rendered split separators, click-to-focus, pane-local wheel routing, keyboard/palette split resizing, toggle zoom, and pane-select Activate/Swap/MoveToNewTab/MoveToNewWindow action paths | ✅ Partial UI |
| Workspaces | Domain-like workspace collection with active workspace switching | Workspace model with named workspaces and switch/rename action support | ✅ Partial |
| Action surface | Typed key assignments, key tables, command palette binding | `AppAction` typed command model and key binding path in app | ✅ Partial |
| Command palette | `ActivateCommandPalette` + Lua extension points | Minimal `Ctrl+Shift+P` palette with tab/pane/workspace actions implemented (`WindowCommand` execution only) | ✅ |
| Quick select | pattern matching mode for URLs/files/IP/email with label-based copy/paste selection | Implemented in v1 for Ctrl+Shift+Space quick-select matching (`https?`, `file://`, `git@`, `git://`, `ssh://`, `ftp://`, markdown URLs, diff paths, docker SHA values, paths, colors, UUID/IPFS/SHA hashes, IPv4/IPv6, hex addresses, long numbers, email) with `Esc`, Tab/arrow navigation, `Ctrl+N`/`Ctrl+P`, PageDown/PageUp page-wise navigation, and WezTerm-style Enter PriorMatch behavior plus label input: lowercase labels copy the match to ClipboardAndPrimarySelection, uppercase labels paste it into the pane | ✅ |
| Copy mode | Vim-like keyboard selection in scrollback, copy, semantic-zone movement, and selection actions such as `ClearSelection` | Terminal mouse select exists; copy-mode (Ctrl+Shift+X, Space/`v`, uppercase `V`, movement, copy/exit, line/char select) implemented, with command-palette Clear Selection, cross-scrollback semantic-zone movement via `z`/`Shift+Z`, typed Prompt/Input/Output movement via `Alt+P`, `Alt+I`, and `Alt+O`/`Alt+Z`, source-row selection/copy across retained history, `y` CopyTo ClipboardAndPrimarySelection plus ScrollToBottom/Close, Cell selection via Space/`v`, Line selection via uppercase no-modifier or shifted `V`, rectangular block selection via `Ctrl+V`, vertical/page movement through retained history, Enter/CR `MoveToStartOfNextLine`, `g`/`Shift+G` scrollback top/bottom, viewport `H`/`M`/`L` movement with shifted and uppercase no-modifier events, content-aware `^`/`Alt+m`/`$`/End first/last non-space movement, WezTerm-style word movement via `w`/`b`/`e`, Tab/Shift+Tab, Alt+Left/Right, and Alt+F/B, jump-to-char via `f`/`t`/`F`/`T` plus `;`/`,` repeat, selection-end movement via `o`/`O`, ordinary copy-mode close with `ScrollToBottom` before `Close`, Escape/character ESC close in copy and search modes with search-status cleanup, `Ctrl+Shift+P` command-palette and `Ctrl+Shift+T` app-shell fallback from copy and copy-mode search, copy-mode search input plus next/prior match navigation including character CR PriorMatch, PageUp/PageDown page-wise match navigation, and `Ctrl+R` match-type cycling; configurable key-table movement remains open | ✅ Partial |
| Clipboard actions | `CopyTo` and `PasteFrom` for Clipboard/PrimarySelection buffers | Command-palette Copy To Clipboard (`CopyTo('Clipboard')`) and Paste From Clipboard (`PasteFrom('Clipboard')`) cover the system clipboard; OSC 52 and iTerm2 `OSC 1337;Copy=;base64` writes from ESC plus UTF-8 C1 OSC/ST active/inactive pane output, with legacy raw C1 compatibility, route through the same clipboard policy path; Copy To Primary Selection, Copy To Clipboard And Primary Selection, Paste From Primary Selection, and `Ctrl+Insert`/`Shift+Insert` shortcut classification cover the PrimarySelection action routing; native OS PrimarySelection backend support remains open | ✅ Partial |
| Terminal reset | `ResetTerminal` injects `ESC c` on the pane output side | Command-palette Reset Terminal injects RIS into the active pane output path, resetting visible terminal state and scrollback through the terminal core | ✅ |
| Scrollback erase | `ClearScrollback('ScrollbackOnly')` and `ClearScrollback('ScrollbackAndViewport')` | Command-palette Clear Scrollback covers `ScrollbackOnly` on the active pane output side while preserving the viewport; Clear Scrollback And Viewport covers `ScrollbackAndViewport` by clearing active-pane history plus the viewport and preserving the prompt/cursor row as the new first visible line | ✅ |
| Scrollback navigation | `ScrollToTop`, `ScrollToBottom`, `ScrollByPage`, `ScrollByLine`, prompt-aware scroll actions | Scrollbar/mouse wheel support plus command-palette Scroll To Top/Bottom, Scroll Page Up/Down, Scroll Line Up/Down, and Scroll To Previous/Next Prompt backed by OSC 133 prompt row markers | ✅ Partial |
| Semantic zones | OSC 133 Prompt/Input/Output/CommandFinished zones, command status metadata, `pane:get_semantic_zones()`, `pane:get_semantic_zone_at()`, text extraction, semantic-zone copy-mode movement | Terminal core records OSC 133 Prompt/Input/Output semantic zones across retained scrollback and visible grid, including line-scoped `I` input markers, OSC 133 `D` command-finished rows with exit status and `aid`, retained row/column region extraction with soft-wrap logical-line unwrapping, semantic-zone text extraction, cross-scrollback copy-mode zone movement including typed Prompt/Input/Output filters, selection/copy across retained history, and content-aware `^`/`Alt+m`/`$`/End line movement; Lua pane APIs and configurable key-table bindings remain open | ✅ Partial |
| Working directory metadata | OSC 7 / shell integration cwd tracking used for new panes/tabs | Terminal core records OSC 7 and iTerm2 `OSC 1337;CurrentDir`; app-shell pane launch metadata is updated from active and inactive pane runtimes and inherited by new tabs/splits, with `file://` cwd URI decoding for PTY spawn | ✅ Partial |
| Shell user variables | iTerm2/WezTerm `OSC 1337;SetUserVar` pane metadata, `pane:get_user_vars()`, and `user-var-changed` events | Terminal core base64-decodes `SetUserVar` values into pane user-var metadata; app-shell stores them per pane for active and inactive runtimes; native window dispatches a typed user-var change hook for active/inactive pane metadata changes. Lua pane APIs/events remain open | ✅ Partial |
| iTerm2 badge metadata | `OSC 1337;SetBadgeFormat` base64 badge text | Terminal core base64-decodes `SetBadgeFormat` into terminal badge metadata; app-shell stores it per pane for active and inactive runtimes. Badge rendering and Lua/status formatting remain open | ✅ Partial |
| Notifications | OSC 9 iTerm2 notification and OSC 777 rxvt `notify` toast events | Native runtime extracts OSC 9 and OSC 777 `notify` events from ESC plus UTF-8 C1 OSC/ST active/inactive pane output, keeps legacy raw C1 compatibility, and dispatches them through the window notification handler. Native OS toast backend remains open | ✅ Partial |
| Pane progress | ConEmu-style `OSC 9;4;st;pr` progress state exposed through `pane:get_progress()` and tab/status formatting | Terminal runtime records `None`, percentage, error, and indeterminate progress states from ESC plus UTF-8 C1 OSC/ST 9;4 sequences, keeps legacy raw C1 compatibility, prevents progress reports from being misclassified as OSC 9 notifications, and syncs the latest progress into active/inactive app-shell pane metadata. Lua pane API exposure and tab/status formatting remain open | ✅ Partial |
| Bell events | `bell` window event when ASCII BEL is emitted by any pane | Terminal core counts BEL events; native window dispatches a typed pane-scoped bell hook for active and inactive pane output while preserving metrics. Lua event wiring plus audible/visual bell configuration remain open | ✅ Partial |
| Window focus events | `window-focus-changed` event with the active pane when GUI focus changes | Native window dispatches a typed focus-change hook with focused/unfocused state and the active pane while preserving CSI focus-reporting writes. Lua event wiring remains open | ✅ Partial |
| Window resize events | `window-resized` event with active pane, dimensions available via window APIs | Native window dispatches a typed resize hook after successful terminal/PTY resize with active pane id, pixel size, and terminal rows/columns. Lua event wiring and fullscreen dimension metadata remain open | ✅ Partial |
| Open URI events | `open-uri` window event before default URI opening; returning `false` suppresses default handling | Native window dispatches a typed open-uri hook for ctrl-clicked OSC 8 hyperlinks with the active pane id and URI before invoking the default opener. Handlers can suppress the default opener by returning `false`. Lua event wiring and full `CompleteSelectionOrOpenLinkAtMouseCursor` action coverage remain open | ✅ Partial |
| Search UX | Search in pane + copy-mode navigation | Terminal scrollback search exists (`Ctrl-F`, `Shift+F3`) with WezTerm-style search table navigation via Down/Up, `Ctrl+N`/`Ctrl+P`, PageUp/PageDown, `Ctrl+R` match-type cycling, `Ctrl+U` clear-pattern, character ESC close, and current-selection first-line query prefill; copy mode now keeps copy-mode state while searching with `/`/`?`, Down/`Ctrl+N`, Up/Enter/CR/`Ctrl+P`, PageUp/PageDown match navigation, and `Ctrl+R` match-type cycling across case-sensitive, case-insensitive, and regex search | ✅ Partial |

## Terminal/Mux/System Parity

| Area | WezTerm baseline | R-SSH App Shell v1 | Status |
| --- | --- | --- | --- |
| Multiplexer | Local mux daemon, domains, SSH/TLS/Unix remote attachment | One PTY process model only | ❌ |
| Visual split layout | Split bars, size controls, zoomed panes, pane-select labels, pane focus visuals | Basic right/down split layout renders pane snapshots with separators; click/wheel hit-testing, keyboard/palette resize, mouse drag resizing, zoom rendering, and pane-select labels are pane-local; focus polish remains | ⚠ Partial |
| GPU rendering | `wgpu` production render path with advanced shaping and font fallback | CPU bitmap demo renderer path used, limited shaping/fallback | ❌ |
| Font shaping | Ligatures, fallback stacks, color emoji | Basic fixed bitmap renderer | ❌ |
| Unicode | Broad protocol + shaping parity in progress | Strong xterm/core baseline plus documented C0 control handling, xterm cell/window size queries, DECALN `ESC # 8` screen alignment fill, DECSTR `CSI ! p` soft reset for insert/origin/scroll-region/left-right-margin/charset/saved-cursor state, cursor visibility/blink tracking for `?25`/`?12`, `DECLRMM ?69` and `DECSLRM` left-right margin state for CR/BS and origin-mode `CUP`/`HVP`, SGR indexed/RGB/RGBA color state including WezTerm mode 6 alpha values, WezTerm SGR 73/74/75 vertical-align state, DECRQSS SGR/cursor/vertical-margin/conformance/left-right-margin query responses, OSC 4/10/11/12 color queries/changes including multi-pair OSC 4 palette updates and multi-index queries plus `#RGB`/`#RRGGBB` and RGBA dynamic color specs, iTerm2 `OSC 1337;ReportCellSize`, XTGETTCAP `Sync` synchronized-output template reporting, XTGETTCAP WezTerm SGR/style/select-color/title/title-stack/palette/keypad/reset-init/cursor-visibility/meta-key/printer/memory-lock templates, XTGETTCAP official boolean and numeric size/tab-interval reporting, XTGETTCAP tab-stop/erase/repeat/scroll-region/control/save-restore/ACS/query/mouse template reporting for implemented parser/input controls, and XTGETTCAP `kf13`-`kf63` modified function-key reporting for implemented input encoders; advanced sequences pending | ⚠ Partial |
| Keyboard protocols | xterm defaults plus Meta-key mode, `modifyOtherKeys`, CSI-u, and kitty progressive keyboard handling | xterm-style key encoding, application cursor/keypad modes, xterm Meta-key mode `?1034` with DECRQM and XTGETTCAP `km`/`smm`/`rmm`, xterm `modifyOtherKeys`, and kitty keyboard progressive-enhancement state negotiation are implemented: native runtime and console filtering consume `CSI > 4 ; N m`, answer `CSI ? 4 m`, encode modified other keys with `CSI 27 ; modifier ; code ~`, consume `CSI = flags ; mode u` plus `CSI > flags u`/`CSI < n u`, maintain the kitty flags stack, answer `CSI ? u`, encode Ctrl/Alt ASCII character keys as kitty `CSI-u` events when the disambiguate flag is active, encode plain text keys plus Enter/Tab/Backspace as canonical `CSI-u` when report-all is active, use kitty canonical forms for navigation, editing, F1-F12, and F13-F35 functional keys, encode keypad keys with kitty KP_* private-use codepoints under CSI-u reporting, encode CapsLock/ScrollLock/NumLock/PrintScreen/Pause/Menu private-use functional key codepoints plus media transport/track/record/volume key codepoints, report repeat/release event types with `modifier:event` subfields, emit associated-text third fields when flag 16 is active with report-all, emit console/native-window text-key shifted alternate subfields when flag 4 is active, report crossterm-provided console Super/Hyper/Meta and CapsLock/NumLock modifier bits plus modifier-key private-use codepoints for left/right Shift/Ctrl/Alt/Super/Hyper/Meta and ISO level shifts, emit native-window printable PC-101 physical base-layout alternate subfields, and report native-window Super/Cmd/Windows modifier bits in kitty sequences; broader kitty alternate-key variants remain open | ⚠ Partial |
| Graphics/protocols | Kitty, iTerm2, sixel/image protocol support | iTerm2 `ReportCellSize` handshake is implemented for cell metrics, iTerm2/WezTerm `OSC 1337;File=...` inline image metadata and decoded payload bytes are retained by the terminal core, carried through render snapshots for live/scrollback/overlaid panes, and PNG/JPEG/GIF payloads are drawn into the framebuffer with delay-aware animated GIF frame selection by elapsed render time, cell/`px` dimensions, plus damage-region redraw coverage; Kitty Graphics Protocol direct `a=T` payloads are parsed and rendered for single-block and chunked, uncompressed and zlib-compressed raw RGB/RGBA plus encoded image data, regular-file `t=f` simple-file transfers are parsed/rendered with optional `O`/`S` file slicing, temporary-file `t=t` transfers are parsed/rendered with guarded `tty-graphics-protocol` temp-file deletion, minimal stored image flow supports `a=t,i=<id>` and omitted-action default `a=t` uploads with `i` OK and invalid-parameter/payload `EINVAL` responses, `a=t,I=<number>` terminal-assigned image-number uploads with `i`/`I` OK responses, and `a=p` placement by image id or image number at the current cursor, direct and stored placements support basic `x`/`y`/`w`/`h` source-rectangle cropping, single-axis `c`/`r` aspect-ratio derivation, `X`/`Y` target pixel offsets, and cursor movement with `C=1` suppression, direct `a=q` support queries return `OK`/`EINVAL` for single-block and chunked direct payloads without storing/displaying queried images, stored-image existence queries and stored placements return `OK` or `ENOENT` for present/missing image ids or image numbers, Kitty `q=1`/`q=2` OK/error response suppression is honored, `i`/`I` mutual exclusion is enforced, repeated `(image id, placement id)` pairs replace old placements, basic `a=d` deletion removes all visible Kitty placements, placements for a specific image id, placements for the latest image assigned to an image number, placements in an image-id range, a specific `(image id, placement id)` pair, cursor-cell placements, explicit-cell placements, visible-column placements, z-index placements, or cell-plus-z-index placements, basic `U=1` virtual placements, including combined `a=T,U=1` uploads, render from `U+10EEEE` placeholder cells with foreground image-id encoding, row/column diacritics, optional image-id high-byte diacritic, non-origin placeholder origin derivation, first-column row-only placeholders, stored left-cell inheritance for omitted placeholder diacritics, stale placeholder cleanup across control sequences, erase/reset cleanup for placeholder metadata, scroll-region movement plus scrollback rebase for placeholder metadata, and alternate-screen metadata isolation/restore, and visible placement deletion retains image data while a virtual placement still references it, terminal erase display cleanup removes retained inline images for `CSI 2J`/`CSI 3J`, `?1049` alternate-screen switching isolates main/alternate image placements, scroll operations move inline-image placements with affected text rows, and the renderer applies Kitty z-index layer ordering below/above text; basic Sixel DCS `q` payloads with VT340 default palette entries, RGB plus DEC HLS hue palette definitions, color selection, DCS `P1` macro pixel aspect, DECGRA `Pan`/`Pad` aspect override plus `Ph`/`Pv` minimum background dimensions, DCS `P2` transparent/opaque background mode, repeat introducers, carriage returns, sixel newlines, and WezTerm-style DECSDM `?80h` active-graphics-origin placement with preserved text cursor are normalized into raw RGBA inline images and rendered; xterm dynamic color query/change handling includes WezTerm-style multi-pair OSC 4 palette updates, multi-index queries, and RGBA dynamic foreground/background/cursor color specs; automatic animated GIF refresh/invalidation scheduling, Kitty shared-memory transfers/remaining richer placement controls/broader query responses beyond current direct/chunked direct payload and stored-image existence checks, full Sixel protocol coverage, and remaining sixel pan edge cases remain open | ⚠ Partial |
| Config layer | Lua config, events, hot reload, plugins | TOML profile system only for launch/runtimes | ⚠ Partial |
| Connectivity | Mux domains, serial/TLS domains, robust remote attach | Local PTY + SSH CLI/native russh paths, no mux domains | ⚠ Partial |

## What V1 Completes

- Deterministic `rssh-app window` startup shell state: one workspace, one tab,
  one pane (IDs start at `1`).
- Tab/pane/workspace action dispatch in `rssh-core` and native-window integration.
- Keyboard shortcuts for new tab, close tab, tab cycling, split-right,
  split-down, plus app-shell last-active tab tracking.
- App-shell state now exposes WezTerm-style indexed tab activation through
  `ActivateTabIndex`, with `Ctrl+Shift+1..9` routed to indices `0..7/-1`.
- App-shell state now exposes WezTerm-style `ActivateTabRelative` wrapping and
  `ActivateTabRelativeNoWrap` clamping; the command palette includes both
  wrapping and no-wrap Next/Previous Tab entries.
- `MoveTabRelative` now reorders the active tab within the current workspace
  while preserving it as the active tab.
- `MoveTab` now reorders the active tab to an absolute zero-based index, with
  command-palette Move Tab To 1..4 entries and typed out-of-range errors.
- Native window title includes shell state for easy smoke verification.
- Native window frame now reserves a one-row tab bar with workspace/tab state,
  explicit tab title priority, active-pane terminal title fallback, mouse
  activation, close markers, and a new-tab button.
- Terminal title state now records OSC 0/1/2 plus Sun OSC L/l aliases, matching
  WezTerm's title/icon-title compatibility for tab fallback labels.
- App runtime and console output filtering now track kitty keyboard
  progressive-enhancement flags from `CSI = flags ; mode u` plus
  `CSI > flags u` / `CSI < n u`, including replace/set/reset flag application,
  and answer `CSI ? u` with the current flags; console and native-window input
  now encode disambiguated Ctrl/Alt ASCII character keys as kitty `CSI-u`
  events when flag 1 is active, and report plain text keys plus
  Enter/Tab/Backspace as canonical `CSI-u` when flag 8 is active. Kitty
  canonical forms now cover navigation, editing, F1-F12, and F13-F35
  functional keys plus KP_* keypad private-use codepoints under
  disambiguate/report-all modes. Xterm `modifyOtherKeys` negotiation from
  `CSI > 4 ; N m`, `CSI ? 4 m` query replies, and modified other-key encoding
  are implemented in console and native-window input paths; kitty event-type
  reporting now covers repeat/release events with
  `modifier:event` subfields, CapsLock/ScrollLock/NumLock/PrintScreen/Pause
  and Menu/ContextMenu use kitty private-use functional key codepoints, and
  media transport, track, record, and volume keys use kitty private-use
  functional key codepoints where the input backend exposes them. Associated
  text third fields are emitted when flag 16 is active with report-all.
  Console and native-window text-key input
  now emits kitty shifted alternate subfields when flag 4 is active, and
  console input now reports crossterm-provided Super/Hyper/Meta plus
  CapsLock/NumLock state modifier bits plus modifier-key private-use codepoints
  for left/right Shift/Ctrl/Alt/Super/Hyper/Meta and ISO level shifts.
  Native-window input additionally emits printable PC-101 physical base-layout
  subfields plus Super/Cmd/Windows modifier bits; broader alternate-key
  variants remain a later keyboard-protocol slice.
- Terminal core now implements DECALN `ESC # 8` screen alignment display,
  filling the visible grid with `E` cells and resetting margins/origin mode.
- Terminal core now implements DECSTR `CSI ! p` soft reset without clearing
  cells or scrollback, and app runtime/console filtering track ESC plus C1
  DECSTR for mode reports while XTGETTCAP exposes WezTerm `is2`/`rs2`
  reset/init templates.
- Terminal core now ignores WezTerm-documented non-printing C0 controls while
  preserving BEL/BS/HT/LF/VT/FF/CR/ESC special handling.
- Terminal core and renderer now preserve WezTerm SGR mode 6 RGBA colors for
  foreground, background, and underline color state.
- Terminal core and renderer now preserve WezTerm SGR 73/74/75 vertical-align
  state for superscript, subscript, and baseline; app-shell DECRQSS SGR
  responses serialize active 73/74 state.
- App runtime and console output filtering now answer WezTerm-documented DECRQSS
  `"` `p` conformance-level and `s` left/right-margin queries. The `s` response
  reports modeled DECSLRM state, and DECRQM reports DECLRMM `?69` for ESC and
  C1 CSI forms.
- App runtime and console output filtering now answer XTGETTCAP `Sync` with the
  WezTerm synchronized-output terminfo template for the existing `2026` mode.
- App runtime and console output filtering now answer XTGETTCAP `Smol`,
  `smxx`, `rmxx`, `op`, `oc`, `sgr`, `sgr0`, `smso`, `rmso`, `setaf`, and
  `setab` using WezTerm-style style/color templates.
- App runtime and console output filtering now track cursor blink mode `?12`
  and Meta-key mode `?1034`, report them through DECRQM for ESC/C1 CSI forms,
  and answer XTGETTCAP `civis`, official WezTerm `cnorm`, `cvvis`, and
  `km`/`smm`/`rmm` cursor visibility/blink and Meta-key templates.
- App runtime and console output filtering now answer the remaining official
  WezTerm XTGETTCAP boolean names (`am`, `bce`, `ccc`, `hs`, `mc5i`, `mir`,
  `msgr`, `npc`, `Su`, `xenl`) plus `flash`, printer (`mc0`/`mc4`/`mc5`),
  memory-lock (`meml`/`memu`), and `rs1` reset templates.
- App runtime and console output filtering now answer XTGETTCAP `cbt`, `ht`,
  `hts`, `tbc`, `ech`, `rep`, `csr`, `indn`, `rin`, `kbs`, `kcbt`, `kent`,
  `kmous`, `XM`, `xm`, shifted navigation/editing key capabilities, and `kf13`-`kf63`
  modified function-key capabilities for behavior already covered by the parser
  and input encoders.
- App runtime and console output filtering now answer XTGETTCAP `co`/`li` and
  official WezTerm `cols`/`lines` numeric names from the current runtime size,
  plus `it=8` for the default tab interval and `pairs=32767`.
- App runtime and console output filtering now answer XTGETTCAP `bel`, `cr`,
  `ind`, `ri`, `sc`, `rc`, `cuu1`, `cud1`, `cuf1`, `cub1`, `dch`, `ich`,
  `dl`, `il`, and WezTerm-style ACS `smacs`/`rmacs` templates for sequences
  already handled by the parser.
- App runtime and console output filtering now answer XTGETTCAP `u6`, `u7`,
  `u8`, and `u9` with WezTerm cursor-position and device-attribute query
  templates for response paths already covered by the runtime.
- App runtime and console output filtering now answer XTGETTCAP `tsl`, `fsl`,
  `dsl`, and `initc` with WezTerm title/status-line and palette-initialization
  templates backed by existing OSC title and OSC 4 color handling.
- Terminal core now saves/restores the tracked terminal title via xterm
  title-stack `CSI 22;0;0t` and `CSI 23;0;0t`, and app runtime plus console
  output filtering now answer XTGETTCAP `smcup`/`rmcup` with WezTerm
  alternate-screen plus title-stack templates.
- App runtime and console output filtering now answer XTGETTCAP `smkx` and
  `rmkx` with WezTerm keypad transmit templates backed by existing application
  cursor-key and application-keypad mode tracking/input paths.
- App runtime and console output filtering now answer XTGETTCAP `kb2` with the
  WezTerm keypad-center template, and console input encodes `KeypadBegin` as
  `ESC O E` while preserving keypad digit 5 as `ESC O u` in application-keypad
  mode.
- Command palette now exposes Rename Tab, including `rename tab <title>` query
  input, and writes an explicit title for the active tab.
- Command palette now exposes WezTerm-style `ClearSelection`, clearing active
  selection state and removing rendered selection highlights.
- Command palette now exposes WezTerm-style `ClearScrollback('ScrollbackOnly')`,
  clearing active-pane history on the output side while preserving the viewport.
- Command palette now exposes WezTerm-style
  `ClearScrollback('ScrollbackAndViewport')`, clearing active-pane history plus
  the viewport while preserving the prompt/cursor row as the new first visible
  line.
- Command palette now exposes WezTerm-style `CopyTo('Clipboard')` as Copy To
  Clipboard for the active selection.
- Command palette now exposes WezTerm-style `CopyTo('PrimarySelection')` and
  `CopyTo('ClipboardAndPrimarySelection')` routing for the active selection.
- Command palette now exposes WezTerm-style `PasteFrom('Clipboard')` as Paste
  From Clipboard for the active pane.
- Command palette now exposes WezTerm-style `PasteFrom('PrimarySelection')`
  routing for the active pane, and shortcut classification maps
  `Ctrl+Insert`/`Shift+Insert` to WezTerm's PrimarySelection defaults.
- Command palette now exposes WezTerm-style `ResetTerminal` as Reset Terminal,
  injecting RIS on the active pane output side.
- Command palette now exposes WezTerm-style scrollback navigation for top,
  bottom, page, line, and OSC 133 prompt movement.
- Terminal core now records OSC 133 Prompt/Input/Output semantic zones and can
  query zones by retained row/column.
- Terminal core now records WezTerm shell-integration OSC 133 `D`
  command-finished metadata, including the retained row, exit status, and
  `aid`.
- Terminal core now records OSC 7 and iTerm2 `OSC 1337;CurrentDir` current
  working directory metadata; app-shell launch metadata syncs it per pane,
  including inactive panes, inherits it for new tabs/splits, and decodes
  `file://` cwd URIs before PTY spawn.
- Terminal core now base64-decodes iTerm2/WezTerm `OSC 1337;SetUserVar`
  metadata into terminal user vars; app-shell syncs those values per pane for
  active and inactive pane runtimes, and the native window dispatches a typed
  user-var change hook when a pane value changes.
- Terminal core now base64-decodes iTerm2 `OSC 1337;SetBadgeFormat` metadata
  into terminal badge format state; app-shell syncs that value per pane for
  active and inactive pane runtimes.
- Terminal core now records iTerm2/WezTerm inline image `OSC 1337;File=...`
  metadata, decoded payload bytes, and retained-history cursor position.
  Renderer snapshots now expose those image items in live, scrollback, and
  overlaid pane views. The renderer now decodes PNG/JPEG/GIF payloads into the
  RGBA framebuffer with delay-aware animated GIF frame selection by elapsed
  render time, respects cell and `px` image dimensions, and redraws images when
  damage intersects covered image cells. The same image
  snapshot path now supports Kitty Graphics Protocol direct `a=T` payloads for
  single-block and chunked, uncompressed and zlib-compressed raw RGB/RGBA plus
  encoded image data, regular-file `t=f` simple-file transfers with optional
  `O`/`S` file slicing, temporary-file `t=t` transfers with guarded
  `tty-graphics-protocol` temp-file deletion, plus minimal `a=t,i=<id>`
  stored-image transmission plus omitted-action default `a=t` uploads with `i`
  OK and invalid-parameter/payload `EINVAL` responses, `a=t,I=<number>`
  terminal-assigned image-number uploads with `i`/`I` OK responses, and `a=p`
  placement by image id or image number at the current cursor.
  Basic source rectangles (`x`/`y`/`w`/`h`) crop direct and stored-placement
  source images, and `X`/`Y` target pixel offsets shift direct and stored
  placements relative to the placement cell. Placements that specify only `c`
  or only `r` derive the other cell axis from the source image or
  source-rectangle aspect ratio. Basic direct `a=q` support queries
  return `OK`/`EINVAL` for single-block and chunked direct payloads without
  storing/displaying queried images, stored-image existence queries and stored
  placements return `OK` or `ENOENT` for present/missing image ids or image numbers,
  Kitty `q=1`/`q=2` OK/error response suppression is honored, `i`/`I` mutual
  exclusion is enforced, image-id re-transmission clears existing visible
  placements before replacing stored bytes, and direct/stored placements
  advance the cursor by the placement cell rectangle unless `C=1` suppresses
  movement. Placement ids are tracked so repeated `(image id, placement id)`
  pairs replace old placements. Relative-placement `P`/`Q` references use
  `H`/`V` offsets for initial positioning when the parent placement exists and
  return `ENOPARENT` for missing parents instead of creating an ordinary
  placement at the cursor; deleting a parent placement cascades to relative
  child placements and drops child image data once unreferenced, scroll-region
  clipping that removes a parent placement deletes orphan relative children,
  and parent cycles are rejected with `ECYCLE`. Re-placing a parent placement
  moves relative descendants by the same cell delta. Relative chains are
  allowed up to eight parent levels and deeper chains return `ETOODEEP`. Basic
  `U=1` virtual placements are recorded for stored images or combined
  `a=T,U=1` uploads, and `U+10EEEE` Unicode placeholder cells with foreground
  image-id encoding plus row/column diacritics from Kitty's 0..255 placeholder
  table render those virtual placements. Placeholder image ids support the
  optional high-byte diacritic, non-origin placeholder cells derive the image
  origin from placeholder row/column, first-column placeholders can omit the
  column diacritic when only the row diacritic is present, adjacent placeholder
  cells inherit omitted row/column/high-byte diacritics from stored metadata for
  the screen cell to the left when foreground and underline colors match,
  pending placeholder state is closed across control sequences, and erase/reset
  paths clear stale placeholder metadata. Scroll-region movement and scrollback
  pruning rebase stored placeholder metadata with the text cells, and
  alternate-screen switching snapshots and isolates placeholder metadata with
  the main screen. Visible placement deletion, including uppercase
  all-placement deletion, keeps stored image data alive while a virtual
  placement still references that image; attempts to make a `U=1` virtual
  placement relative return `EINVAL`.
  Basic `a=d`
  deletion removes all live viewport visible Kitty placements while retaining
  scrollback placements, placements for a specific image id, placements for the
  latest image assigned to an image number, placements in an image-id range, a specific
  `(image id, placement id)` pair, cursor-cell placements, explicit-cell
  placements, visible-column placements, visible-row placements, z-index
  placements, or cell-plus-z-index placements. Position-oriented deletes leave
  Unicode-placeholder-derived renders intact until the underlying placeholder
  cell is overwritten or erased. The renderer applies Kitty
  z-index layer ordering, drawing negative z-index images below text,
  z-index values below `i32::MIN / 2` below non-default cell backgrounds, and
  non-negative z-index images above text in ascending z order with Kitty image
  id breaking ties for overlapping same-z images. Terminal erase
  display cleanup removes visible inline-image placements for `CSI 2J`, drops
  scrollback inline images for `CSI 3J`, and rebases retained visible image rows
  after scrollback clearing. Alternate-screen `?1049` switches isolate
  inline-image placements between main and alternate buffers, restoring main
  placements on exit and discarding alternate placements. Scroll operations move
  inline-image placements with affected text rows and drop placements that leave
  the scrolled region. Basic Sixel DCS `q` payloads with VT340 default palette
  entries, RGB plus DEC HLS hue palette definitions, color selection,
  DCS `P1` macro pixel aspect, DECGRA `Pan`/`Pad` aspect override plus
  `Ph`/`Pv` minimum background dimensions, DCS `P2`
  transparent/opaque background mode, repeat introducers, carriage returns,
  and sixel newlines are normalized into raw RGBA inline images. Default and
  `?80l` output starts at the text cursor and advances to the next terminal
  line; DECSDM `?80h` output starts at the active graphics-page origin and
  keeps the text cursor fixed. `?80` is reported through DECRQM/DECRPM and the
  image renders through the same snapshot path. Automatic animated GIF
  refresh/invalidation scheduling, Kitty shared-memory transfers,
  remaining richer placement controls, broader query responses beyond current
  direct/chunked direct payload and stored-image
  existence checks, full Sixel protocol coverage, sixel scrolling/pan edge
  cases, and pane sync remain open.
- App runtime now extracts WezTerm-documented OSC 9 and OSC 777 `notify`
  notification events from ESC plus UTF-8 C1 OSC/ST active and inactive pane
  output, with legacy raw C1 compatibility, and dispatches them through the
  native-window notification handler. Native OS toast integration remains open.
- App runtime now records WezTerm-documented ConEmu-style OSC 9;4 progress
  state as None, percentage, error, or indeterminate from ESC plus UTF-8 C1
  OSC/ST forms, does not treat progress reports as OSC 9 notifications, and
  syncs active/inactive pane progress into app-shell pane metadata. Lua pane API
  exposure remains open.
- Native window now dispatches typed pane-scoped bell hooks for ASCII BEL from
  active and inactive pane output while preserving bell metrics. Lua event wiring
  and audible/visual bell configuration remain open.
- Native window now dispatches a typed focus-change hook with the active pane
  and focused/unfocused state while preserving CSI focus-reporting writes.
  Lua event wiring remains open.
- Native window now dispatches a typed resize hook after successful terminal and
  PTY resize with the active pane id, pixel size, and terminal rows/columns.
  Lua event wiring and fullscreen dimension metadata remain open.
- Native window now dispatches a typed open-uri hook for ctrl-clicked OSC 8
  hyperlinks before invoking the default opener. Returning `false` suppresses
  the default opener; Lua event wiring and full
  `CompleteSelectionOrOpenLinkAtMouseCursor` coverage remain open.
- Terminal core can extract text from retained row/column regions and semantic
  zones while unwrapping soft-wrapped physical rows to logical-line text.
- Copy mode semantic-zone movement can scroll into retained history and supports
  typed Prompt/Input/Output filters.
- Copy mode source-row selection anchors now preserve and copy selections that
  span the live viewport and retained history.
- Copy mode `y` now follows WezTerm's default CopyTo
  ClipboardAndPrimarySelection, then ScrollToBottom and Close behavior.
- Copy mode Cell selection now covers WezTerm's default Space/`v` bindings.
- Copy mode Line selection now covers WezTerm's default uppercase no-modifier
  and shifted `V` bindings.
- Copy mode block selection now covers WezTerm's default `Ctrl+V` rectangular
  selection binding.
- Copy mode vertical and page movement now uses source-row coordinates and can
  scroll through retained history.
- Copy mode Enter and character CR (`\r`) now cover WezTerm's default
  `MoveToStartOfNextLine` binding.
- Copy mode `g`/`Shift+G` now move to scrollback top/bottom.
- Copy mode `H`/`M`/`L` now moves to viewport top/middle/bottom for both
  shifted and uppercase no-modifier default key-table events.
- Copy mode `^`/`Alt+m` and `$`/End now move to the first/last non-space cell in
  the current source row, matching WezTerm's content-aware line start/end
  actions.
- Copy mode word movement now covers WezTerm's default `w`/`b`/`e`,
  Tab/Shift+Tab, Alt+Left/Right, and Alt+F/B bindings across retained source
  rows.
- Copy mode jump-to-char now covers WezTerm's default `f`/`t`/`F`/`T`
  bindings plus `;` repeat and `,` reverse repeat on the current source row.
- Copy mode selection-end movement now covers WezTerm's default `o` and `O`
  bindings.
- Ordinary copy-mode close now follows WezTerm's default `ScrollToBottom` then
  `Close` behavior before exiting the overlay.
- Copy mode and copy-mode search close now cover WezTerm's character ESC
  (`\u{1b}`) defaults as well as Escape key events, and copy-mode search close
  clears the search status from the window title.
- Copy mode and copy-mode search now allow global command-palette and app-shell
  shortcuts such as `Ctrl+Shift+P` and `Ctrl+Shift+T` to fall through from the
  overlay, matching WezTerm key-table fallback behavior.
- Copy mode search now keeps copy-mode state while entering `/`/`?` queries and
  supports WezTerm-style next/prior match navigation with Down/`Ctrl+N` and
  Up/Enter/CR/`Ctrl+P`.
- Copy mode search now supports WezTerm-style page-wise match navigation with
  PageDown/PageUp.
- Copy mode search now supports WezTerm-style `Ctrl+R` match-type cycling
  across case-sensitive, case-insensitive, and regex search.
- Ordinary `Ctrl+F` search now supports WezTerm-style search table navigation
  with Down/Up, `Ctrl+N`/`Ctrl+P`, PageDown/PageUp, `Ctrl+R` match-type
  cycling, `Ctrl+U` clear-pattern, character ESC close, and initial query
  prefill from the current selection's first line.
- Command palette now exposes Activate Last Tab backed by app-shell last-active
  tab state.
- App-shell close-tab state handling now supports WezTerm's
  `switch_to_last_active_tab_when_closing_tab` selection policy; native UI
  close entry points still use the default left-neighbor behavior until the
  config layer can expose that option.
- Native window renders basic right/down pane splits from app-shell split state,
  including per-pane snapshot placement and split separators.
- Split panes now have pane-local mouse hit testing for click-to-focus and wheel
  scroll routing.
- Split panes support WezTerm-style directional resize actions from
  `Ctrl+Shift+Alt+Arrow` and the command palette.
- Split panes support WezTerm-style directional pane activation from
  `Ctrl+Shift+Arrow` and the command palette, with ambiguous candidates resolved
  by most recent pane activation.
- App-shell state now exposes WezTerm-style `ActivatePaneByIndex`, and the
  command palette includes Activate Pane 1..4 entries.
- App-shell state now exposes WezTerm-style `RotatePanes`; the command palette
  includes clockwise and counter-clockwise rotate entries, and pane identity
  rotation preserves split positions and size deltas.
- Split separators can now be dragged with the mouse to update split sizes via
  the same app-shell resize path used by keyboard/palette resize actions.
- Split panes support WezTerm-style toggle zoom from `Ctrl+Shift+Z` and the
  command palette, plus explicit `SetPaneZoomState` command-palette zoom/unzoom
  actions, rendering the zoomed pane across the full tab region and unzooming
  before pane-switch actions activate another pane.
- Split panes support WezTerm-style pane-select Activate mode from the command
  palette: pane labels use the WezTerm default selection alphabet, selecting a
  label focuses that pane, and `Esc`/`Ctrl+g` exits without changing focus.
- Pane-select swap modes now cover WezTerm's `SwapWithActive` and
  `SwapWithActiveKeepFocus`: selected panes exchange layout positions with the
  active pane, with focus either moving to the selected pane or staying on the
  original active pane.
- Pane-select `MoveToNewTab` now moves the selected pane into a new tab in the
  same workspace and activates that tab.
- Pane-select `MoveToNewWindow` now removes the selected pane from the current
  split layout and records a pending native-window request with its own tab and
  active pane.
- Pending `MoveToNewWindow` requests can now be consumed into detached
  app-shell/native-window app state while transferring the selected pane runtime
  snapshot.
- `rssh-app window` now runs through a multi-window manager that materializes
  detached MoveToNewWindow app states as additional native OS windows.
- PTY reader events now carry app-shell `WindowId` plus `PaneId`, avoiding
  pane-id-only routing once independent windows create their own panes.
- Command palette close actions now follow WezTerm's pane/tab/window lifecycle:
  closing the final pane in a tab closes the tab when possible, and closing the
  final tab/pane requests native-window shutdown.

The next layer is full App Shell v2 integration (multi-window focus/lifecycle
polish, pane focus visuals, pane-local scrollbars/selection polish, drag
resize affordance polish, Lua/custom tab formatting, and external CLI/mux
tab-title control) before mux/domain and protocol extensions are scaled.
