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
| Local terminal | Working console and native-window PTY paths | Native window owns only one PTY session |
| Tabs | Not present | Need tab identifiers, active tab selection, tab actions |
| Panes | Not present | Need pane tree/splits, active pane focus, per-pane runtime state |
| Workspaces | Not present | Need named workspace grouping and switching |
| Action model | Not present | Need typed actions for key bindings, command palette, tests, and CLI |
| Multiplexer | Session lifecycle only | Need domain/session/pane model before mux server/client |
| Rendering | CPU pixel renderer and damage tracking | Later GPU/font/shaping work required |
| Terminal protocols | Strong xterm/query/mouse baseline | Later image, keyboard, and Unicode parity work required |
| Config | TOML profiles | Later action bindings and possible Lua/plugin layer required |

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
  focus next/previous pane, switch workspace, and rename workspace.
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

- Closing the last pane in the last tab should be rejected by the state model
  unless the caller explicitly requests window shutdown.
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
   palette, quick select, copy mode, and configurable key bindings.
2. `Mux/Domain v1`: local, SSH, and future WSL/serial domains behind a common
   pane runtime model.
3. `Renderer v2`: GPU text renderer, font shaping/fallback, glyph atlas, and
   Unicode correctness expansion.
4. `Protocol v2`: kitty keyboard, graphics protocols, sixel/iTerm2 image paths,
   and additional compatibility responses.
5. `Config v2`: action bindings in config first, Lua/plugin layer only after
   the action and event surface is stable.
