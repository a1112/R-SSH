# WezTerm Parity App Shell v1 Design

## Context

The objective is to compare R-SSH against WezTerm and move R-SSH toward at
least equivalent product capability. The current WezTerm reference checkout is
`refs/wezterm` at `093bf6b`, with official user-facing capability references at
`https://wezterm.org/features.html`, `https://wezterm.org/ssh.html`, and
`https://github.com/wezterm/wezterm`.

R-SSH already has a terminal parser/grid, scrollback, native window, local PTY,
OpenSSH-backed SSH/SFTP/SCP, experimental native SSH, profiles, metrics, query
responses, OSC 8 hyperlinks, OSC 52 clipboard integration, xterm mouse modes,
and a downloadable Windows console package. The largest product-level gap is
not another escape sequence; it is the missing application shell that can own
tabs, panes, workspaces, commands, and future domains.

## WezTerm Capability Baseline

WezTerm's public shape is a GPU-accelerated terminal emulator and multiplexer.
The parts relevant to parity are:

- Application UI: tabs, panes, workspaces, launcher, command palette, quick
  select, copy mode, key tables, and rich key assignments.
- Multiplexing: mux server/client, local and remote domains, SSH domains, and
  CLI control of tabs, panes, and workspaces.
- Rendering: GPU rendering, mature font fallback and shaping, ligatures, glyph
  cache, and wide Unicode handling.
- Terminal protocols: broad xterm compatibility plus modern input and graphics
  protocols such as enhanced keyboard handling and terminal image paths.
- Configuration ecosystem: Lua configuration, events, plugins, and hot reload.
- Connectivity: local PTY, SSH, serial, WSL-style domains, tunnels, and remote
  shell integration.

## Current R-SSH Gap Matrix

| Area | R-SSH status | Gap to close |
| --- | --- | --- |
| Local terminal | Working console and native-window PTY paths | Native window starts with one PTY session and can materialize process-local pane/window runtimes |
| Tabs | Not present | Need tab identifiers, active tab selection, tab actions |
| Panes | Not present | Need pane tree/splits, active pane focus, per-pane runtime state |
| Workspaces | Not present | Need named workspace grouping and switching |
| Action model | Not present | Need typed actions for key bindings, command palette, tests, and CLI |
| Multiplexer | Session lifecycle only | Need domain/session/pane model before mux server/client |
| Rendering | CPU pixel renderer and damage tracking | Later GPU/font/shaping work required |
| Terminal protocols | Strong xterm/query/mouse baseline | Later image, keyboard, and Unicode parity work required |
| Config | TOML profiles | Later action bindings and possible Lua/plugin layer required |

## Task 11 Status (2026-06-09)

- Completed:
  - `rssh-core` now contains `AppShell`, `Workspace`, `Tab`, `Pane`,
    `PaneLaunch`, `AppAction`, and `AppShellError`.
  - `rssh-app` initializes app-shell state from startup command and keeps one
    default workspace/tab/pane at startup.
  - Action dispatch is available for new tab/close tab/close pane/focus navigation,
    split pane, and workspace creation/switch/rename operations.
  - Keyboard shortcuts are routed to app-shell actions (`Ctrl+Shift+T/W/[/{]/]/D/E`).
  - `Ctrl+Shift+1..9` tab number activation is routed through app-shell
    `ActivateTabIndex` (`1`..`8` map to `0`..`7`, `9` maps to `-1`).
  - App-shell state tracks the last active tab, with `ActivateLastTab` no-oping
    when no previous active tab exists and the command palette exposing
    Activate Last Tab.
  - App-shell state exposes WezTerm-style indexed tab activation through
    `ActivateTabIndex`, including negative indices for right-to-left selection.
  - App-shell state exposes WezTerm-style `ActivateTabRelative` wrapping and
    `ActivateTabRelativeNoWrap` clamping, and the command palette includes both
    wrapping and no-wrap Next/Previous Tab entries.
  - `MoveTabRelative` reorders the active tab within the current workspace while
    preserving that tab as active.
  - `MoveTab` reorders the active tab to an absolute zero-based index with typed
    out-of-range errors, and the command palette exposes Move Tab To 1..4.
  - CloseTab state handling can select the default left-neighbor tab or the
    previous active tab, matching WezTerm's
    `switch_to_last_active_tab_when_closing_tab` behavior surface.
  - The native window now reserves and renders a one-row tab bar with
    workspace/tab/pane-count state, explicit tab title priority, active-pane
    terminal title fallback, click-to-activate tab behavior, and clickable tab
    close markers plus a new-tab button.
  - Basic right/down split layouts now render pane snapshots into split regions
    with separator cells.
  - Split pane mouse hit testing now supports click-to-focus and pane-local
    wheel scrolling.
  - Split pane resize actions are routed from `Ctrl+Shift+Alt+Arrow` and the
    command palette into app-shell split size deltas.
  - App-shell state exposes WezTerm-style `ActivatePaneByIndex`, and the command
    palette includes Activate Pane 1..4 entries.
  - App-shell state exposes WezTerm-style `RotatePanes`, and the command palette
    includes clockwise/counter-clockwise pane rotation while preserving split
    positions and size deltas.
  - Rendered split separators can be drag-resized with the mouse, reusing the
    existing app-shell resize actions and split size deltas.
  - Toggle pane zoom is routed from `Ctrl+Shift+Z` and the command palette into
    app-shell zoomed-pane state, and WezTerm-style `SetPaneZoomState` is exposed
    for explicit command-palette zoom/unzoom. Zoom rendering fills the tab with
    the active pane and unzooms before pane-switch actions activate another pane.
  - Pane select Activate mode is available from the command palette, rendering
    WezTerm-style selection labels over pane regions and activating the labelled
    pane on key input.
  - Pane select swap modes cover `SwapWithActive` and
    `SwapWithActiveKeepFocus`, exchanging pane layout positions while applying
    the corresponding focus rule.
  - Pane select `MoveToNewTab` moves the selected pane into a newly created tab
    in the same workspace and activates that tab.
  - Pane select `MoveToNewWindow` removes the selected pane from the current
    split layout and records a pending native-window request with its own tab
    and active pane.
  - Pending `MoveToNewWindow` requests can be consumed into detached app-shell
    and native-window app state while preserving the selected pane runtime
    snapshot.
  - `rssh-app window` now runs through a multi-window event-loop manager that
    materializes detached MoveToNewWindow app states as additional native OS
    windows.
  - PTY reader events carry app-shell `WindowId` plus `PaneId`, so event routing
    is scoped to the owning native-window app instead of relying on globally
    unique pane IDs.
  - ClosePane/CloseTab lifecycle handling now matches WezTerm's cascade model:
    a final pane can close its tab, and the final tab/pane requests native-window
    shutdown from the window manager.
  - `SetTabTitle` stores explicit tab titles in app-shell state, and the native
    tab bar prefers those titles before falling back to active-pane terminal
    titles.
  - The command palette includes `Rename Tab` with `rename tab <title>` query
    input, writing explicit titles for the active tab.
  - The command palette includes WezTerm-style `ClearSelection`, clearing
    active-window selection state and rendered selection highlights.
  - The command palette includes WezTerm-style
    `ClearScrollback('ScrollbackOnly')`, clearing active-pane history on the
    output side while preserving the viewport.
  - The command palette includes WezTerm-style
    `ClearScrollback('ScrollbackAndViewport')`, clearing active-pane history
    plus the viewport while preserving the prompt/cursor row as the new first
    visible line.
  - The command palette includes WezTerm-style `CopyTo('Clipboard')`, copying
    the active selection into the system clipboard writer.
  - The command palette includes WezTerm-style `CopyTo('PrimarySelection')` and
    `CopyTo('ClipboardAndPrimarySelection')` routing. Native OS
    PrimarySelection storage remains a platform-adapter follow-up.
  - The command palette includes WezTerm-style `PasteFrom('Clipboard')`, pasting
    the configured clipboard reader into the active pane.
  - The command palette includes WezTerm-style `PasteFrom('PrimarySelection')`
    routing, and `Ctrl+Insert`/`Shift+Insert` shortcut classification matches
    WezTerm's PrimarySelection defaults.
  - The command palette includes WezTerm-style `ResetTerminal`, injecting RIS
    (`ESC c`) into the active pane output side.
  - The command palette includes WezTerm-style scrollback navigation for
    top/bottom, page up/down, line up/down, and OSC 133 previous/next prompt
    movement.
  - The terminal core records OSC 133 Prompt/Input/Output semantic zones across
    retained rows, including line-scoped `I` input markers.
  - The terminal core can extract text from semantic zones and retained
    row/column regions.
  - Copy mode can move between semantic zones across retained scrollback with
    WezTerm-style `z`/`Shift+Z` bindings and typed Prompt/Input/Output filters.
  - Copy mode can copy source-row selections that span the live viewport and
    retained scrollback.
  - Copy mode `y` follows WezTerm's default CopyTo
    ClipboardAndPrimarySelection, then ScrollToBottom and Close behavior.
  - Copy mode supports WezTerm-style Cell selection with Space/`v`, Line
    selection with uppercase no-modifier or shifted `V`, and rectangular block
    selection with `Ctrl+V`.
  - Copy mode vertical/page movement can traverse retained scrollback with
    source-row cursor coordinates.
  - Copy mode supports WezTerm-style `MoveToStartOfNextLine` through Enter and
    character CR (`\r`) events.
  - Copy mode can move to scrollback top/bottom with WezTerm-style
    `g`/`Shift+G` bindings.
  - Copy mode can move to viewport top/middle/bottom with WezTerm-style
    `H`/`M`/`L` bindings, including uppercase no-modifier key-table events.
  - Copy mode can move to first/last non-space cell with WezTerm-style
    content-aware `^`/`Alt+m` and `$`/End line start/end bindings.
  - Copy mode supports WezTerm-style word movement (`w`/`b`/`e`,
    Tab/Shift+Tab, Alt+Left/Right, Alt+F/B) across retained source rows.
  - Copy mode supports WezTerm-style jump-to-char movement (`f`/`t`/`F`/`T`,
    `;`, `,`) on the current source row.
  - Copy mode supports WezTerm-style selection-end movement (`o`/`O`).
  - Ordinary copy-mode close follows WezTerm's `ScrollToBottom` then `Close`
    default behavior before exiting the overlay.
  - Copy mode and copy-mode search close on both Escape key events and
    character ESC (`\u{1b}`) events, clearing copy-mode search status from the
    window title.
  - Copy mode and copy-mode search allow global command-palette and app-shell
    shortcuts such as `Ctrl+Shift+P` and `Ctrl+Shift+T` to fall through from
    the overlay, matching WezTerm key-table fallback behavior.
  - Copy-mode search keeps copy mode active while entering `/`/`?` queries and
    supports next/prior match navigation, including character CR as PriorMatch.
  - Copy-mode search supports page-wise match navigation with PageDown/PageUp.
  - Copy-mode search supports `Ctrl+R` match-type cycling across
    case-sensitive, case-insensitive, and regex search.
  - Ordinary `Ctrl+F` search supports WezTerm-style search table navigation
    with Down/Up, `Ctrl+N`/`Ctrl+P`, PageDown/PageUp, `Ctrl+R` match-type
    cycling, `Ctrl+U` clear-pattern, character ESC close, and initial query
    prefill from the current selection's first line.
  - Retained row/column and semantic-zone text extraction unwraps soft-wrapped
    physical rows into logical-line text.
  - Native window title now includes shell-state suffix.
- Open gaps after v1:
  - multi-window focus/lifecycle polish, pane-local scrollbar UI, richer
    Lua/custom tab formatting, external CLI/mux tab-title control, new-tab
    launcher behavior, split-drag affordances, and richer pane focus visuals
  - true multiplexing server/client and domain attachments
  - GPU text shaping/fallback and remaining protocol extensions (broader kitty
    alternate-key variants, graphics/sixel)
  - full command palette UX (discovery, fuzzy filtering, richer actions)
  - configurable action bindings and Lua/plugin extension layer
  - WezTerm-style Lua pane semantic-zone APIs and configurable key-table bindings
- Full gap matrix: `docs/research/wezterm-parity-gap.md`

## Chosen Approach

Use an R-SSH-native staged implementation. The first stage is `App Shell v1`,
which creates the internal model that WezTerm-like tabs, panes, workspaces, and
commands can sit on. It deliberately does not implement a daemon multiplexer,
GPU renderer, Lua, or image protocols in this stage.

This keeps the current console and native window startup behavior stable while
making the application shell a first-class layer instead of adding tab/pane
state directly into `window.rs`.

## App Shell v1 Scope

App Shell v1 adds:

- Stable IDs for windows, workspaces, tabs, panes, and commands/actions.
- A workspace model containing tabs.
- A tab model containing a pane tree.
- A pane model with local PTY launch intent and terminal size.
- Typed actions for new tab, close tab, switch tab, split pane, close pane,
  indexed tab activation, wrapping and no-wrap relative tab activation, absolute
  and relative tab movement, indexed pane activation, pane rotation, directional
  pane activation, focus next/previous pane aliases, switch workspace, and
  rename workspace.
- A command dispatch boundary that validates whether an action can be applied
  before mutating state.
- Native-window integration that starts with one workspace, one tab, and one
  pane, then preserves the existing default `rssh-app` / `rssh-app window`
  behavior.

The first implementation should support local PTY panes only. SSH domains and
remote panes depend on the later mux/domain stage.

## Non-Goals

- No mux server/client daemon in App Shell v1.
- No remote SSH domain panes in App Shell v1.
- No GPU renderer replacement.
- No Lua plugin system.
- No image protocol implementation.
- No full visual tab bar polish beyond what is needed to make state and actions
  observable and testable.

## Architecture

Add a pure state model under `rssh-core`, because tabs, panes, workspaces, and
actions are product-domain concepts rather than window-event details. The app
crate can then use this model from the native window, profiles, CLI commands,
and future mux code.

The model should be deterministic and unit-testable without spawning PTYs or
creating windows.

```text
rssh-core
  AppShell
    Workspace[]
      Tab[]
        PaneTree
          Pane
            PaneLaunch(LocalCommand)

rssh-app
  window.rs
    NativeWindowApp
      AppShell state
      active PaneRuntime
      existing TerminalRuntime/PTySession per active pane
```

In the first integration step, `NativeWindowApp` may still render only the
active pane. That still moves the product toward WezTerm parity because the
state model and action dispatch become ready for multi-pane rendering and mux
control.

## Data Flow

Startup:

1. CLI parses `window` as it does today.
2. `window::run` creates an `AppShell` with a default workspace, tab, and local
   pane from the current startup command.
3. `NativeWindowApp` spawns the active pane's PTY and renders its terminal
   runtime.

Action dispatch:

1. A keyboard shortcut, future command palette entry, or test sends an
   `AppAction`.
2. `AppShell::apply_action` validates and mutates the workspace/tab/pane model.
3. Window integration reacts to created/closed/focused pane changes by spawning
   or stopping pane runtime resources.

Rendering:

1. App Shell v1 renders the active pane terminal content as today.
2. The window title includes enough shell state to verify active workspace, tab,
   and pane in smoke runs.
3. Later stages can add tab bars, split layouts, and pane borders without
   changing the core state model.

## Error Handling

- Closing the last pane in a tab cascades to closing that tab when another tab
  exists; closing the last tab/pane returns a typed guard that the native-window
  layer converts into a window shutdown request.
- Closing a tab removes its pane tree and selects a neighboring tab.
- Invalid IDs return typed errors and do not mutate state.
- Split actions inherit the active pane's launch intent unless a command is
  supplied.
- Runtime spawn failures remain visible through the existing terminal/window
  error path; the state model should not hide failed panes.

## Testing Strategy

- Unit-test the pure `rssh-core` app-shell model first.
- Add parser tests for any new CLI flags only after the state model exists.
- Add native-window unit tests that prove startup creates one workspace, one
  tab, and one active pane without changing the existing command behavior.
- Add action-dispatch tests before wiring shortcuts.
- Keep existing workspace-wide format, test, clippy, and release build gates.

## Parity Roadmap After App Shell v1

1. `App Shell v2`: visual tab bar, split rendering, pane focus UI, command
   palette, quick select, and configurable key bindings.
2. `Mux/Domain v1`: local, SSH, and future WSL/serial domains behind a common
   pane runtime model.
3. `Renderer v2`: GPU text renderer, font shaping/fallback, glyph atlas, and
   Unicode correctness expansion.
4. `Protocol v2`: remaining kitty keyboard variants, graphics protocols,
   sixel/iTerm2 image paths, and additional compatibility responses.
5. `Config v2`: action bindings in config first, Lua/plugin layer only after
   the action and event surface is stable.
