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
- `rssh-app` applies the WezTerm-style `term` effective-config value when
  constructing native PTY commands, defaulting `TERM` to `xterm-256color` and
  allowing overrides such as `term='wezterm'` for newly spawned panes/windows.
  Native `default_prog` and `set_environment_variables` overrides are also
  applied to local-domain PTY launches, and native `default_cwd`, then the user
  home directory, are used as fallback cwd values when a pane launch has no
  explicit cwd or process-tree cwd. Native pane metadata prefers OSC
  7/current-dir cwd and falls back to the local session process tree cwd,
  preferring child processes, when the PTY backend exposes a pid. Native
  new-tab/split/spawn-window actions without an explicit launch program use
  `default_prog` while preserving inherited cwd, omitted-spawn
  `SwitchToWorkspace`/new-workspace creation also uses `default_prog`, and
  no-program `SpawnWindow` requests inherit the active pane launch/cwd when no
  `default_prog` override is active; a pre-spawn native config override also
  applies `default_prog` to the initial native window pane when startup would
  otherwise use the platform default shell. Native `default_workspace` names
  the initial default workspace before spawn when no explicit startup workspace
  is present. Native `default_domain` is retained in the effective config, and
  `SpawnTab(DefaultDomain)` uses the local spawn path only while the configured
  default domain is `local`. Native `prefer_to_spawn_tabs` is retained with
  WezTerm's default `false` and, when enabled, routes unpositioned
  same-process `SpawnWindow` requests into a new tab while preserving
  positioned spawn-window requests as detached windows.
  Local/window/start/console CLI startup accepts
  WezTerm-style `--cwd` for the initial child process, and native window startup
  accepts WezTerm-style `start` as an alias for `window`, bare
  `<program> [args...]`, and `-e` as initial program forms. It also accepts
  WezTerm-style `--workspace` to name the initial workspace,
  `--class CLASS` to request the native window class name on Windows, and
  `--position X,Y`/`screen:X,Y`/`main:X,Y`/`active:X,Y`/`<monitor>:X,Y` to
  request an initial native window screen position. `main:` is relative to the
  primary monitor origin, `active:` is relative to the active monitor when the
  platform exposes one and otherwise falls back to the primary monitor origin,
  and named monitor forms are relative to the matching monitor origin. Native
  window startup also accepts WezTerm startup compatibility flags
  `--no-auto-connect`, `--always-new-process`, and `--new-tab` as current
  no-ops because there is no GUI daemon or auto-connected mux domain yet;
  `--domain local` selects the current local PTY domain and `--attach` is
  accepted as a no-op until mux attachment exists. X11/Wayland class/app-id
  application and remote/named mux domains remain later parity work. Static
  WezTerm-style Lua `default_prog`/`default_cwd` parsing now feeds the native
  launch override path, and `default_prog` is applied to the initial default
  shell before spawn while preserving the startup cwd. When a top-level static
  `return { ... }` config table or `return cfg` config variable is present, it
  is treated as the returned config and earlier assignments to unreturned config
  variables are ignored; supported repeated direct field assignments and
  whole-table assignments on that returned config use the latest static value
  by source order, and duplicate fields inside static config table constructors
  use the later entry.
- `rssh-app` app-shell action dispatch maps from typed actions to updated
  app state.
- `rssh-app` keyboard handling recognizes app-shell shortcuts before PTY input:
  - `Ctrl+Shift+N` spawn window
  - `Super+N` spawn window
  - `Alt+Enter` toggle full screen
  - `Ctrl+Shift+T` new tab via `SpawnTab(CurrentPaneDomain)`
  - `Super+T` new tab via `SpawnTab(CurrentPaneDomain)`
  - `Super+Shift+T` new tab via `SpawnTab(DefaultDomain)` local-domain validation
  - `Ctrl+Shift+W` close tab via `CloseCurrentTab(confirm=true)` confirmation
  - `Super+W` close tab via `CloseCurrentTab(confirm=true)` confirmation
  - `Super+Shift+]` next tab
  - `Super+Shift+[` previous tab
  - `Ctrl+Tab` next tab via `ActivateTabRelative(1)`
  - `Ctrl+Shift+Tab` previous tab
  - `Ctrl+PageUp` previous tab
  - `Ctrl+PageDown` next tab via `ActivateTabRelative(1)`
  - `Ctrl+Shift+PageUp` move tab left
  - `Ctrl+Shift+PageDown` move tab right
  - `Ctrl+Shift+1..9` activate tab via `ActivateTab(0..7/-1)`
  - `Super+1..9` activate tab via `ActivateTab(0..7/-1)`
  - `Ctrl+Shift+Alt+\"` `SplitVertical={domain="CurrentPaneDomain"}` / split pane down
  - `Ctrl+Shift+Alt+%` `SplitHorizontal={domain="CurrentPaneDomain"}` / split pane right
  - `Ctrl+Shift+Alt+Arrow` resize active pane in the arrow direction
  - `Ctrl+Shift+Arrow` activate the neighboring pane in that direction
  - `Ctrl+Shift+Z` toggle active pane zoom via `TogglePaneZoomState`
  - `Ctrl+Shift+R` reload configuration
  - `Super+R` reload configuration
  - `Super+K` / `Ctrl+Shift+K` clear scrollback
  - `Super+F` / `Ctrl+Shift+F` search via `Search(CaseSensitiveString="")`
  - `Ctrl+Shift+X` copy mode via `ActivateCopyMode`
  - `Ctrl+Shift+Space` quick select via `QuickSelect(default args)`
  - `Super+C` / `Ctrl+Shift+C` / `Copy` copy to clipboard
  - `Super+V` / `Ctrl+Shift+V` / `Paste` paste from clipboard
  - `Super+M` hide/minimize the native window
  - `Super+H` hide the application on macOS, falling back to native-window
    minimization
  - `Ctrl`/`Super` with `-`, `=`, and `0` decrease, increase, and reset the
    logical native-window font-size scale
  - `Ctrl+Shift+L` show the native debug overlay state
  - `Ctrl+Shift+U` enter native character selection mode
- `rssh-core` tracks the last active tab and exposes an `ActivateLastTab`
  action matching WezTerm's no-op behavior when no previous active tab exists.
- `rssh-core` exposes `ActivateTabIndex` for WezTerm-style `ActivateTab`
  semantics: zero-based positive indices select from the left, and negative
  indices select from the right. `rssh-app` routes `Ctrl+Shift+1..9` and
  `Super+1..9` through this action, with `9` mapped to `-1`, and exposes
  command-palette Activate Tab 1..9 entries plus `activate tab <index>` and
  `activate tab index <index>` plus `activatetab <index>` and WezTerm-style
  `wezterm.action.ActivateTab(<index>)` function-call queries for direct
  selection. Action-name `activatelasttab` and `activatetab1` through
  `activatetab9` queries dispatch the corresponding fixed entries. Native
  `WindowCommand::ActivateTab(index)` payloads cover arbitrary positive or
  negative indices. The default `Ctrl+Shift+1..9`
  and `Super+1..9` key-assignment entries expose `ActivateTab(0..7/-1)`
  payloads while retaining numbered `Activate Tab 1..9` launcher labels.
- `rssh-app` exposes WezTerm-style `ShowTabNavigator` through command-palette
  `Show Tab Navigator`, opening a native tab-list overlay with the active tab
  initially selected and Enter activating the selected tab. Action-name
  `showtabnavigator` queries dispatch the same command.
- `rssh-app` routes WezTerm's default `Super+Shift+T`
  `SpawnTab="DefaultDomain"` binding through the native `SpawnTab(DefaultDomain)`
  path, preserving configured `default_domain` validation before it creates a
  native tab. Native `SpawnTab` action payloads cover the local-domain subset:
  `CurrentPaneDomain`, `DefaultDomain`, and `DomainName("local")` all create a
  new tab through the same native launch path when they resolve to local.
  Structured command-palette `spawn tab current pane domain`,
  `spawn tab default domain`, and `spawn tab domain <name>` queries dispatch the
  same native payload subset; action-name `spawntab ...` aliases dispatch that
  payload with quote-aware domain-name parsing, and no-argument `spawntab`
  dispatches the current-pane-domain default. Remote/mux named domain spawning
  remains a later mux/domain parity item.
- `rssh-app` exposes native command-palette query subsets for WezTerm-style
  `SpawnCommandInNewTab` and `SpawnCommandInNewWindow`: `new tab <program>
  [args...]` creates and activates a tab with that launch command, while
  `spawn window <program> [args...]` records a pending native window with that
  launch command. The WezTerm action-name aliases `spawncommandinnewtab
  <program> [args...]` and `spawncommandinnewwindow <program> [args...]`
  route through the same parser. These query forms also accept an optional leading
  `--domain local`, `--domain CurrentPaneDomain`/`--domain current-pane-domain`,
  or `--domain DefaultDomain`/`--domain default-domain` before the program and
  reject unsupported named domains; they also accept
  leading `--cwd <path>` / `--cwd=<path>`, `--env NAME=VALUE` /
  `--env=NAME=VALUE`, and
  `--set-environment-variables NAME=VALUE` /
  `--set_environment_variables=NAME=VALUE` fields for the same native
  `SpawnCommand` subset. Single
  or double quotes can group option values and args that contain spaces.
  `spawn window` queries additionally accept leading `--position <position>` /
  `--position=<position>` using the same `X,Y`, `screen:X,Y`, `main:X,Y`,
  `active:X,Y`, and `<monitor>:X,Y` forms as startup positioning, and spawned
  native windows inherit the startup `--class` value from the current app
  process. `new tab` and `spawn window` queries can also omit the program when
  they provide supported SpawnCommand options: `--domain local`,
  `--domain CurrentPaneDomain`/`--domain current-pane-domain`,
  `--domain DefaultDomain`/`--domain default-domain`, `--cwd`, `--env`,
  `--set-environment-variables`/`--set_environment_variables`, and for
  `spawn window`, `--position`, are applied to the existing
  default-prog/inherited launch path.
  Native
  `SpawnCommandInNewTab` and
  `SpawnCommandInNewWindow` action payloads also carry a `SpawnCommand`
  `args`/`cwd`/`set_environment_variables` subset through the same tab/window
  launch paths, accept the local-domain subset `CurrentPaneDomain`,
  `DefaultDomain`, and `DomainName("local")`, and `SpawnCommandInNewWindow`
  carries the WezTerm-style `position` payload into the detached native
  window's initial position, including Lua table `{ x = ..., y = ...,
  origin = ... }` values with bracketed string table keys and nested named
  origins.
  No-argument action-name `spawnwindow` dispatches the same default
  `SpawnWindow` path as the command-palette entry.
  Split command-palette queries also accept explicit launch commands: `split
  horizontal <program> [args...]` / `split right <program> [args...]` and
  `split vertical <program> [args...]` / `split down <program> [args...]`,
  plus `split left <program> [args...]` and `split up <program> [args...]`,
  create splits with that launch command. The WezTerm action-name forms
  `splitpane <right|down|left|up> ...` and
  `splitpane direction <right|down|left|up> ...` build the same native payload
  with the same quoted-value support for launch command option values and args.
  These query forms also accept
  `--percent N`/`--percent=N`, `--cells N`/`--cells=N`,
  `--top-level`/`--top-level=true|false`, and supported
  `--domain`/`--cwd`/`--env` options in any order before the optional launch
  command. When the launch command is omitted, supported spawn fields are
  applied to the existing default-prog/inherited launch path.
  Native `SplitPane`
  action payloads carry Left/Right/Up/Down `direction` values, the local-domain
  subset `CurrentPaneDomain`, `DefaultDomain`, and `DomainName("local")`, plus
  optional `SpawnCommand` `args`/`cwd`/`set_environment_variables` subset
  through the same split launch path, and support `size = { Percent = ... }` /
  `size = { Cells = ... }` for the new pane's initial size. The default
  `Ctrl+Shift+Alt+\"` and `Ctrl+Shift+Alt+%` key-assignment entries expose the
  WezTerm-style `SplitVertical={domain="CurrentPaneDomain"}` and
  `SplitHorizontal={domain="CurrentPaneDomain"}` payloads while the command
  palette keeps `SplitVertical`/`SplitHorizontal` aliases; action-name
  `splitvertical` and `splithorizontal` queries dispatch those default split
  directions. Native `SplitPane` payloads also support `top_level = true` by
  splitting the full active-tab root region and compressing the existing layout
  into the source side. Full Lua
  `SpawnCommand`/`SplitPane` table parsing, remote/mux domains, and broader
  table forms remain later config parity work.
- `rssh-core` exposes `ActivateTabRelative` for wrapping relative tab activation
  and `ActivateTabRelativeNoWrap` for first/last clamping. `rssh-app` routes the
  default tab navigation shortcuts and command-palette Next/Previous Tab entries
  plus `activate tab relative <offset>` and
  `activate tab relative no wrap <offset>` plus action-name
  `activatetabrelative <offset>` and `activatetabrelativenowrap <offset>`
  plus WezTerm-style `wezterm.action.ActivateTabRelative(<offset>)` and
  `wezterm.action.ActivateTabRelativeNoWrap(<offset>)` function-call queries
  through those app-shell actions; action-name `nexttab`,
  `previoustab`, `nexttabnowrap`, and `previoustabnowrap` queries dispatch the
  corresponding fixed entries. Native
  `WindowCommand::ActivateTabRelative(offset)` plus
  `WindowCommand::ActivateTabRelativeNoWrap(offset)` payloads cover arbitrary
  relative offsets. The default `Ctrl+Tab`, `Ctrl+Shift+Tab`,
  `Ctrl+PageUp`, `Ctrl+PageDown`, and `Super+Shift+[/]` key-assignment entries
  expose `ActivateTabRelative` payloads while the command palette keeps
  Next/Previous Tab aliases.
- `rssh-core` applies `MoveTabRelative` by reordering the active tab within the
  current workspace while preserving that tab as active. `rssh-app` exposes
  command-palette Move Tab Relative Left/Right entries for one-step movement
  plus `move tab relative <offset>` / `movetabrelative <offset>` and
  WezTerm-style `wezterm.action.MoveTabRelative(<offset>)` function-call
  queries, and a native `WindowCommand::MoveTabRelative(offset)` payload for
  arbitrary relative offsets; action-name `movetabrelativeleft` and
  `movetabrelativeright` queries dispatch the fixed one-step entries.
- `rssh-core` exposes WezTerm-style `MoveTab` for moving the active tab to a
  zero-based absolute index, preserving the active tab and rejecting out-of-range
  indices. `rssh-app` exposes command-palette Move Tab To 1..8 entries and a
  `move tab <index>`/`move tab to <index>`/`movetab <index>` query set plus
  WezTerm-style `wezterm.action.MoveTab(<index>)` function-call queries,
  action-name `movetabto1` through `movetabto8`, plus a native
  `WindowCommand::MoveTab(index)` payload for arbitrary zero-based indices.
- `rssh-core` exposes WezTerm-style `Nop` as a true no-effect action; it does
  not mutate active IDs, workspace/tab/pane collections, or active-pane
  unseen-output state. `rssh-app` also exposes native `WindowCommand::Nop`,
  allowing native key/palette action payloads to consume a trigger without
  changing window state, and structured command-palette `nop` queries dispatch
  that payload directly.
- `rssh-app` dispatches matching native user `key_assignments` before built-in
  default shortcuts, executing the configured native `WindowCommand` subset so
  user bindings can override defaults. Native key strings accept WezTerm-style
  `|` modifier grouping, such as `CTRL|ALT+D` and `LEADER|SHIFT+|`, in
  addition to the existing `+`-separated shorthand, and honor the documented
  `SUPER`/`CMD`/`WIN` plus `ALT`/`OPT`/`META` modifier aliases. The native key
  matcher also recognizes WezTerm-style `F1` through `F24` function-key
  identifiers, physical `Numpad0` through `Numpad9` and numpad operator
  identifiers, browser navigation identifiers, and native named identifiers for
  lock keys, `PrintScreen`, `Pause`, `Menu`/`ContextMenu`, media transport keys,
  and audio volume keys including WezTerm's documented
  `MediaNextTrack`/`MediaPrevTrack` and `Volume*` aliases. Explicit
  WezTerm-style `phys:` and `mapped:` key prefixes are supported for native key
  assignments so physical letter/digit positions can be distinguished from
  mapped layout output, and `raw:<decimal>` matches platform-native
  unidentified physical keycodes. Native `key_map_preference` supports the
  default `Mapped` behavior and `Physical` matching for unprefixed
  letter/digit user assignments and default app-shell/window shortcuts. Native
  `enable_csi_u_key_encoding` defaults to false and, when enabled, routes
  modified ASCII keys through CSI-u encoding while preserving the default legacy
  key encoding path. Native `enable_kitty_keyboard` defaults to false and, when
  enabled, honors kitty keyboard protocol negotiation sequences and flags
  queries. Native `allow_win32_input_mode` defaults to true, tracks ConPTY
  `CSI ? 9001 h/l` mode requests, and makes native-window and local console
  input emit Win32 key records for that mode before CSI-u/kitty encoding; Lua
  config parsing remains future parity work.
- `rssh-app` exposes a native WezTerm-style `leader` override subset for user
  key assignments. Pressing the configured leader key arms the virtual
  `LEADER` modifier until the next key press or `timeout_milliseconds`; while
  active, only `LEADER` assignments are matched and unmatched keys are
  swallowed before normal input resumes. Static WezTerm-style `config.leader`
  snippets parse `key`, `mods`, and optional `timeout_milliseconds`, including
  bracketed string table keys with long-bracket values. Broader Lua config
  evaluation remains future config parity work.
- `rssh-app` exposes a native WezTerm-style `DisableDefaultAssignment` action
  for user key assignments. Matching key strings suppress built-in app-shell,
  window-level, and scrollback shortcuts so the key continues through the later
  input path instead of being consumed by the default binding. Structured
  command-palette `disabledefaultassignment` queries parse to the same native
  action payload for command/payload coverage.
- `rssh-app` exposes native WezTerm-style `SendString` action payloads, writing
  the provided string bytes directly to the active PTY input path as typed
  input. The structured command-palette query `send string <text>` plus
  action-name `sendstring <text>` dispatches the same typed payload path. This
  path does not wrap data as bracketed paste. WezTerm-style
  `wezterm.action.SendString { string = ... }` table-call queries dispatch the
  same typed payload path.
- `rssh-app` exposes native WezTerm-style `SendKey` action payloads, encoding
  the specified key and modifiers through the active terminal input mode and
  writing the resulting bytes directly to the active PTY input path without
  re-matching key assignments. The structured command-palette query
  `send key <mods+key>` plus action-name `sendkey <mods+key>` covers
  single-character key payloads such as `send key ALT+B` plus WezTerm-style
  logical named keys and F1-F35 identifiers such as `send key ALT+LeftArrow`
  and `send key CTRL+SHIFT+F5`.
- `rssh-core` exposes WezTerm-style `Multiple` sequencing for already
  implemented `AppAction` values, applying each nested action in order.
- `rssh-core` exposes a named WezTerm-style `SwitchToWorkspace` subset:
  existing named workspaces become active without duplication, while missing
  named workspaces are created with the requested spawn command and selected.
  Native `SwitchToWorkspaceArgs` payloads expose that spawn command path through
  `rssh-app`, while existing named workspaces keep their current pane launch.
  Missing workspaces created without an explicit spawn command use native
  `default_prog` for the new pane when configured. Omitted-name actions create
  randomly named workspaces. Native
  `SwitchWorkspaceRelative` payloads switch by arbitrary signed offsets using
  the same sorted workspace order as Next/Previous Workspace, and the structured
  command palette queries `switch workspace relative <offset>` and
  action-name `switchworkspacerelative <offset>` dispatch the same native
  payload. `rssh-app` exposes command-palette `Switch To Workspace` plus
  `switch workspace <name>` and action-name `switchtoworkspace <name>` queries
  for that path; `switch workspace <name> spawn [--domain ...] [--cwd ...]
  [--env NAME=VALUE] [--set-environment-variables NAME=VALUE]
  [<program> [args...]]`
  carries the same native `SpawnCommand` query subset into newly created
  workspaces. Quoted workspace names can contain the word `spawn` without being
  treated as the spawn-command delimiter. When the program is omitted, supported
  spawn options are applied to the default-prog/inherited launch path. `switch
  workspace spawn [--domain ...] [--cwd ...] [--env NAME=VALUE]
  [--set-environment-variables NAME=VALUE] [<program> [args...]]` creates a
  randomly named workspace with the requested launch
  command or commandless spawn options. WezTerm-style
  `wezterm.action.SwitchToWorkspace { name = ..., spawn = { ... } }` table
  queries dispatch the same implemented name and spawn subset, including
  bracketed string keys for nested commandless spawn options and environment
  entries. Native
  `ShowLauncher` opens the default Launcher Menu for
  local-domain spawning plus native launch-menu items, and action-name
  `showlauncher` queries dispatch that default launcher command. Native
  `ShowLauncherArgs` accepts WezTerm-style pipe-separated flags through
  `show launcher <FLAGS>` and action-name `showlauncherargs <FLAGS>` /
  `showlauncher <FLAGS>` / `showlauncherargs flags=<FLAGS>` queries,
  accepting case-insensitive flag aliases and
  `_`/`-`/compact spellings for multi-word flags.
  `FUZZY` and,
  with `COMMANDS`, `DOMAINS`, `KEY_ASSIGNMENTS`, `LAUNCH_MENU_ITEMS`, `TABS`,
  and/or `WORKSPACES`, opens a launcher-scoped palette containing built-in
  commands, the local domain spawn entry, native default plus override key
  assignment entries, native launch-menu items, active-workspace tabs, and
  existing workspaces; selecting an entry executes that command or activates that
  tab/workspace. A `FUZZY`-only launcher intentionally opens with no entries,
  matching WezTerm's flag semantics. Native `ShowLauncherArgs` also carries an
  `alphabet` subset via `show launcher <FLAGS> alphabet <chars>`,
  `showlauncherargs <FLAGS> alphabet <chars>`, and
  `showlauncher <FLAGS> alphabet <chars>` queries; in
  non-`FUZZY` launcher mode, pressing a configured one- or two-key shortcut
  executes the matching visible entry, falling back to the native
  `launcher_alphabet` effective-config value when the action omits `alphabet`,
  `j`/`k` move the selected launcher entry, and `/` enters fuzzy filtering
  mode. Native `ShowLauncherArgs` also accepts `help_text` for default launcher
  mode and `fuzzy_help_text` for fuzzy filtering mode status prompts, including
  quote-aware `show launcher <FLAGS> help_text <text> fuzzy_help_text <text>`
  / `showlauncherargs <FLAGS> help_text <text> fuzzy_help_text <text>`
  / `showlauncher <FLAGS> help_text <text> fuzzy_help_text <text>`
  query fields plus `help text`/`fuzzy help text` and hyphenated field-key
  aliases for alphabet/title/help strings, supports both `field <text>` and
  `field=<text>` forms, keeps field-key words inside help/title text when a
  later valid field boundary exists, and falls back to WezTerm's documented
  single-space default prompt strings when omitted.
  Structured `show launcher <FLAGS>` queries reject unknown top-level fields
  instead of silently discarding them.
  Static WezTerm-style `config.launch_menu` snippets feed native launch-menu
  entries for the implemented `SpawnCommand` subset, including bracketed string
  table keys with long-bracket values for launch-menu item fields and
  environment entries, and top-level static
  `table.insert(config.launch_menu, { ... })` append entries plus
  `table.insert(config.launch_menu, index, { ... })` numeric-position inserts,
  with bracket field selectors such as `config['launch_menu']` and static
  table variables such as `table.insert(config.launch_menu, item)` or
  `table.insert(config.launch_menu, index, item)` supported.
  Static
  WezTerm-style `config.keys` actions can also carry `ShowLauncherArgs` table
  payloads through the implemented native action subset, and static key
  `wezterm.action_callback` bodies that call
  `window:perform_action(<implemented action>, pane)` map onto existing native
  commands. Remote/mux domains, richer default-mode UI styling, broader Lua key
  assignment/config parsing, broader dynamic Lua `launch_menu` construction,
  arbitrary Lua callback execution, and Lua event/config wiring remain later
  parity work.
- `rssh-core` supports WezTerm's close-tab selection policy: callers can keep
  the default left-neighbor activation or request last-active-tab activation
  when closing the active tab.
- `rssh-app` applies `switch_to_last_active_tab_when_closing_tab` to default
  close-tab shortcuts, tab-bar close clicks, and Close Current Tab command
  paths, including accepted `confirm = true` overlays and
  confirmation-skipped closes.
- `rssh-app` honors WezTerm's `quit_when_all_windows_are_closed=true` default
  in the multi-window manager and keeps the event loop running after the last
  window closes when the native override is false.
- `rssh-app` includes a minimal command palette (`Ctrl+Shift+P`) for quick
  execution of tab/pane/window/workspace actions, including Spawn Window,
  Toggle Full Screen, Activate Last Tab, WezTerm-style Close Current Tab/Pane,
  WezTerm-style Split Horizontal/Vertical, explicit split launch queries, and
  `ActivatePaneDirection` Left/Right/Up/Down/Next/Previous entries. The
  command-palette queries `activate pane <index>`,
  `activate pane by index <index>`, and
  `activate pane direction <direction>` plus
  `activatepanedirection <direction>` plus WezTerm-style
  `wezterm.action.ActivatePaneDirection '<direction>'` bare-string and
  `wezterm.action.ActivatePaneDirection("<direction>")` function-call queries
  cover arbitrary zero-based current-tab pane indices and
  Left/Right/Up/Down/Next/Prev direction payloads.
  Action-name `activatepaneleft`, `activatepaneright`, `activatepaneup`,
  `activatepanedown`, `nextpane`, `previouspane`, and `activatepane1` through
  `activatepane8` queries dispatch the corresponding no-argument entries.
  Native `WindowCommand::ActivatePaneDirection(direction)` payloads dispatch
  the same Up/Down/Left/Right/Next/Previous pane focus path.
- `rssh-app` includes a quick-select overlay (`Ctrl+Shift+Space`) and
  command-palette Quick Select entry for WezTerm-style `QuickSelect`. It
  detects common URL/path/hash/IP/email patterns including WezTerm's non-http
  URL schemes (`git@`, `git://`, `ssh://`, `ftp://`), markdown URLs, diff
  paths, docker SHA values, paths, colors, UUID/IPFS/SHA hashes, IPv4/IPv6, hex
  addresses, and long numbers. It supports keyboard navigation including
  `Ctrl+N`/`Ctrl+P`, PageDown/PageUp page-wise movement, WezTerm's Enter
  PriorMatch binding, configurable labels honoring `quick_select_alphabet`,
  configurable `quick_select_patterns` appended to the defaults, including
  top-level static Lua table-variable assignments, configurable
  `disable_default_quick_select_patterns` so configured patterns become the
  full set, and native/effective-config storage for `quick_select_remove_styling`,
  quote-aware command-palette `quick select alphabet <chars>` for the native
  `QuickSelectArgs { alphabet = ... }` subset, command-palette
  `quick select pattern <regex>` and
  `quick select patterns <regex> ; <regex>` for native
  `QuickSelectArgs { patterns = ... }` override subsets, splitting only on
  unquoted ` ; ` separators so quoted regexes can include semicolons,
  command-palette `quick select scope lines <n>` for the native
  `QuickSelectArgs { scope_lines = ... }` subset with a complete numeric value,
  command-palette
  `quick select label <text>` for the native status/overlay label subset with
  quote-aware text parsing, command-palette `quick select action open uri` with
  quoted or unquoted action names for a native open-uri action subset using the
  same open-uri hook as hyperlink clicks, command-palette
  `quick select action copy to clipboard`,
  `quick select action copy to primary selection`, and
  `quick select action copy to clipboard and primary selection` for native
  `CopyTo` action subsets with quoted or unquoted destinations, command-palette
  `quick select action open uri skip action on paste`/`skip_action_on_paste`/
  `skip-action-on-paste`, including `=true|false` suffixes, for the native
  `skip_action_on_paste` subset on valid native action paths, including
  `action=<action> skip_action_on_paste=true|false` assignment forms,
  and WezTerm-style action-name `quickselectargs pattern`/`patterns`/
  `alphabet`/`label`/`action`/`scope lines`/`scope_lines`/`scope-lines` query
  prefixes, with `pattern=<regex>`, `patterns=<regex>[;<regex>]`, `alphabet=<chars>`,
  `label=<text>`, `action=<action>`, `scope_lines=<n>`, and `scope-lines=<n>` assignment forms plus
  legacy `quickselect ...` aliases, with assignment fields combinable in the
  same query for the same implemented `QuickSelectArgs`
  fields, and WezTerm-style
  quick-select labels: lowercase labels copy the match to
  ClipboardAndPrimarySelection, uppercase labels paste it into the pane. The
  native action payload also carries `QuickSelectArgs { patterns, alphabet,
  label, action, skip_action_on_paste, scope_lines }` directly for
  command-palette augmentation and later config wiring. The default
  `Ctrl+Shift+Space` key-assignment entry exposes `QuickSelect` with default
  native args, while `EnterQuickSelect` remains an internal command-palette
  query alias and action-name `enterquickselect` queries dispatch that default
  entry. WezTerm-style `wezterm.action.QuickSelectArgs { patterns = { ... },
  alphabet = ..., label = ..., action = ... }` Lua table queries parse the same
  implemented options, including bracketed string table keys and nested
  `wezterm.action { CopyTo = ... }` wrapper keys. Arbitrary custom callback actions
  remain open.
- `rssh-app` exposes a native WezTerm-style `PromptInputLine` action payload
  with `description`, `prompt`, and `initial_value`. It opens a modal line-input
  overlay, honors WezTerm's `"> "` default prompt when `prompt` is omitted,
  submits `Some(line)` to a typed native handler on Enter, and submits `None`
  on Escape or `Ctrl+C`. The structured command-palette query `prompt input
  line description <text> [prompt <text>] [initial_value <text>]` plus
  action-name `promptinputline description <text> [prompt <text>]
  [initial_value <text>]` dispatches the same native payload subset with
  quote-aware text parsing, accepts `initial_value`, `initial value`, and
  `initial-value` field keys, supports both `field <text>` and `field=<text>`
  forms, and keeps field-key words inside text values when a later valid field
  boundary exists. WezTerm-style
  `wezterm.action.PromptInputLine { description = ..., prompt = ...,
  initial_value = ... }` table-call queries also dispatch that native field
  subset. The documented static rename-tab callback form maps submitted text to
  the native `RenameTabTo` command; arbitrary Lua `wezterm.action_callback`
  execution remains later parity work.
- `rssh-app` exposes a native WezTerm-style `InputSelector` action payload with
  `title`, `choices`, `fuzzy`, `alphabet`, `description`, and
  `fuzzy_description`. It opens a modal selector, supports default-mode alphabet
  shortcuts, `/` fuzzy filtering, `j`/`k` and arrow/Ctrl movement, Enter
  selection, and Escape/`Ctrl+C`/`Ctrl+G` cancellation. Default-mode text that
  is not in `alphabet` is ignored until fuzzy mode is entered, matching
  WezTerm's split between shortcut selection and fuzzy filtering. The selector
  dispatches a typed native handler with selected `id`/`label` or `None` values
  on cancel. The
  structured command-palette query `input selector title <text> choices
  <id=label ; id=label> [alphabet <chars>] [description <text>]
  [fuzzy_description <text>] [fuzzy true|false|fuzzy=true|false]` dispatches
  the same native payload subset; action-name queries starting with
  `inputselector ...` dispatch the same payload. Both use quote-aware field
  parsing, accept
  `fuzzy_description`, `fuzzy description`, and `fuzzy-description` field keys,
  support both `field <text>` and `field=<text>` forms for selector fields,
  split choices only on unquoted semicolon separators including compact
  `id=label;id=label` forms so quoted labels can include semicolons, and keep
  field-key words inside title/description values when a later valid field
  boundary exists. Known fields following `choices` are treated as the earliest
  structured boundary, and duplicate `fuzzy` fields are rejected instead of
  silently overriding them. WezTerm-style
  `wezterm.action.InputSelector { title = ..., choices = "...", alphabet = ...,
  description = ..., fuzzy_description = ..., fuzzy = ... }` table-call queries
  also dispatch that native field subset when `choices` uses the existing
  semicolon-delimited string form or WezTerm's Lua table-of-tables choice form
  with `{ label = ..., id = ... }` entries, including bracketed string keys on
  those nested choice tables. Static callback bodies that call
  `pane:send_text(id)` or `pane:send_text(label)` map selected choice data to
  the native `SendString` path; arbitrary Lua `wezterm.action_callback`
  execution remains later parity work.
- `rssh-app` exposes a native WezTerm-style `Confirmation` action payload with a
  message string, required Yes action, and optional No/cancel action. It opens a
  modal confirmation overlay, dispatches a typed native handler with
  `accepted = true` on Enter/`Y`/Space before running the Yes action, and
  dispatches `accepted = false` on Escape/`N`/`Ctrl+C`/`Ctrl+G` before running
  the optional cancel action. The structured command-palette query
  `confirmation message <text> action <command> [cancel <command>]` dispatches
  the same native payload subset, and action-name `confirmationmessage ...`
  aliases dispatch that payload for typed nested commands such as `send string`,
  `send key`, `emit event`, key-table stack mutations, copy/paste,
  clear-scrollback, and close-current-pane/tab confirmations, while keeping
  field-key words inside message/action text when a later valid field boundary
  exists. Message fields use quote-aware parsing, and `message`/`action`/`cancel`
  accept both `field <text>` and `field=<text>` forms. WezTerm-style
  `wezterm.action.Confirmation { message = ..., action = ..., cancel = ... }`
  table-call queries also dispatch the same native nested-command subset. Static
  callback bodies that call `window:perform_action(<implemented action>, pane)`
  map onto existing native commands; arbitrary Lua `wezterm.action_callback`
  execution remains later parity work.
- `rssh-app` exposes a native WezTerm-style `EmitEvent` action payload carrying
  a custom event name. Executing it dispatches a typed native handler with the
  active window id and pane id. The structured command-palette query
  `emit event <name>` plus action-name `emitevent <name>` dispatches the same
  typed payload path with quote-aware event-name parsing. WezTerm-style
  `wezterm.action.EmitEvent { name = ... }` table-call queries dispatch the
  same typed payload path. Lua `wezterm.on`/`wezterm.emit` wiring remains later
  parity work.
- `rssh-app` exposes native WezTerm-style `ActivateKeyTable`, `PopKeyTable`,
  and `ClearKeyTableStack` action payloads that maintain a per-window
  key-table activation stack, show the active table in native window status and
  the typed title-formatting snapshot, and clear the stack when configuration
  is reloaded. Timed activations expire from the stack via
  `timeout_milliseconds`, matching native key-table assignments reset that
  timeout, and one-shot activations pop on the next native key press.
  `prevent_fallback` activations consume unmatched native key presses so they
  do not fall through to default shortcuts or PTY input, while `until_unknown`
  activations pop when an unmatched native key press is seen. The structured
  command-palette query `activate key table <name> [timeout <ms>] [one shot
  true|false] [replace current true|false] [until unknown true|false] [prevent
  fallback true|false]` dispatches native `ActivateKeyTable` payloads, with
  snake_case and hyphenated field aliases such as `timeout_milliseconds`/
  `timeout-milliseconds`, `one_shot`/`one-shot`, `replace_current`/
  `replace-current`, `until_unknown`/`until-unknown`, and `prevent_fallback`/
  `prevent-fallback`, and accepts single-token assignment forms such as
  `timeout=<ms>`, `one_shot=false`, and `prevent-fallback=true`. `one shot`
  defaults to true when omitted and single or double quotes group key-table
  names that contain spaces. Duplicate option fields are rejected instead of
  silently overriding earlier values. Action-name `activatekeytable ...`
  aliases dispatch the same activation payloads.
  `popkeytable` and `clearkeytablestack` dispatch the same stack mutations as
  their spaced query forms.
  Native `key_tables` overrides now match table entries from the activation
  stack top downward and execute the matched native action. Static
  WezTerm-style `config.keys` and `config.key_tables` snippets parse the
  implemented native assignment subset into runtime key-table overrides,
  including bracketed string table keys with long-bracket values for key-table
  names and nested assignment fields, static table variable assignments such as
  `config.keys = user_keys` or `config.key_tables = user_key_tables`, and
  static return-table fields such as `return { keys = user_keys }` or
  `return { key_tables = user_key_tables }`, plus top-level static
  `table.insert(config.keys, { ... })` appends plus
  `table.insert(config.key_tables.<name>, { ... })` nested appends and
  static table variables such as
  `table.insert(config.key_tables.<name>, item)` or
  `table.insert(config.key_tables.<name>, index, item)`, plus
  `table.insert(config.key_tables.<name>, index, { ... })` numeric-position
  inserts, with bracket field selectors such as `config['key_tables']`
  supported for nested inserts. Full Lua config evaluation remains later parity
  work.
- `rssh-app` includes a WezTerm-style `PaneSelect` overlay from the command
  palette entry `Pane Select`. It labels panes with the WezTerm default
  selection alphabet (`a`, `s`, `d`, ...) and honors the native effective
  `quick_select_alphabet` value when configured. The quote-aware
  command-palette query `pane select alphabet <chars>` and explicit-mode
  `pane select activate alphabet <chars>` plus action-name `paneselect ...`
  aliases cover the native Activate plus per-action alphabet subset,
  action-name `enterpaneselect` queries dispatch the default Activate entry,
  default Activate mode activates the selected pane when a label is typed, and
  `Esc`/`Ctrl+g` exits without changing focus. The
  command-palette `Pane Select Show Pane IDs` entry covers the native
  `show_pane_ids=true` subset by rendering labels as `label:pane_id` while
  preserving Activate behavior, and quote-aware
  `pane select show pane ids alphabet <chars>`/`show-pane-ids alphabet <chars>`
  plus `pane select activate show pane ids alphabet <chars>`/
  `show-pane-ids alphabet <chars>` cover the native combined Activate,
  `show_pane_ids=true`, and per-action alphabet subset, with
  `alphabet=<chars>` assignment forms accepted for the same alphabet field.
  Action-name
  `enterpaneselectshowpaneids` dispatches the default show-pane-ids entry. The
  implemented non-default mode queries (`swap`, `swap keep focus`, `move to new
  tab`, and `move to new window`) can also include `show pane ids`,
  `show_pane_ids`, or `show-pane-ids`, and may add quote-aware
  `alphabet <chars>` after that to combine mode,
  `show_pane_ids=true`, and a per-action alphabet. The native action payload
  also carries `PaneSelect { mode, show_pane_ids, alphabet }` directly for
  command-palette augmentation and later config wiring. Structured
  `pane select mode <mode>` / `pane select mode=<mode>` queries with
  `[show_pane_ids true|false] [show_pane_ids=true|false]
  [alphabet <chars>|alphabet=<chars>]` fields and action-name `paneselect ...`
  aliases map WezTerm-style option names to the same payload and reject duplicate
  structured fields. WezTerm-style `wezterm.action.PaneSelect { mode = ...,
  show_pane_ids = ..., alphabet = ... }` and parenthesized table-call queries
  dispatch the same native field subset, including long-bracket table keys;
  config-file wiring remains later parity work.
- `rssh-core` exposes `ActivatePaneByIndex` for WezTerm-style current-tab pane
  index activation. `rssh-app` exposes command-palette Activate Pane By Index
  1..8 entries, the structured queries `activate pane <index>` and
  `activate pane by index <index>` plus `activatepanebyindex <index>`,
  WezTerm-style `wezterm.action.ActivatePaneByIndex(<index>)` function-call
  queries, action-name `activatepane1` through `activatepane8`, and a native
  `WindowCommand::ActivatePaneByIndex(index)` payload for arbitrary zero-based
  pane indices, ignoring invalid pane indices.
- `rssh-core` exposes `RotatePanes` for WezTerm-style clockwise and
  counter-clockwise pane identity rotation while preserving split positions and
  size deltas. `rssh-app` exposes command-palette Rotate Panes Clockwise and
  Rotate Panes Counter Clockwise entries, `rotate panes <direction>` queries
  plus `rotatepanes <direction>` action-name queries with quoted or unquoted
  directions plus WezTerm-style `wezterm.action.RotatePanes("<direction>")`
  function-call queries for Clockwise/CounterClockwise native payloads, plus native
  `WindowCommand::RotatePanes(direction)` payloads for both directions.
- `rssh-app` includes WezTerm-style pane-select swap mode entries from the command
  palette: `Pane Select Swap With Active` exchanges the active pane's layout
  position with the selected pane and focuses the selected pane, while `Pane Select
  Swap With Active Keep Focus` keeps focus on the original active pane after the
  exchange. Action-name `enterpaneswap` and `enterpaneswapkeepfocus` queries
  dispatch those default mode entries.
- `rssh-app` includes a pane-select `MoveToNewTab` mode from the command
  palette. Selecting a pane moves it into a newly created tab in the same
  workspace and activates that tab. Action-name `enterpanemovetonewtab`
  queries dispatch that default mode entry.
- `rssh-app` includes a pane-select `MoveToNewWindow` mode from the command
  palette. Selecting a pane removes it from the current split layout and records
  a pending native-window request with its own tab and active pane. Action-name
  `enterpanemovetonewwindow` queries dispatch that default mode entry.
- Pending `MoveToNewWindow` requests can be consumed into an independent
  app-shell/native-window app state while transferring the detached pane runtime
  snapshot.
- `rssh-app window` now runs through a multi-window manager that materializes
  detached `MoveToNewWindow` app states as additional native OS windows.
- WezTerm-style `SpawnWindow` is exposed through the default `Ctrl+Shift+N`
  shortcut and command-palette `Spawn Window` entry. It creates a pending native
  window with a fresh tab and pane from the default launch configuration, and
  the multi-window manager materializes it as an additional OS window. The
  command-palette `spawn window <program> [args...]` query uses the same pending
  native-window path with an explicit pane launch command.
- WezTerm-style `ToggleFullScreen` is exposed through the default `Alt+Enter`
  shortcut and command-palette `Toggle Full Screen` entry, toggling the native
  window fullscreen state when a window exists and dispatching the typed
  resize hook with current fullscreen dimension metadata. Action-name
  `togglefullscreen` queries dispatch the same command.
- WezTerm-style `StartWindowDrag` is exposed through command-palette
  `Start Window Drag` and the default `SUPER` + left drag / `CTRL|SHIFT` +
  left drag bindings, requesting native drag-to-move through the platform
  window backend when a window exists. Action-name `startwindowdrag` queries
  dispatch the same command. Native `disable_default_mouse_bindings`
  defaults to false and suppresses the implemented default mouse-assignment
  subset when true, including built-in wheel scroll/alternate-screen arrow
  fallback when no user wheel binding matched. Mouse bindings that use
  `DisableDefaultAssignment` suppress the matching default mouse assignment
  without consuming the event, matching WezTerm's opt-out semantics. Static
  WezTerm-style `table.insert(config.mouse_bindings, { ... })` appends parse
  into the same native mouse assignment path.
- Native `hide_mouse_cursor_when_typing` defaults to true, hides the OS mouse
  cursor on key press while the cursor is inside the native window, and
  restores it on mouse motion or cursor leave.
- Native `disable_default_key_bindings` defaults to false and suppresses the
  implemented built-in WezTerm-style default key assignments when true.
- WezTerm-style `ActivateWindow`, `ActivateWindowRelative`, and
  `ActivateWindowRelativeNoWrap` native action payloads request manager-level
  focus across materialized OS windows. Target selection is ordered by app
  window id; `ActivateWindow` uses zero-based absolute indexes, the default
  relative action wraps, while `NoWrap` stops at the edge. The command palette
  now accepts `activate window <index>`, `activate window index <index>`,
  `activatewindow <index>`, `activate window relative <offset>`,
  `activatewindowrelative <offset>`,
  `activate window relative no wrap <offset>`, and
  `activatewindowrelativenowrap <offset>` queries plus WezTerm-style
  `wezterm.action.ActivateWindow(<index>)`,
  `wezterm.action.ActivateWindowRelative(<offset>)`, and
  `wezterm.action.ActivateWindowRelativeNoWrap(<offset>)` function-call queries
  for those same payloads.
- WezTerm-style `SetWindowLevel` native action payloads accept
  `AlwaysOnBottom`, `Normal`, and `AlwaysOnTop`, updating the app's remembered
  window level and applying it to the platform window through winit's
  `WindowLevel` API when backend support exists. The command-palette query
  `set window level <value>` plus the action-name spelling
  `setwindowlevel <value>` maps AlwaysOnBottom/Normal/AlwaysOnTop spellings to
  the same native payload with quote-aware value parsing, including WezTerm-style
  `wezterm.action.SetWindowLevel '<value>'` and
  `wezterm.action.SetWindowLevel('<value>')` Lua action queries.
- WezTerm-style `ToggleAlwaysOnTop` and `ToggleAlwaysOnBottom` are exposed
  through command-palette entries and native action payloads, toggling the
  remembered window level between the requested z-order and `Normal`.
  Action-name `togglealwaysontop` and `togglealwaysonbottom` queries dispatch
  the corresponding commands.
- WezTerm-style `Show` is exposed through the command palette and native action
  payloads. It clears a prior native hide request and, when a window exists,
  restores visibility, unminimizes, and requests focus. Action-name `show`
  queries dispatch the same command.
- WezTerm-style `Hide` is exposed through the default `Super+M` shortcut and
  command-palette `Hide` entry. It requests native hide/minimize state, using
  platform window minimization when a window exists. Action-name `hide` queries
  dispatch the same command.
- WezTerm-style `HideApplication` is exposed through the macOS-default
  `Super+H` shortcut, command-palette `Hide Application` entry, and
  action-name `hideapplication` query. It records an application-hide request
  and uses native window minimization as the current platform fallback when a
  window exists. The default `KEY_ASSIGNMENTS` list includes `Super+H` only on
  macOS, matching WezTerm's platform-specific default.
- WezTerm-style `QuitApplication` is exposed through command-palette `Quit
  Application` and action-name `quitapplication` queries. It requests
  whole-application shutdown, drops pending native window apps, and preserves
  final metrics.
- WezTerm-style `DecreaseFontSize`, `IncreaseFontSize`, `ResetFontSize`, and
  command-palette `ResetFontAndWindowSize` update the native window's logical
  font-size scale by WezTerm's 10% step or reset it to the configured baseline.
  Action-name `decreasefontsize`, `increasefontsize`, `resetfontsize`, and
  `resetfontandwindowsize` queries dispatch the same commands.
  Native `font_size` defaults to WezTerm's `12.0` points and scales the fixed
  native base cell metrics. Native `cell_width` defaults to WezTerm's `1.0`
  ratio and further scales horizontal cell geometry, while native `line_height`
  defaults to WezTerm's `1.0` ratio and further scales vertical cell geometry
  used for rendering, hit testing, terminal size calculation, and frame sizing;
  shortcut zoom remains an additional scale over that configured baseline. Native
  `adjust_window_size_when_changing_font_size`
  defaults to the
  non-tiling WezTerm effective behavior of true, preserving terminal
  rows/columns by resizing the native frame and requesting the matching
  OS-window inner size when a native window exists; setting it false keeps the
  current window size and recomputes terminal rows/columns from the scaled cell
  size. Reset Font And Window Size also restores the native frame to the
  configured initial rows and columns. Native config overrides expose
  `font_size`, `cell_width`, `cell_widths`, `line_height`,
  `font_antialias`, `font_hinting`, `font_rasterizer`, `font_shaper`,
  `font_dirs`, `font_locator`, `custom_block_glyphs`,
  `anti_alias_custom_block_glyphs`,
  `allow_square_glyphs_to_overflow_width`, `freetype_load_target`,
  `freetype_render_target`, `freetype_load_flags`,
  `freetype_interpreter_version`, `freetype_pcf_long_family_names`,
  `display_pixel_geometry`, `dpi`, `initial_cols`, `initial_rows`, and
  `adjust_window_size_when_changing_font_size`; static WezTerm-style Lua
  snippets for those fields now parse into the same native override path.
  `config.dpi` overrides the detected native window DPI for renderer state and
  FreeType defaults until the override is cleared. Static `config.font_dirs`
  snippets parse inline or through top-level static table variables, and
  `config.font_locator = 'ConfigDirsOnly'` snippets are retained in effective
  config. Actual renderer glyph strategy, configured font-directory
  scanning, font-locator application, shaping-engine application, FreeType
  interpreter application, subpixel geometry application, PCF font-resolution
  changes, and full Lua config evaluation remain later parity work.
- WezTerm-style `ShowDebugOverlay` is exposed through the default
  `Ctrl+Shift+L` shortcut, command-palette `Show Debug Overlay` entry, and
  action-name `showdebugoverlay` query. It records native debug-overlay state
  for the active window and renders a visible native diagnostic overlay with
  current window/tab/pane/workspace and runtime state plus recent native
  diagnostic log lines from key-event, unknown-escape, and missing-glyph
  warnings; bare `Esc` closes the overlay without forwarding input to the PTY.
  Lua REPL support and full external log-source integration remain later parity
  work.
- WezTerm-style `CharSelect` is exposed through the default `Ctrl+Shift+U`
  shortcut and command-palette `Char Select` entry. It enters native
  character-selection mode and closes other active overlays; native
  `CharSelectArgs` payloads carry `copy_on_select`, `copy_to`, and `group`
  into the overlay state, and the structured command-palette query
  `char select copy_on_select <bool> copy_to <destination> group <name>` plus
  WezTerm-style action-name `charselect` default and argument queries open the same
  typed payload path with quote-aware field parsing and `field=value` assignment
  forms including `copy-on-select=false`, `copy-to=<destination>` /
  `copy-to="primary selection"`, and `group=<name>` /
  `group="<name with spaces>"`, so quoted group values with spaces do not retain
  their quotes. Duplicate `copy_on_select`, `copy_to`, and
  `group` fields are rejected instead of silently overriding an earlier field.
  When `group` is omitted, the overlay resolves it
  to `RecentlyUsed` after an accepted character selection and to
  `SmileysAndEmotion` before any selection history, matching WezTerm's default
  group rule; opening `RecentlyUsed` with no typed filter renders recent
  character candidates and can reselect earlier accepted characters. `Esc` /
  `Ctrl+G` cancellation plus typed text
  input, Backspace editing, `Ctrl+U` input clearing, `Ctrl+R` /
  `Ctrl+Shift+R` group cycling, and Enter acceptance for raw, `U+`, and `0x`
  hex Unicode codepoint input stay inside the modal without forwarding those keys to the PTY. Accepted
  codepoints insert into the active pane and honor `copy_on_select` /
  `copy_to` for Clipboard, PrimarySelection, or both configured copy targets.
  Standard Unicode character-name input such as `grinning face`, plus fuzzy
  token queries such as `grin face`, resolve through the same Enter acceptance
  path. Window title/status text shows `Char Select`, includes the active
  group, surfaces the current text input, renders a visible candidate overlay
  for name/codepoint matches, RecentlyUsed entries, and initial built-in
  category candidates including NerdFonts private-use glyphs; typed fuzzy
  queries and hex codepoint input also match the built-in NerdFonts names.
  ArrowUp/ArrowDown moves the selected candidate before Enter acceptance while
  scrolling the overlay past the first visible rows. RecentlyUsed candidates use
  persisted JSON selection counts plus a last-used sequence across app
  instances. Rendering the full categorized character picker/database plus
  exact WezTerm frecency scoring remains later parity work.
- PTY reader events now carry the app-shell `WindowId` plus `PaneId`, so
  independent windows do not rely on globally unique pane IDs for event routing.
  PTY EOF handling waits for the process status and honors native
  `exit_behavior` overrides for `Close`, `Hold`, and `CloseOnCleanExit`;
  configured `clean_exit_codes` are treated as clean for `CloseOnCleanExit`.
  Native `exit_behavior_messaging` controls held-pane status text verbosity,
  with `None` suppressing the message and verbose text reporting the actual
  `exit_behavior` value that kept the pane open; verbose/brief messages use
  WezTerm's documented success/failure prefixes. Static Lua config parsing
  covers `exit_behavior`, `exit_behavior_messaging`, and `clean_exit_codes`
  inline or through top-level static table variables.
- `rssh-app` includes copy mode (`Ctrl+Shift+X`) with Vim-like movement and copy
  actions (Space/`v`, `V`, `y`, `Enter`, cursor movement keys,
  Home/End/`^`/`$`, etc.), and the command palette exposes WezTerm-style
  `ActivateCopyMode` as Activate Copy Mode. Native
  `WindowCommand::ActivateCopyMode` payloads enter the same copy-mode path, and
  structured `activatecopymode` and `entercopymode` action-name queries resolve
  to the same copy-mode paths. The default `Ctrl+Shift+X` key-assignment entry
  now exposes that WezTerm-style payload while the older `EnterCopyMode` alias
  remains accepted.
- Copy mode supports WezTerm-style semantic-zone movement across retained
  scrollback with `z`/`Shift+Z`, plus typed Prompt/Input/Output zone movement
  via `Alt+P`, `Alt+I`, and `Alt+O`/`Alt+Z`, backed by OSC 133 zones.
- Copy mode keeps source-row selection anchors, so selections that span the
  live viewport and retained scrollback can be copied with `y`.
- Mouse double-click word selection honors `selection_word_boundary`, including
  WezTerm's documented default boundary set and native per-window overrides.
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
- Default `Ctrl+Shift+F`/`Super+F` search shortcuts open WezTerm-style
  `Search(CaseSensitiveString="")`, while command-palette Search exposes the
  same search overlay with search table navigation via Down/Up,
  `Ctrl+N`/`Ctrl+P`, PageDown/PageUp, `Ctrl+R` match-type cycling, `Ctrl+U`
  clear-pattern, and character ESC close. Command-palette `search <pattern>`,
  `search regex <pattern>`, `search case-sensitive <pattern>`, and
  `search case-insensitive <pattern>` / `search case insensitive <pattern>`
  queries open Search with that initial typed pattern using quote-aware parsing.
  WezTerm-style action field names `search casesensitivestring <pattern>` and
  `search caseinsensitivestring <pattern>` dispatch the same typed search
  payloads, and `search current selection or empty string` maps to WezTerm-style
  `CurrentSelectionOrEmptyString`. Native `Search` action payloads now support
  typed `Regex`, `CaseSensitiveString`, and `CaseInSensitiveString` patterns
  through the same search path, plus
  `CurrentSelectionOrEmptyString` to reuse the selected text collapsed to a
  single line or open an empty search overlay when nothing is selected; Lua
  parsing/wiring remains future work. Plain `Ctrl+F` remains available to the
  active PTY.
- `rssh-app` exposes WezTerm-style `ClearSelection` through the command palette
  and structured `clearselection` action-name queries, clearing the active
  window selection and refreshing the rendered highlight.
- `rssh-app` exposes WezTerm-style `SelectTextAtMouseCursor` and
  `ExtendSelectionToMouseCursor` Cell, Word, Line, and Block modes through
  command-palette entries and native action payloads using the current mouse
  cell. Structured queries accept `select text at mouse cursor <mode>` /
  `selecttextatmousecursor <mode>` and `extend selection to mouse cursor
  <mode>` / `extendselectiontomousecursor <mode>` action-name forms, plus
  WezTerm-style `wezterm.action.SelectTextAtMouseCursor '<mode>'` and
  `wezterm.action.ExtendSelectionToMouseCursor '<mode>'` Lua action queries. It
  maps default left-mouse selection to WezTerm-style Cell/Word/Line click
  streaks, `SHIFT` selection extension, `ALT` rectangular block drag, and
  `ALT|SHIFT` rectangular extension. Releasing a non-empty left-drag selection
  or modified extension copies it to ClipboardAndPrimarySelection, while
  NONE/SHIFT single-click release can open the OSC 8 hyperlink under the mouse.
  Double/triple-click drag extends by Word/Line boundaries, and
  double/triple-click release completes the selected word or line to
  ClipboardAndPrimarySelection. It
  also exposes WezTerm-style `SelectTextAtMouseCursor` SemanticZone selection
  through those paths for the OSC 133 semantic zone under the mouse.
- `rssh-app` exposes WezTerm-style `ActivateCommandPalette` through the command
  palette and the default `Ctrl+Shift+P` shortcut; invoking it from the palette
  closes the current palette action and reopens a fresh command palette.
  Action-name `activatecommandpalette` queries dispatch the same command. The
  native command palette renders a visible candidate overlay whose row count
  honors `command_palette_rows`, falling back to a terminal-height-based
  default when unset. It also keeps command-palette frecency for executed
  command labels in memory and persists it to a JSON state file so later app
  instances can promote frequently and recently used entries, while fuzzy
  queries keep match score first and use frecency only as a tie-breaker.
- Native default key-assignment entries include the implemented WezTerm
  defaults for tab navigation/movement, split creation, pane focus, and pane
  resize so `ShowLauncherArgs { flags = KEY_ASSIGNMENTS }` can surface those
  bindings alongside user overrides.
- `rssh-app` exposes native WezTerm-style
  `CloseCurrentPane { confirm = false }` and
  `CloseCurrentTab { confirm = false }` action payloads through the same
  immediate-close path as the command-palette Close Current Pane/Tab entries.
  Action-name `closepane` and `closetab` queries dispatch the no-argument
  immediate-close aliases.
  The command palette also accepts structured
  `close current pane confirm true|false`,
  `close current pane confirm=true|false`,
  `close current tab confirm true|false`,
  `close current tab confirm=true|false`,
  `closecurrentpane confirm true|false`,
  `closecurrentpane confirm=true|false`,
  `closecurrenttab confirm true|false`, and
  `closecurrenttab confirm=true|false` queries for the typed payloads.
  WezTerm-style `wezterm.action.CloseCurrentPane { confirm = ... }` and
  `wezterm.action.CloseCurrentTab { confirm = ... }` table-call queries
  dispatch the same payloads.
  `confirm = true` opens a native confirmation overlay that captures the target
  pane/tab at invocation time, accepts Enter/Y, and cancels with
  Esc/N/Ctrl-C/Ctrl-G before dispatching the close action.
- `rssh-app` exposes WezTerm-style `ReloadConfiguration` through the command
  palette and the default `Ctrl+Shift+R` shortcut, dispatching a typed native
  `window-config-reloaded` hook with the window id and active pane id.
  Action-name `reloadconfiguration` queries dispatch the same command.
  A typed native
  `set_config_overrides`/`get_config_overrides` subset stores per-window
  overrides for implemented effective-config fields (`dpi`, `tab_max_width`,
  `status_update_interval`, `max_fps`, `animation_fps`, `cursor_blink_rate`, `cursor_blink_ease_in`,
  `cursor_blink_ease_out`, `text_blink_rate`, `text_blink_rate_rapid`,
  `text_blink_ease_in`, `text_blink_ease_out`, `text_blink_rapid_ease_in`,
  `text_blink_rapid_ease_out`, `font_size`, `cell_width`, `cell_widths`,
  `line_height`, `font_antialias`, `font_hinting`, `font_rasterizer`,
  `font_shaper`, `font_dirs`, `font_locator`, `custom_block_glyphs`,
  `anti_alias_custom_block_glyphs`,
  `allow_square_glyphs_to_overflow_width`, `freetype_load_target`,
  `freetype_render_target`, `freetype_load_flags`,
  `freetype_interpreter_version`, `freetype_pcf_long_family_names`,
  `display_pixel_geometry`, `dpi`, `foreground_text_hsb`, `bold_brightens_ansi_colors`,
  `text_background_opacity`, `window_background_opacity`, `window_decorations`,
  `default_cursor_style`, `cursor_thickness`, `underline_thickness`,
  `underline_position`, `strikethrough_position`, `force_reverse_video_cursor`,
  `window_padding`, `window_content_alignment`, `initial_cols`, `initial_rows`, `adjust_window_size_when_changing_font_size`, `inactive_pane_hsb`, `command_palette_rows`, `launcher_alphabet`, `quick_select_alphabet`, `quick_select_patterns`, `disable_default_quick_select_patterns`, `quick_select_remove_styling`, `hyperlink_rules`, `selection_word_boundary`, `term`, `audible_bell`, `visual_bell`, `color_scheme_dirs`, `foreground_color`, `background_color`, `ansi_palette`, `indexed_palette`, `selection_fg_color`, `selection_bg_color`, `cursor_bg_color`, `cursor_border_color`, `cursor_fg_color`, `visual_bell_color`, `notification_handling`, `default_prog`, `default_domain`, `default_workspace`, `prefer_to_spawn_tabs`, `automatically_reload_config`, `check_for_updates`, `check_for_updates_interval_seconds`, `show_update_window`, `use_resize_increments`, `debug_key_events`, `log_unknown_escape_sequences`, `warn_about_missing_glyphs`, `default_cwd`, `set_environment_variables`, `scroll_to_bottom_on_input`, `alternate_buffer_wheel_scroll_speed`, `canonicalize_pasted_newlines`, `quote_dropped_files`, `disable_default_key_bindings`, `disable_default_mouse_bindings`, `hide_mouse_cursor_when_typing`, `pane_focus_follows_mouse`, `swallow_mouse_click_on_pane_focus`, `swallow_mouse_click_on_window_focus`, `bypass_mouse_reporting_modifiers`, `enable_scroll_bar`, `min_scroll_bar_height`, `enable_tab_bar`,
  `hide_tab_bar_if_only_one_tab`, `unzoom_on_switch_pane`, `tab_bar_at_bottom`,
  `tab_and_split_indices_are_zero_based`,
  `mouse_wheel_scrolls_tabs`,
  `switch_to_last_active_tab_when_closing_tab`,
  `quit_when_all_windows_are_closed`,
  `window_close_confirmation`,
  `exit_behavior`,
  `clean_exit_codes`,
  `exit_behavior_messaging`,
  `skip_close_confirmation_for_processes_named`,
  `show_close_tab_button_in_tabs`,
  `show_new_tab_button_in_tab_bar`, `show_tab_index_in_tab_bar`, and
  `show_tabs_in_tab_bar`) and emits
  `window-config-reloaded` on every set. Static WezTerm-style
  `foreground_text_hsb` and `inactive_pane_hsb` tables parse inline or through
  top-level static table variables. `automatically_reload_config` is
  stored with WezTerm's default `true` and included in effective config
  snapshots. `check_for_updates` is stored with WezTerm's default `true`,
  `check_for_updates_interval_seconds` with the default `86400`, and
  `show_update_window` with the compatibility default `false`; actual update
  checks and update-window UI remain later parity work. `max_fps` is stored
  with WezTerm's default `60`, and `animation_fps` with the default `10`;
  actual frame pacing and animation redraw scheduling remain later parity
  work. `use_resize_increments`
  is stored with WezTerm's default `false`
  and included in effective config snapshots; actual OS-specific window resize
  increment application remains later parity work. `debug_key_events` and
  `log_unknown_escape_sequences` are stored with WezTerm's default `false`
  and included in effective config snapshots. `warn_about_missing_glyphs` is
  stored with WezTerm's default `true` and included in effective config
  snapshots. Missing glyph codepoints detected in rendered cells are emitted
  once per native window as stderr `CONFIG ERROR missing glyph ...`
  diagnostics when `warn_about_missing_glyphs` is enabled; setting it false
  suppresses those diagnostics. Unknown ESC/CSI sequences are recorded by the
  terminal runtime and emitted as native stderr warnings when
  `log_unknown_escape_sequences` is enabled. Native key events are emitted as
  stderr `INFO key_event` diagnostics when `debug_key_events` is enabled; full
  WezTerm-style configuration error window UI, actual Lua config reload,
  automatic file watching, Lua `window:set_config_overrides` wiring, and
  broader config option coverage remain later parity work.
- `rssh-app` parses native `window_padding` px and cell-unit side padding
  inline or through top-level static table variables, and parses static
  `window_content_alignment` values for horizontal
  `Left`/`Center`/`Right` and vertical `Top`/`Center`/`Bottom` inline or
  through top-level static table variables. When explicitly
  configured, non-cell-multiple window sizes keep their real framebuffer size,
  fill leftover gap pixels with the configured background, align the terminal
  cell grid into that gap, and reverse-map mouse coordinates through the same
  offset.
- `rssh-app` dispatches a typed native `augment-command-palette` hook whenever
  the command palette opens, carrying the window id and active pane id. Returned
  entries provide `brief`, optional `doc`/`icon`, and an implemented
  `WindowCommand` action, participate in the same fuzzy filtering, palette
  status, selection, and execution flow, and render optional `doc` text plus
  known Nerd Font `icon` names including `md_rename_box`, `fa_clock_o`, and
  `cod_github` beside the brief label. Lua event wiring, arbitrary Lua
  callbacks, full Nerd Font icon catalog coverage, exact WezTerm frecency, and
  full WezTerm action-value parity remain later work.
- `rssh-app` exposes a native WezTerm-style `Multiple` action payload for the
  implemented `WindowCommand` subset. It executes commands in order and stops on
  the first failed command, so native key/palette entries can compose covered
  actions behind a single trigger. The structured command-palette query
  `multiple <command> ; <command> [; <command>...]` dispatches the same payload
  subset for typed nested commands, splitting only on unquoted ` ; `
  separators so quoted `send string` payloads can contain semicolons.
- `rssh-app` exposes WezTerm-style `ClearScrollback('ScrollbackOnly')` through
  the command palette and native action payloads, clearing active-pane history
  on the output side while preserving the viewport. The structured command
  palette queries `clear scrollback scrollback only` and
  `clearscrollback scrollback only` map to the same payload and accept quoted
  or unquoted mode text. WezTerm-style
  `wezterm.action.ClearScrollback { mode = ... }` table-call queries dispatch
  the same native payload path.
- `rssh-app` exposes WezTerm-style
  `ClearScrollback('ScrollbackAndViewport')` through the command palette and
  native action payloads, clearing active-pane history plus the viewport while
  preserving the prompt/cursor row as the new first visible line. The
  structured command palette queries `clear scrollback scrollback and viewport`
  and `clearscrollback scrollback and viewport` map to the same payload and
  accept quoted or unquoted mode text. Action-name
  `clearscrollbackandviewport` queries dispatch the no-argument compatibility
  command.
- `rssh-app` exposes WezTerm-style `CopyTo('Clipboard')` through the command
  palette and native action payloads for the active selection. The default
  `Super+C` and `Ctrl+Shift+C` shortcuts plus the dedicated `Copy` key map to
  the same Clipboard destination. The structured command palette queries
  `copy to <destination>` and `copyto <destination>` map Clipboard,
  PrimarySelection, and
  ClipboardAndPrimarySelection spellings, quoted or unquoted, to native
  `CopyTo(destination)` payloads; action-name `copytoclipboard`,
  `copytoprimaryselection`, and `copytoclipboardandprimaryselection` queries
  dispatch the same commands.
- `rssh-app` exposes WezTerm-style `CopyTo('PrimarySelection')` and
  `CopyTo('ClipboardAndPrimarySelection')` through the command palette routing
  layer and native action payloads. The actual OS PrimarySelection backend is
  still a platform-adapter follow-up.
- `rssh-app` exposes WezTerm-style `PasteFrom('Clipboard')` through the command
  palette and native action payloads into the active pane. The default
  `Super+V` and `Ctrl+Shift+V` shortcuts plus the dedicated `Paste` key map to
  the same Clipboard source; unmodified `Ctrl+V` remains available to the
  active PTY application. The structured command palette queries
  `paste from <source>` and `pastefrom <source>` map Clipboard and
  PrimarySelection spellings, quoted or unquoted, to native
  `PasteFrom(source)` payloads; action-name `pastefromclipboard` and
  `pastefromprimaryselection` queries dispatch the same commands, as do
  WezTerm-style `wezterm.action.PasteFrom '<source>'` and
  `wezterm.action.PasteFrom('<source>')` Lua action queries. Native
  `canonicalize_pasted_newlines` normalizes
  non-bracketed paste newlines to `None`, `LineFeed`, `CarriageReturn`, or
  `CarriageReturnAndLineFeed`, while bracketed paste sends the original text
  inside bracketed-paste markers.
- `rssh-app` writes native dropped-file paths into the active pane using
  WezTerm-style `quote_dropped_files` modes: `None`, `SpacesOnly`, `Posix`,
  `Windows`, and `WindowsAlwaysQuoted`. Defaults are `Windows` on Windows and
  `SpacesOnly` on other platforms.
- `rssh-app` exposes WezTerm-style `PasteFrom('PrimarySelection')` through the
  command palette routing layer and native action payloads, and classifies
  `Ctrl+Insert`/`Shift+Insert` the same way as WezTerm's PrimarySelection
  defaults. Unmodified middle-click now routes through the same PrimarySelection
  paste path as WezTerm's default mouse assignment.
- `rssh-app` also accepts deprecated native WezTerm aliases `Copy`, `Paste`,
  and `PastePrimarySelection` for compatibility with older action payloads,
  routing them to `CopyTo('Clipboard')`, `PasteFrom('Clipboard')`, and
  `PasteFrom('PrimarySelection')` respectively. Action-name `copy`, `paste`,
  and `pasteprimaryselection` queries dispatch those aliases directly.
- `rssh-app` exposes WezTerm-style `ResetTerminal` through the command palette,
  injecting RIS (`ESC c`) on the active pane output side.
- `rssh-app` exposes WezTerm-style scrollback navigation through the command
  palette: Scroll To Top, Scroll To Bottom, Scroll By Page Up/Down, and Scroll
  By Line Up/Down, plus native `ScrollByPage(amount)` and
  `ScrollByLine(amount)` payloads whose signed values follow WezTerm's
  up/down direction. Action-name `scrolltotop`, `scrolltobottom`,
  `scrollpageup`, `scrollpagedown`, `scrolllineup`, and `scrolllinedown`
  queries dispatch the corresponding no-argument commands. Action-name
  `scrollbycurrenteventwheeldelta` dispatches the native current wheel-delta
  payload. The command-palette queries `scroll by page <amount>` /
  `scrollbypage <amount>` and `scroll by line <amount>` /
  `scrollbyline <amount>` plus WezTerm-style
  `wezterm.action.ScrollByPage(<amount>)` and
  `wezterm.action.ScrollByLine(<amount>)` function-call queries dispatch those
  signed native payloads directly.
  Native `ScrollByCurrentEventWheelDelta` uses the current vertical mouse-wheel
  event delta when one is active and otherwise no-ops. The default
  `Shift+PageUp` and `Shift+PageDown` shortcuts expose WezTerm-style
  `ScrollByPage(-1)` and `ScrollByPage(1)` in native `KEY_ASSIGNMENTS` and
  route to the same page-wise scrollback movement while leaving unmodified
  PageUp/PageDown available to the active PTY application.
- Native `scroll_to_bottom_on_input` defaults to true and resets the active
  scrollback viewport to the bottom when terminal input is written; setting it
  false preserves the current scrollback viewport on input.
- Native `alternate_buffer_wheel_scroll_speed` defaults to WezTerm's `3`; in
  the alternate screen with mouse reporting disabled, vertical wheel input
  writes repeated Up/Down arrow-key sequences to the PTY instead of moving
  scrollback.
- `rssh-terminal` records OSC 133 `A`/`N`/`P` prompt rows, and `rssh-app`
  exposes WezTerm-style Scroll To Prompt Previous and Scroll To Prompt Next
  through the command palette plus native `ScrollToPrompt(amount)` payloads for
  the active pane. The command-palette queries `scroll to prompt <amount>` and
  `scrolltoprompt <amount>` plus WezTerm-style
  `wezterm.action.ScrollToPrompt(<amount>)` function-call queries dispatch
  arbitrary signed prompt offsets, while action-name `scrolltopreviousprompt`
  and `scrolltonextprompt` queries
  dispatch the adjacent prompt commands.
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
  for active and inactive panes, falling back to local session process tree cwd
  when the PTY backend exposes a pid and preferring child processes over the
  session root, so new tabs/splits inherit the cwd, and decodes `file://` cwd
  URIs before spawning local PTYs.
- `rssh-terminal` base64-decodes iTerm2/WezTerm `OSC 1337;SetUserVar`
  metadata into terminal user vars. `rssh-app` syncs those user vars into
  per-pane app-shell metadata for active and inactive panes and dispatches a
  typed native-window user-var change hook with the window id, pane id, name,
  and value when a stored pane value changes.
- `rssh-terminal` base64-decodes iTerm2 `OSC 1337;SetBadgeFormat` metadata into
  terminal badge format state. `rssh-app` syncs that badge metadata per pane for
  active and inactive panes, interpolates `\(user.NAME)` badge variables from
  pane user vars, `\(iterm2.pid)` from the current app process id,
  `\(iterm2.localhostName)` from the local host name,
  `\(iterm2.effectiveTheme)`, `\(tab.iterm2.effectiveTheme)`,
  `\(tab.window.iterm2.effectiveTheme)`, and
  `\(tab.window.currentTab.iterm2.effectiveTheme)` as `dark` for the current
  fixed dark native UI,
  `\(tab.window.id)` from the native window id,
  `\(tab.window.number)` from the native window number,
  `\(tab.window.frame)` from the latest native window origin and pixel size as
  `[x, y, width, height]`,
  `\(tab.window.style)` from current normal/full-screen window style,
  `\(tab.window.isHotkeyWindow)` as `false` until native hotkey windows exist,
  `\(tab.window.titleOverrideFormat)`/`\(tab.window.titleOverride)` from the
  current base window title,
  `\(tab.window.currentTab.id)`/`\(tab.window.currentTab.title)`/
  `\(tab.window.currentTab.titleOverrideFormat)`/
  `\(tab.window.currentTab.titleOverride)` from the active tab
  id/title/explicit tab title,
  `\(tab.window.currentTab.currentSession.id)`/
  `\(tab.window.currentTab.currentSession.pid)`/
  `\(tab.window.currentTab.currentSession.jobPid)`/
  `\(tab.window.currentTab.currentSession.tty)`/
  `\(tab.window.currentTab.currentSession.autoName)`/
  `\(tab.window.currentTab.currentSession.autoNameFormat)`/
  `\(tab.window.currentTab.currentSession.name)`/
  `\(tab.window.currentTab.currentSession.presentationName)`/
  `\(tab.window.currentTab.currentSession.jobName)`/
  `\(tab.window.currentTab.currentSession.processTitle)`/
  `\(tab.window.currentTab.currentSession.commandLine)`/
  `\(tab.window.currentTab.currentSession.lastCommand)`/
  `\(tab.window.currentTab.currentSession.homeDirectory)`/
  `\(tab.window.currentTab.currentSession.sshIntegrationLevel)`/
  `\(tab.window.currentTab.currentSession.username)`/
  `\(tab.window.currentTab.currentSession.hostname)`/
  `\(tab.window.currentTab.currentSession.shell)`/
  `\(tab.window.currentTab.currentSession.uname)`/
  `\(tab.window.currentTab.currentSession.path)`/
  `\(tab.window.currentTab.currentSession.profileName)`/
  `\(tab.window.currentTab.currentSession.terminalIconName)`/
  `\(tab.window.currentTab.currentSession.terminalWindowName)`/
  `\(tab.window.currentTab.currentSession.applicationKeypad)`/
  `\(tab.window.currentTab.currentSession.bellCount)`/
  `\(tab.window.currentTab.currentSession.mouseReportingMode)`/
  `\(tab.window.currentTab.currentSession.mouseInfo)`/
  `\(tab.window.currentTab.currentSession.mouseInfo[0/1/2/3/4/5/6])`/
  `\(tab.window.currentTab.currentSession.columns)`/
  `\(tab.window.currentTab.currentSession.rows)`/
  `\(tab.window.currentTab.currentSession.selection)`/
  `\(tab.window.currentTab.currentSession.selectionLength)` from the active tab
  current pane id/process id/PTY name/title/auto-name from OSC 1 icon title or
  profile name/launch program/command line/last OSC 133 shell-integration command/local home directory/SSH-integration level/local user/host/shell/uname/path/profile
  name/OSC 1 icon title/OSC 2 window title/application-keypad state/bell count/mouse
  reporting mode/latest mouse-info array and indexed values/size/selection,
  `\(tab.id)`/`\(tab.title)`/`\(tab.titleOverrideFormat)`/
  `\(tab.titleOverride)` from the active tab id/title/explicit tab title,
  `\(tab.currentSession.id)` from the active tab current pane id,
  `\(tab.currentSession.pid)`/`\(tab.currentSession.jobPid)`/
  `\(tab.currentSession.tty)` from the active tab current pane process id and PTY
  name,
  `\(tab.currentSession.autoName)`/`\(tab.currentSession.autoNameFormat)`/
  `\(tab.currentSession.name)`/`\(tab.currentSession.presentationName)` from the
  active tab current pane, with auto-name values using the current OSC 1 icon
  title or profile name and name/presentation-name values using the pane
  title/session name,
  `\(tab.currentSession.jobName)`/`\(tab.currentSession.processTitle)`/
  `\(tab.currentSession.commandLine)` from the active tab current pane launch
  program and command line, `\(tab.currentSession.lastCommand)` from the active
  tab current pane's most recent OSC 133 shell-integration input command,
  `\(tab.currentSession.homeDirectory)`/`\(tab.currentSession.sshIntegrationLevel)`/
  `\(tab.currentSession.username)`/`\(tab.currentSession.hostname)`/
  `\(tab.currentSession.shell)`/`\(tab.currentSession.uname)` from the local host
  home directory, native/local SSH-integration level `0`, local user name, local
  host name, local shell, and local OS/architecture description,
  `\(tab.currentSession.path)` from the active tab current working directory,
  `\(tab.currentSession.profileName)` from the active tab current pane profile name,
  `\(tab.currentSession.terminalIconName)` from the active tab current OSC 1 icon title,
  `\(tab.currentSession.terminalWindowName)` from the active tab current OSC 2 window title,
  `\(tab.currentSession.applicationKeypad)`/`\(tab.currentSession.bellCount)`/
  `\(tab.currentSession.mouseReportingMode)`/
  `\(tab.currentSession.mouseInfo)`/
  `\(tab.currentSession.mouseInfo[0/1/2/3/4/5/6])`/
  `\(tab.currentSession.columns)`/
  `\(tab.currentSession.rows)`/`\(tab.currentSession.selection)`/
  `\(tab.currentSession.selectionLength)` from the active tab current pane
  keypad state, retained BEL count, iTerm2-compatible mouse reporting mode,
  latest reported mouse-info array plus x/y/button/click-count/modifier-array/
  side-effects/event-type indices using iTerm2's up/down/drag event-type values
  `0`/`1`/`2`, modifier values Control/Option/Command/Shift as `1`/`2`/`3`/`4`,
  and the drag side-effect bit, rendered pane size, and active selection text/UTF-8 byte length,
  `\(session.id)` from the app-shell pane id,
  `\(session.termid)` from the current window/tab/pane identifiers, with the
  same value injected into spawned PTY children as `TERM_SESSION_ID`,
  `\(session.pid)`/`\(session.jobPid)` from the live PTY child process id when available,
  `\(session.tty)` from the PTY name when the backend exposes one,
  `\(session.autoName)`/`\(session.autoNameFormat)` from the current OSC 1 icon
  title or loaded profile name, `\(session.name)`/`\(session.presentationName)`
  from the pane title/session name,
  `\(session.jobName)` from the pane launch program,
  `\(session.processTitle)` from the pane launch program,
  `\(session.commandLine)` from the pane launch program plus args,
  `\(session.lastCommand)` from the pane's most recent OSC 133
  shell-integration input command,
  `\(session.homeDirectory)` from the local host home directory,
  `\(session.profileName)` from the loaded TOML profile name exported as
  `RSSH_PROFILE` when present,
  `\(session.sshIntegrationLevel)` as `0` for native/local sessions,
  `\(session.username)` from the local host user name,
  `\(session.hostname)` from the local host name,
  `\(session.shell)` from the local host shell,
  `\(session.uname)` from the local host OS/architecture description,
  `\(session.path)` from the pane current working directory,
  `\(session.terminalIconName)` from OSC 1 icon title, and
  `\(session.terminalWindowName)` from OSC 2 window title, then renders non-empty
  badge text as a pane-local top-right overlay. `\(session.columns)` and
  `\(session.rows)` interpolate from current pane dimensions,
  `\(session.applicationKeypad)` interpolates the current pane application
  keypad boolean as `true` or `false`,
  `\(session.bellCount)` interpolates the retained per-pane BEL count,
  `\(session.mouseReportingMode)` interpolates the iTerm2-compatible
  `-1`/`0`/`2`/`3` reporting value,
  `\(session.mouseInfo)` and `\(session.mouseInfo[0/1/2/3/4/5/6])` interpolate
  the latest app-reported mouse-info array and its x/y/button/click-count/
  modifier-array/side-effects/event-type indices for the pane, with modifier
  arrays rendered as ordered numeric arrays such as `[2, 4]`, event-type values
  `0`/`1`/`2` for up/down/drag, and the drag side-effect bit included when
  applicable, and
  `\(session.selection)`/`\(session.selectionLength)` interpolate from active
  selection text and its UTF-8 byte length. Undefined badge variables evaluate
  to empty strings.
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
  shift direct and stored placements relative to the placement cell. Placements
  that specify only `c` or only `r` derive the other cell axis from the source
  image or source-rectangle aspect ratio. Basic `a=q` support queries return
  `OK`/`EINVAL` for supported direct, regular-file, and temporary-file payloads
  without storing/displaying the queried image, stored placements return `OK`
  or `ENOENT` for present/missing image ids or image numbers, stored-image
  existence queries return `OK`/`ENOENT`, Kitty `q=1`/`q=2` response
  suppression is honored, `i`/`I` mutual exclusion is enforced, direct/stored
  placements advance the cursor by the placement cell rectangle unless `C=1`
  suppresses movement, and placement ids are tracked so
  repeated
  `(image id, placement id)` pairs replace old placements. Relative placement
  parents can also be `U=1` virtual placements, with the parent origin derived
  from the minimum row/column of matching Unicode placeholder renders before
  applying `H`/`V`. Basic `a=d` deletion
  covers all live viewport visible Kitty placements while retaining scrollback
  placements, image-id placement deletion, image-number placement deletion,
  image-id range deletion, `(image id, placement id)` pair deletion, and cursor-cell,
  explicit-cell, visible-column, visible-row, z-index, and cell-plus-z-index
  deletion. Position-oriented deletes leave Unicode-placeholder-derived renders
  intact until the underlying placeholder cell is overwritten or erased. The
  renderer applies Kitty z-index layer ordering, drawing negative
  z-index images below text and non-negative z-index images above text in
  ascending z order, with Kitty image id breaking ties for overlapping same-z
  images. Terminal erase display paths remove affected inline-image
  placements for `CSI 2J`, drop scrollback inline images for `CSI 3J`, and
  rebase retained visible image rows after scrollback clearing. Alternate-screen
  `?1049` switches isolate inline-image placements between main and alternate
  buffers, restoring main placements on exit and discarding alternate placements.
  Scroll operations move inline-image placements with affected text rows and
  drop placements that leave the scrolled region. Basic Sixel DCS `q` payloads
  with RGB/HLS palette definitions, color selection, DCS `P1` macro pixel
  aspect, DECGRA `Pan`/`Pad` aspect override plus `Ph`/`Pv` minimum background
  dimensions, DCS `P2` transparent/opaque background mode, repeat introducers,
  carriage returns, and sixel newlines are normalized into raw RGBA inline
  images. Default and `?80l` output starts at the text cursor and advances the
  cursor below the image while preserving the left-edge column; `?8452h` moves
  that post-Sixel cursor to the right edge. DECSDM `?80h` output starts at the
  active graphics-page origin and preserves the text cursor. WezTerm's
  tmux-control `DCS 1000 q` is ignored instead of being treated as Sixel.
  Native window redraws advance elapsed-time GIF frames through the renderer
  animation clock. Kitty shared-memory transfers, remaining richer placement
  controls, broader query responses beyond current direct/local-file payload
  validation and stored-image existence checks, full Sixel protocol coverage,
  sixel scrolling/pan edge cases, and pane sync remain later parity work.
- `rssh-app` extracts OSC 52 and iTerm2 `OSC 1337;Copy=;base64` clipboard writes
  from ESC plus UTF-8 C1 OSC/ST active and inactive pane output, retaining
  legacy raw C1 compatibility and reusing the existing OSC52 write policy and
  clipboard writer path. The default OSC52 policy is WezTerm-style write-only;
  read queries require explicit `--osc52 read-write`.
- `rssh-app` extracts WezTerm-documented OSC 9 notification text and OSC 777
  `notify` title/body events from ESC plus UTF-8 C1 OSC/ST active and inactive
  pane output, retaining legacy raw C1 compatibility and dispatching them
  through the native-window notification handler. Native per-window
  `notification_handling` defaults to `AlwaysShow` and supports `NeverShow`,
  `SuppressFromFocusedPane`, `SuppressFromFocusedTab`, and
  `SuppressFromFocusedWindow` before handler dispatch and latest-notification
  title updates. The native window title also shows the latest notification as
  a `Notification: ...` status suffix. The native OS toast backend remains a
  later platform-adapter task.
- `rssh-app` records WezTerm-documented ConEmu-style `OSC 9;4;st;pr` progress
  state as None, percentage, error, or indeterminate from ESC plus UTF-8 C1
  OSC/ST forms, does not treat progress reports as OSC 9 notifications, and
  syncs active/inactive pane progress into app-shell pane metadata. The native
  tab bar shows active-pane progress as `N%`, `err:N%`, or `~`; Lua pane API
  exposure and configurable status formatting remain later parity work.
- `rssh-app` counts ASCII BEL events from active and inactive pane output and
  dispatches them through a typed native-window bell hook with the window id and
  originating pane id. Native per-window `audible_bell` overrides support
  `SystemBeep` and `Disabled`; disabling the audible bell suppresses only the
  system-beep path. Native per-window `visual_bell` overrides support
  WezTerm's zero-duration default no-op plus `BackgroundColor` pane flashes and
  `CursorColor` cursor-color flashes derived from the active rendered foreground
  color with a default text-foreground fallback, with native
  `visual_bell_color` overrides standing in for WezTerm `colors.visual_bell`;
  background flashes include blank cells and blend over existing
  background/cursor colors across the configured fade-in/fade-out durations
  using the native Constant/Linear/Ease/EaseIn/EaseOut/EaseInOut/CubicBezier
  easing subset; `CursorColor` fades return to the current rendered cursor
  color, including `force_reverse_video_cursor` cursor-cell foreground
  behavior. Static WezTerm-style `config.visual_bell` snippets parse inline or
  through top-level static table variables, and
  `config.colors.foreground`, `config.colors.background`,
  `config.colors.ansi`, `config.colors.brights`, `config.colors.indexed`,
  `config.colors.selection_fg`, `config.colors.selection_bg`,
  `config.colors.cursor_bg`, `config.colors.cursor_border`,
  `config.colors.cursor_fg`, and
  `config.colors.visual_bell` snippets parse into the same native override path,
  including bracketed string table keys with long-bracket values for
  visual-bell fields, nested `CubicBezier` easing tables, and
  `colors.visual_bell`; native `foreground_color`, `background_color`,
  `ansi_palette`, `indexed_palette`, `selection_fg_color`,
  `selection_bg_color`, `cursor_bg_color`, `cursor_border_color`, and
  `cursor_fg_color` overrides stand in for WezTerm `colors.foreground`,
  `colors.background`, `colors.ansi`, `colors.brights`, `colors.indexed`,
  `colors.selection_fg`, `colors.selection_bg`, `colors.cursor_bg`,
  `colors.cursor_border`, and `colors.cursor_fg`; static `config.color_schemes`
  entries can define custom in-file schemes inline or through static top-level
  Lua table variables, and static `config.color_schemes['Name'] = scheme` or
  `config.color_schemes.Name = scheme` assignments can append or replace named
  schemes after initialization. Selected custom scheme entries also support
  static top-level field mutations such as
  `config.color_schemes['Name'].background = '#101010'` and bracket-key
  variants such as `config.color_schemes['Name']['cursor_bg'] = '#101010'`,
  indexed slot mutations such as
  `config.color_schemes['Name'].indexed[136] = '#101010'`, plus ANSI/bright
  slot mutations such as
  `config.color_schemes['Name'].ansi[2] = '#101010'` and tab-bar nested
  mutations such as
  `config.color_schemes['Name'].tab_bar.active_tab.bg_color = '#101010'`.
  Mutations are applied after the final selected static scheme definition, so
  later full `config.color_schemes['Name'] = { ... }` assignments replace
  earlier entry mutations. Returned static config variables such as `return cfg`
  also carry `cfg.color_schemes['Name']` entry assignments and selected-scheme
  mutations while unreturned `config.color_schemes` assignments are ignored.
  `config.color_scheme` selects one before `config.colors` applies overriding
  fields, and static `config.color_scheme_dirs` lists, inline or through top-level
  static table variables, are retained in effective config and scan
  configured directories for matching TOML scheme files. External TOML schemes
  load when `[metadata].name` or the file stem matches `config.color_scheme`
  and reuse the same implemented color fields before `config.colors` applies
  overriding fields. Static `wezterm.color.load_scheme('path')` calls with a
  constant TOML path can also feed selected `config.color_schemes['Name']`
  entries directly or through static variables whose supported static mutations
  are applied, or `config.colors` directly or through the first returned variable from
  `local colors, metadata = ...` or
  `colors, metadata = ...` assignments. Static `load_scheme` variable
  references resolve to the latest binding before the `config.colors`
  assignment and ignore later rebinding, including top-level static mutations such as
  `colors.background = '#101010'` and bracket-key variants such as
  `colors['background'] = '#101010'`, indexed slot mutations such as
  `colors.indexed[136] = '#101010'`, ANSI/bright slot mutations such as
  `colors.ansi[2] = '#101010'`, tab-bar nested mutations such as
  `colors.tab_bar.active_tab.bg_color = '#101010'`, or multiline table mutations such as
  `colors.ansi = { ... }` before assignment. When complete `config.colors`
  table assignments and load-scheme-backed `config.colors = colors`
  assignments both appear, the static parser chooses the later source before
  applying supported mutations; a top-level `return { colors = ... }` table or
  returned static config variable such as `return cfg` is treated as the
  returned config and wins over earlier unreturned `config.colors`
  assignments. When no in-file or configured-dir
  scheme matches, the default WezTerm custom scheme directories are also
  searched: `$HOME/.config/wezterm/colors` on POSIX and `colors` next to the
  executable on Windows. These color overrides drive the default text
  foreground, framebuffer background, ANSI 0-15 palette, indexed 16-255 palette
  overrides, selected text foreground/background, cursor fill, block-cursor
  border, line-cursor color, and block-cursor text foreground for full and
  damage renders. Built-in scheme lookup, richer dynamic `load_scheme`
  composition, and Lua event wiring remain later parity work.
- `rssh-app` preserves CSI focus-reporting writes on window focus changes and
  dispatches a typed native-window focus-change hook with the window id, active
  pane id, and focused/unfocused state. Lua event wiring remains later parity
  work.
- `rssh-app` dispatches a typed native-window resize hook after successful
  terminal/runtime resize and fullscreen/windowed transitions, carrying the
  window id, active pane id, pixel size, terminal rows/columns, and
  `is_full_screen` state so native handlers receive the fullscreen dimension
  metadata exposed by WezTerm's window dimensions APIs. Lua event wiring
  remains later parity work.
- `rssh-app` dispatches a typed native-window `window-config-reloaded` hook for
  command-palette `ReloadConfiguration` and the default `Ctrl+Shift+R`
  shortcut, carrying the window id and active pane id. A typed native
  `set_config_overrides`/`get_config_overrides` subset stores
  per-window overrides for `dpi`, `tab_max_width`, `status_update_interval`,
  `max_fps`, `animation_fps`, `cursor_blink_rate`, `cursor_blink_ease_in`, `cursor_blink_ease_out`,
  `text_blink_rate`, `text_blink_rate_rapid`, `text_blink_ease_in`,
  `text_blink_ease_out`, `text_blink_rapid_ease_in`,
  `text_blink_rapid_ease_out`,
  `font_size`, `cell_width`, `cell_widths`, `line_height`, `font_antialias`,
  `font_hinting`, `font_rasterizer`, `font_shaper`, `font_dirs`,
  `font_locator`, `custom_block_glyphs`,
  `anti_alias_custom_block_glyphs`, `allow_square_glyphs_to_overflow_width`,
  `freetype_load_target`, `freetype_render_target`, `freetype_load_flags`,
  `freetype_interpreter_version`, `freetype_pcf_long_family_names`,
  `display_pixel_geometry`, `dpi`, `bold_brightens_ansi_colors`, `default_cursor_style`,
  `cursor_thickness`, `underline_thickness`, `underline_position`, `strikethrough_position`, `force_reverse_video_cursor`,
  `initial_cols`, `initial_rows`, `adjust_window_size_when_changing_font_size`,
  `command_palette_rows`, `quick_select_alphabet`, `quick_select_patterns`,
  `disable_default_quick_select_patterns`, `quick_select_remove_styling`, `hyperlink_rules`, `selection_word_boundary`, `term`, `audible_bell`, `visual_bell`, `color_scheme_dirs`, `foreground_color`, `background_color`, `ansi_palette`, `indexed_palette`, `selection_fg_color`, `selection_bg_color`, `cursor_bg_color`, `cursor_border_color`, `cursor_fg_color`, `visual_bell_color`, `notification_handling`, `default_prog`,
  `default_domain`, `default_workspace`, `prefer_to_spawn_tabs`, `automatically_reload_config`, `check_for_updates`, `check_for_updates_interval_seconds`, `show_update_window`, `use_resize_increments`, `debug_key_events`, `log_unknown_escape_sequences`, `warn_about_missing_glyphs`, `default_cwd`, `set_environment_variables`, `key_map_preference`,
  `swap_backspace_and_delete`, `enable_csi_u_key_encoding`,
  `enable_kitty_keyboard`, `allow_win32_input_mode`,
  `scroll_to_bottom_on_input`,
  `alternate_buffer_wheel_scroll_speed`,
  `canonicalize_pasted_newlines`,
  `quote_dropped_files`,
  `disable_default_key_bindings`,
  `disable_default_mouse_bindings`,
  `hide_mouse_cursor_when_typing`,
  `pane_focus_follows_mouse`,
  `swallow_mouse_click_on_pane_focus`,
  `swallow_mouse_click_on_window_focus`,
  `bypass_mouse_reporting_modifiers`,
  `enable_scroll_bar`, `min_scroll_bar_height`,
  `enable_tab_bar`, `hide_tab_bar_if_only_one_tab`, `unzoom_on_switch_pane`,
  `tab_bar_at_bottom`,
  `tab_and_split_indices_are_zero_based`, `mouse_wheel_scrolls_tabs`,
  `switch_to_last_active_tab_when_closing_tab`,
  `quit_when_all_windows_are_closed`,
  `window_close_confirmation`,
  `exit_behavior`,
  `clean_exit_codes`,
  `exit_behavior_messaging`,
  `skip_close_confirmation_for_processes_named`,
  `show_close_tab_button_in_tabs`,
  `show_new_tab_button_in_tab_bar`, `show_tab_index_in_tab_bar`, and
  `show_tabs_in_tab_bar`, updates effective config snapshots, and
  emits `window-config-reloaded` on every set. `automatically_reload_config`,
  `check_for_updates`, `check_for_updates_interval_seconds`,
  `show_update_window`, `use_resize_increments`, `debug_key_events`, and
  `log_unknown_escape_sequences` are retained in effective config snapshots.
  `warn_about_missing_glyphs` is retained with WezTerm's default `true`.
  Unknown ESC/CSI sequences are recorded by the terminal runtime and emitted
  as native stderr warnings when `log_unknown_escape_sequences` is enabled.
  Native key events are emitted as stderr `INFO key_event` diagnostics when
  `debug_key_events` is enabled. Missing glyph codepoints detected in rendered
  cells are emitted once per native window as stderr `CONFIG ERROR missing
  glyph ...` diagnostics when `warn_about_missing_glyphs` is enabled. Lua event
  wiring, full WezTerm-style configuration error window UI, actual OS-specific
  resize increment application, actual Lua config reload, automatic file
  watching, Lua `window:set_config_overrides` wiring, and broader config option
  coverage remain later parity work.
- `rssh-app` dispatches a typed native `augment-command-palette` hook when the
  command palette opens, carrying the window id and active pane id. Returned
  entries add native `WindowCommand` actions to the fuzzy-filtered palette list,
  and optional entry `doc` text plus known Nerd Font `icon` names, including
  `md_rename_box`, `fa_clock_o`, and `cod_github`, are rendered beside the brief
  label. Executed command labels update in-memory and persisted
  JSON frecency so empty queries and equal-score fuzzy matches can prefer
  higher-use and more-recent entries across app instances. Lua event wiring,
  arbitrary Lua callbacks, full Nerd Font icon catalog coverage, and full
  action-value parity remain later work.
- `rssh-app` dispatches a typed native-window `format-tab-title` hook when
  rendering tab bar labels. The event carries the computed default title, tab
  id, active pane id, tab index, tab count, active-tab pane count, and active
  plus last-active state, along with tab-bar hover state and `max_width`, using
  WezTerm-style two-pass dispatch: first with `hover=false` and the
  WezTerm-default 16-cell `tab_max_width`, then with the computed hover state
  and an available-space title width; returning a
  string overrides the displayed title, native Text/Foreground/Background format items can style the
  title segment, Text items consume embedded SGR presentation escapes including
  blink/inverse/conceal/strikethrough/overline while layout uses only their
  visible text, ResetAttributes restores the tab segment style, and `None`
  keeps the default.
  Native Intensity Normal/Bold/Half toggles tab-title bold/faint rendering,
  native Italic true/false toggles tab-title italic rendering, and native
  Underline None/Single/Double/Curly/Dotted/Dashed maps to tab-title underline
  style. The typed event also carries TabInformation/PaneInformation-style
  snapshots with window id/title, all tabs in the window, explicit tab title,
  the current tab's active pane and pane entries, plus the active tab's pane
  entries for the top-level `panes` parameter. Pane snapshots include geometry,
  titles, foreground process name, current working directory, unseen-output
  state, local domain name, tty name when known, user vars, and progress. The
  typed event also carries an effective config snapshot for implemented window
  options including `dpi`, `tab_max_width`, `status_update_interval`,
  `max_fps`, `animation_fps`, `cursor_blink_rate`, `cursor_blink_ease_in`, `cursor_blink_ease_out`,
  `text_blink_rate`, `text_blink_rate_rapid`, `text_blink_ease_in`,
  `text_blink_ease_out`, `text_blink_rapid_ease_in`,
  `text_blink_rapid_ease_out`,
  `font_size`, `cell_width`, `cell_widths`, `line_height`, `font_antialias`,
  `font_hinting`, `font_rasterizer`, `font_shaper`, `font_dirs`,
  `font_locator`, `custom_block_glyphs`,
  `anti_alias_custom_block_glyphs`, `allow_square_glyphs_to_overflow_width`,
  `freetype_load_target`, `freetype_render_target`, `freetype_load_flags`,
  `freetype_interpreter_version`, `freetype_pcf_long_family_names`,
  `display_pixel_geometry`, `dpi`, `bold_brightens_ansi_colors`, `default_cursor_style`,
  `cursor_thickness`, `underline_thickness`, `underline_position`, `strikethrough_position`, `force_reverse_video_cursor`,
  `adjust_window_size_when_changing_font_size`, `command_palette_rows`,
  `quick_select_alphabet`, `quick_select_patterns`,
  `disable_default_quick_select_patterns`, `quick_select_remove_styling`, `hyperlink_rules`, `selection_word_boundary`, `term`, `audible_bell`, `visual_bell`, `color_scheme_dirs`, `foreground_color`, `background_color`, `ansi_palette`, `indexed_palette`, `selection_fg_color`, `selection_bg_color`, `cursor_bg_color`, `cursor_border_color`, `cursor_fg_color`, `visual_bell_color`, `notification_handling`, `default_prog`,
  `default_domain`, `default_workspace`, `prefer_to_spawn_tabs`, `automatically_reload_config`, `check_for_updates`, `check_for_updates_interval_seconds`, `show_update_window`, `use_resize_increments`, `debug_key_events`, `log_unknown_escape_sequences`, `warn_about_missing_glyphs`, `default_cwd`, `set_environment_variables`, `key_map_preference`,
  `swap_backspace_and_delete`, `enable_csi_u_key_encoding`,
  `enable_kitty_keyboard`, `allow_win32_input_mode`,
  `scroll_to_bottom_on_input`,
  `alternate_buffer_wheel_scroll_speed`,
  `canonicalize_pasted_newlines`,
  `quote_dropped_files`,
  `disable_default_key_bindings`,
  `disable_default_mouse_bindings`,
  `hide_mouse_cursor_when_typing`,
  `pane_focus_follows_mouse`,
  `swallow_mouse_click_on_pane_focus`,
  `swallow_mouse_click_on_window_focus`,
  `bypass_mouse_reporting_modifiers`,
  `enable_scroll_bar`, `min_scroll_bar_height`,
  `enable_tab_bar`, `hide_tab_bar_if_only_one_tab`, `unzoom_on_switch_pane`,
  `tab_bar_at_bottom`,
  `tab_and_split_indices_are_zero_based`, `mouse_wheel_scrolls_tabs`,
  `switch_to_last_active_tab_when_closing_tab`,
  `quit_when_all_windows_are_closed`,
  `window_close_confirmation`,
  `exit_behavior`,
  `clean_exit_codes`,
  `exit_behavior_messaging`,
  `skip_close_confirmation_for_processes_named`,
  `show_close_tab_button_in_tabs`,
  `show_new_tab_button_in_tab_bar`, `show_tab_index_in_tab_bar`, and
  `show_tabs_in_tab_bar`. Static
  `wezterm.on('format-tab-title', function(...) return ... end)` string and
  inline, callback-local, or top-level static FormatItem table returns map onto
  the same tab-title override path; arbitrary Lua callbacks and the full Lua
  config object remain later parity work.
- `rssh-app` dispatches a typed native-window `format-window-title` hook after
  computing the default title. The event carries the default title, active tab
  id, active pane id, tab count, active-tab pane count, and the active
  key-table stack top plus
  TabInformation/PaneInformation-style snapshots for the active tab, active
  pane, all tabs in the window, and panes in the active tab; returning a string
  overrides the native title, while `None` keeps the default. The typed event
  carries the same effective config snapshot. Static
  `wezterm.on('format-window-title', function(...) return 'title' end)` string
  returns map onto the same title override path; arbitrary Lua callbacks and
  the full Lua config object remain later parity work.
- `rssh-app` dispatches typed native-window `update-status` and deprecated
  `update-right-status` hooks from the native event loop with the window id and
  active pane id, scheduled by a WezTerm-style 1000ms
  `status_update_interval` default. The
  handlers can update stored left and right status strings; the tab bar renders
  left status after the workspace label, consumes SGR presentation escapes
  including blink/inverse/conceal/strikethrough/overline plus WezTerm underline
  style variants and ANSI/indexed/RGB
  foreground/background/underline color escapes in status strings, computes
  status layout from visible text, and right-aligns right status at the window
  edge, clipping over-wide right status from the left. Native
  `set_left_status` and `set_right_status` methods update the same tab-bar
  state directly. Lua-configurable `status_update_interval` plus static
  `wezterm.on('update-status', ...)` and deprecated `update-right-status`
  literal `window:set_left_status(...)` / `set_right_status(...)` setters map
  into the same status state, including inline or static-table-variable
  `wezterm.format` Text/Foreground/Background/ResetAttributes and Attribute
  Intensity/Italic/Underline item composition with static item tables resolved
  from callback-local or top-level scope plus callback-local `table.insert` or
  `items[#items + 1] = ...` appends whose string items can resolve from static
  variables. Arbitrary Lua callbacks and dynamic `wezterm.format` construction
  remain later parity work.
- `rssh-app` dispatches a typed native-window `new-tab-button-click` hook for
  Left/Right/Middle clicks on the tab bar `+` button, carrying the window id
  and active pane id. Left click carries the default `NewTab` action in the
  event payload, while Right/Middle clicks have no default action; returning
  `false` suppresses any default action. Lua event wiring remains later parity
  work.
- `rssh-app` dispatches a typed native-window open-uri hook for ctrl-clicked OSC
  8 hyperlinks before invoking the default opener, carrying the window id,
  active pane id, and URI. Returning `false` suppresses the default opener. The
  command palette exposes WezTerm-style `CompleteSelection`,
  `OpenLinkAtMouseCursor`, and `CompleteSelectionOrOpenLinkAtMouseCursor`:
  active mouse selections copy to ClipboardAndPrimarySelection; otherwise the
  OSC 8 link under the mouse opens through the same open-uri hook. Structured
  `completeselection`, `openlinkatmousecursor`, and
  `completeselectionoropenlinkatmousecursor` action-name queries resolve to the
  same native behavior. Native
  `CompleteSelectionTo(destination)` and
  `CompleteSelectionOrOpenLinkAtMouseCursorTo(destination)` payloads complete
  active selections into a specific implemented copy destination, and the
  command palette now accepts `complete selection to <destination>`,
  `completeselectionto <destination>`,
  `complete selection open link to <destination>`, and
  `completeselectionoropenlinkatmousecursorto <destination>` for quoted or
  unquoted `Clipboard`, `PrimarySelection`, or `ClipboardAndPrimarySelection`;
  WezTerm-style `wezterm.action.CompleteSelection '<destination>'` and
  `wezterm.action.CompleteSelectionOrOpenLinkAtMouseCursor '<destination>'`
  Lua action queries dispatch the same destination-specific payloads.
  Static WezTerm-style `config.hyperlink_rules` tables parse `regex`,
  `format`, and `highlight` fields; `table.insert(config.hyperlink_rules, ...)`
  appends rules, and the official `config.hyperlink_rules =
  wezterm.default_hyperlink_rules()` seed preserves default rules before
  appended custom rules.
  Lua event wiring remains later parity work.
- `rssh-app` answers WezTerm/iTerm2-compatible `OSC 1337;ReportCellSize`
  queries with the current fixed cell pixel dimensions, alongside the existing
  xterm cell/window size query responses.
- `rssh-app` tracks kitty keyboard progressive-enhancement flags from
  `CSI = flags ; mode u` plus `CSI > flags u` / `CSI < n u` in both native
  runtime and console filtering, with native windows honoring those negotiation
  sequences and `CSI ? u` replies only when `enable_kitty_keyboard` is true.
  Native windows and local console input also honor WezTerm's default-on
  `allow_win32_input_mode` by tracking ConPTY `CSI ? 9001 h/l` and emitting
  Win32 key records ahead of CSI-u/kitty encoding while that mode is active.
  When the kitty disambiguate flag is active, console and native-window input
  encode Ctrl/Alt ASCII character keys as `CSI-u` events while leaving plain
  text input on the legacy path; when the kitty report-all flag is active,
  plain text keys plus Enter/Tab/Backspace are encoded as canonical `CSI-u`
  events, and navigation/editing keys, F1-F12, and F13-F35 use kitty canonical
  functional-key forms under disambiguate/report-all modes. Keypad keys use
  kitty KP_* private-use codepoints when kitty keyboard flags request CSI-u
  reporting, and kitty private-use functional codes cover CapsLock, ScrollLock,
  NumLock, PrintScreen, Pause, and Menu/ContextMenu in console and
  native-window paths, plus media transport, track, record, and volume keys
  where the input backend exposes them. Kitty event-type reporting is supported
  for repeat/release events using `modifier:event` subfields, including
  event-types-only text-key repeat/release, and
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
- Native window overrides expose WezTerm-style `cursor_blink_rate`; `0` keeps
  blinking cursors visible, and non-zero values use `cursor_blink_ease_in` /
  `cursor_blink_ease_out` to interpolate cursor opacity between visible and
  hidden phases. Native window overrides also expose WezTerm-style
  `text_blink_rate`, `text_blink_rate_rapid`, `text_blink_ease_in`,
  `text_blink_ease_out`, `text_blink_rapid_ease_in`, and
  `text_blink_rapid_ease_out`; SGR 5 and SGR 6 text blink use independent
  opacity phases and interpolate foreground/decorations toward the rendered
  background. Native overrides also expose WezTerm-style
  `bold_brightens_ansi_colors`, with `No`, `BrightAndBold`, and `BrightOnly`
  modes applied to bold ANSI 0-7 foreground colors. Native overrides also
  expose WezTerm-style `default_cursor_style` and `cursor_thickness` overrides for underline and bar
  cursor glyphs using px, DPI-scaled pt, percent-of-default, and
  cell-fraction units. Native `underline_thickness` applies the same unit forms
  to terminal text underline decorations. Native `underline_position` applies
  signed px, DPI-scaled pt, percent-of-default, and cell-fraction units to
  terminal text underline placement using the current default underline row as
  a baseline approximation. Native `strikethrough_position` applies px,
  DPI-scaled pt, percent-of-default, and cell-fraction units to terminal text
  strikethrough decorations.
  `force_reverse_video_cursor` forces native cursor fills to use the cursor
  cell's effective foreground color unless OSC 12 set an explicit cursor color,
  and OSC 112 resets that override; `DECSCUSR 0` and full terminal reset
  restore the configured steady/blinking block, underline, or bar default. Lua
  config parsing, exact font-metric-derived baselines/defaults, and
  split/custom-glyph line use remain later parity work.
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
  insert/replace mode, cursor visibility, origin mode, scroll region, G0
  character set, and saved-cursor state without clearing visible cells or
  scrollback; app runtime and console filtering track ESC and C1 CSI forms for
  mode-status replies.
- `rssh-terminal` tracks `DECSCA` protected-cell state and applies it to DEC
  selective display/line erase (`DECSED`/`DECSEL`) while ordinary `ED`/`EL`
  still clears the addressed cell range.
- `rssh-terminal` ignores WezTerm-documented non-printing C0 controls while
  preserving BEL/BS/HT/LF/VT/FF/CR/ESC special handling.
- `rssh-terminal` consumes WezTerm-documented `ESC =`/`ESC >` application
  keypad mode escapes so they do not enter the rendered grid.
- `rssh-terminal` plus app/runtime output filtering consume standalone ST
  controls (`ESC \`, UTF-8 C1 `U+009C`, and legacy raw C1 `0x9C`) as no-effect
  sequences, matching WezTerm's string-terminator handling when ST is seen
  outside an active control string.
- `rssh-terminal` tracks reverse-wrap mode `?45`; when auto-wrap is also
  enabled, BS at the left boundary wraps to the previous row's right boundary,
  and app runtime plus console filtering report `?45` through DECRQM.
- `rssh-terminal` and `rssh-renderer` preserve WezTerm SGR mode 6 RGBA colors
  for foreground, background, and underline color state, and app-shell DECRQSS
  SGR responses serialize those alpha-bearing colors.
- `rssh-terminal` maps WezTerm SGR `6` rapid blink onto the existing blink cell
  attribute, matching SGR `5` visibility behavior and SGR `25` reset.
- `rssh-terminal` and `rssh-renderer` preserve WezTerm SGR 73/74/75
  vertical-align state for superscript, subscript, and baseline, and app-shell
  DECRQSS SGR responses serialize active 73/74 state.
- `rssh-app` answers WezTerm-documented DECRQSS `"` `p` conformance-level and
  `s` left/right-margin queries in both native runtime and console output
  filter paths. The left/right-margin response reports the modeled DECSLRM
  state, and DECRQM reports DECLRMM `?69` for ESC and C1 CSI forms.
- `rssh-app` includes a command-palette `Rename Tab` entry that sets an explicit
  title for the active tab using the current visible tab title as its base, and
  supports quote-aware `rename tab <title>` and action-name `renametab <title>`
  queries for arbitrary user-entered titles.
- `rssh-app` includes command-palette `Rename Workspace` query input through
  quote-aware `rename workspace <name>` and action-name
  `renameworkspace <name>`, naming the active workspace from user-entered text
  without requiring the later Lua `PromptInputLine` event path.
- `rssh-app` defaults to one native-window row for a basic tab bar, renders
  workspace/tab/pane-count state plus tab titles, prefers explicit tab titles,
  falls back to each tab's active-pane terminal title, activates tabs by mouse
  click, and exposes a configurable clickable close marker per tab via
  `show_close_tab_button_in_tabs`. Closing a non-final tab honors
  `switch_to_last_active_tab_when_closing_tab`, and closing the final tab
  requests native-window shutdown through the same close lifecycle path as
  command palette close.
- Native window-manager/decorations close requests honor the WezTerm-style
  `window_close_confirmation` override: `AlwaysPrompt` enters a `Close Window?`
  confirmation overlay before shutdown, while `NeverPrompt` requests shutdown
  immediately. Native `skip_close_confirmation_for_processes_named` uses
  WezTerm's default stateless-process list or a custom override to skip
  close-window, close-tab, and close-pane confirmation when every affected
  pane's known local launch-program basename matches. Static Lua config parsing
  covers `window_close_confirmation` and
  `skip_close_confirmation_for_processes_named` inline or through top-level
  static table variables; child process tree inspection and
  `mux-is-process-stateful` remain later parity work.
- The tab bar can hide tab indices via the native
  `show_tab_index_in_tab_bar` effective-config field and hide tab labels via
  `show_tabs_in_tab_bar`, and can switch tabs with vertical mouse wheel input
  via `mouse_wheel_scrolls_tabs`. It can also be disabled entirely via
  `enable_tab_bar=false` or hidden while only one tab exists via
  `hide_tab_bar_if_only_one_tab=true`, and can render at the bottom via
  `tab_bar_at_bottom=true`. Tab indices can use zero-based labels via
  `tab_and_split_indices_are_zero_based=true`, while leaving status text and the
  `+` button independently controlled when the bar is visible. The clickable
  `+` button creates and activates a new tab through the same app-shell `NewTab`
  action used by keyboard shortcuts and the command palette.
- Split panes can optionally focus on hover when the native
  `pane_focus_follows_mouse` effective-config field is true; the default remains
  click-to-focus.
- Inactive-pane clicks default to WezTerm-style click-through, and
  `swallow_mouse_click_on_pane_focus=true` focuses the pane while consuming that
  initial click.
- A click that refocuses the native window honors
  `swallow_mouse_click_on_window_focus`: when true it is consumed before pane
  handling, and when false it passes through to pane processing. The default
  follows WezTerm's platform rule: true on macOS and false elsewhere.
- When an application enables mouse reporting, holding
  `bypass_mouse_reporting_modifiers` prevents the mouse event from being sent to
  the PTY and routes it through native mouse handling as if those modifiers were
  not pressed; the default is `SHIFT`. Drag user mouse bindings treat that
  bypassed path as `mouse_reporting = false`, and bypassed wheel input uses
  native scroll handling instead of SGR mouse reporting.
- `rssh-app` renders basic right/down pane split layouts by clipping and placing
  each pane snapshot into its app-shell split region with separator cells.
- `rssh-app` maps mouse clicks and wheel input to the pane under the cursor, so
  split panes can be focused and scrolled independently at the native-window
  layer.
- The scrollback scrollbar follows WezTerm's `enable_scroll_bar` default: hidden
  by default and rendered/clickable only when the native effective-config field
  is true. Its thumb minimum follows WezTerm's default
  `min_scroll_bar_height = "0.5cell"` behavior, with native px, DPI-scaled pt,
  cell, and percent units applied to rendering and hit testing; Lua config
  wiring remains later parity work.
- Terminal scrollback retention follows WezTerm's `scrollback_lines` default of
  `3500` lines. Native effective-config overrides update active and inactive
  pane runtimes, carry into newly spawned panes/windows, and immediately prune
  retained history when the configured limit shrinks.
- Alternate-screen wheel fallback follows WezTerm's
  `alternate_buffer_wheel_scroll_speed` default of `3`: when mouse reporting is
  disabled, each vertical wheel tick writes repeated Up/Down arrow-key
  sequences to the active PTY instead of changing scrollback.
- `rssh-app` supports WezTerm-style `AdjustPaneSize` actions through
  `Ctrl+Shift+Alt+Arrow` and command palette Adjust Pane Size entries. Resize
  state is stored in the app-shell split model and applied when rendering split
  regions. The command-palette query
  `adjust pane size <direction> <amount>` plus
  `adjustpanesize <direction> <amount>` covers arbitrary Left/Right/Up/Down
  resize amounts, and structured field forms
  `adjustpanesize direction=<direction> amount=<cells>` dispatch the same
  payload. WezTerm-style
  `wezterm.action.AdjustPaneSize { '<direction>', <cells> }` Lua table action
  queries dispatch the same payload. Native
  `WindowCommand::AdjustPaneSize { direction, amount }` payloads dispatch the
  same active-pane resize path with arbitrary cell amounts.
- `rssh-app` supports mouse drag resizing on rendered split separators, feeding
  drag distance back through the same app-shell resize action path.
- `rssh-app` supports WezTerm-style `TogglePaneZoomState` through
  `Ctrl+Shift+Z` and the command palette. The default `Ctrl+Shift+Z`
  key-assignment entry exposes `TogglePaneZoomState`, while `TogglePaneZoom`
  remains a native compatibility alias. `rssh-core` also exposes
  `SetPaneZoomState` for explicit zoom/unzoom, and native
  `WindowCommand::SetPaneZoomState(bool)` payloads dispatch that same
  idempotent zoom-state path. The command-palette query
  `set pane zoom state true|false` / `set pane zoom state=true|false` plus
  the action-name spelling `setpanezoomstate true|false` /
  `setpanezoomstate=true|false` dispatches those explicit native payloads.
  WezTerm-style `wezterm.action.SetPaneZoomState(true|false)` function-call
  queries dispatch the same path.
  Action-name `togglepanezoomstate`, `togglepanezoom`, `zoompane`, and
  `unzoompane` queries dispatch the corresponding no-argument zoom commands.
  The zoomed pane fills the tab terminal region until zoom is toggled off,
  explicitly unzoomed, or a
  directional pane-switch action unzooms before activating another pane when
  `unzoom_on_switch_pane` is true. Setting it false keeps the zoomed pane active
  and blocks `ActivatePaneDirection`, including command-palette Next/Previous
  pane cycling.
- `rssh-app` now follows WezTerm-style close lifecycle behavior for command
  palette close actions: closing the last pane in a tab closes that tab when a
  neighboring tab exists, and closing the final tab/pane requests native-window
  shutdown from the window manager.
- Native window title surfaces app-shell state as `[workspace:X tab:Y pane:Z]` so
  smoke runs can verify transitions without opening multiple PTY sessions.

## Known Limitations

- Multi-PTY runtime orchestration is still basic and process-local; there is no
  mux server/client or remote domain attachment yet. Local process exit behavior
  covers the native `exit_behavior`, `clean_exit_codes`, and
  `exit_behavior_messaging` subset, including static Lua config parsing for
  those fields.
- The tab bar is basic text UI only; Lua/custom tab title formatting,
  external CLI/mux tab-title control, richer new-tab launcher behavior,
  pane-local scrollbar UI, richer split-drag affordances, and richer focus
  indicators are not yet implemented.
- Pane select Activate, swap, MoveToNewTab, and MoveToNewWindow action paths are
  implemented. MoveToNewWindow can now produce a detached native-window app
  state with the selected pane runtime, and the event loop can materialize it as
  an additional OS window. Platform focus/activation polish is still pending.
- No mux/domain model exists yet beyond action/state support.
- Command palette UX is minimal: richer discovery and configurable Lua bindings
  are still pending. The native `augment-command-palette` hook currently covers
  typed `WindowCommand` entries only.
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
  deletion, source-rectangle cropping, single-axis `c`/`r` aspect-ratio
  derivation, `X`/`Y` target pixel offsets, relative parent lookup for `U=1`
  virtual placeholder bounds, z-index layer ordering, and basic
  Kitty `a=q` plus stored-image query and
  stored-placement `OK`/`ENOENT` response writeback with `q=1`/`q=2`
  suppression, terminal erase display cleanup for retained inline images,
  `?1049` alternate-screen image isolation, plus basic Sixel DCS `q`
  bitmap rendering
  with RGB/HLS palette, raster-attribute minimum background dimensions,
  DCS/DECGRA pixel aspect controls, DCS `P2` background mode,
  DECSDM-controlled image origin, `?8452` right-edge cursor advancement, and
  `DCS 1000 q` tmux-control exclusion,
  with typed native-window user-var change hooks carrying the window id, pane
  id, name, and value for changed pane values, while
  Lua pane APIs/events, Kitty shared-memory transfers, remaining richer
  placement controls, broader query responses beyond current direct/local-file
  payload validation and stored-image existence checks, full Sixel protocol
  coverage, remaining badge variables/status formatting, and configurable key
  tables are not implemented yet.
- Kitty keyboard negotiation state is tracked and queryable through both
  `CSI = flags ; mode u` and push/pop forms, and disambiguated Ctrl/Alt ASCII
  character keys plus report-all plain text keys and Enter/Tab/Backspace use
  `CSI-u`; navigation/editing keys, F1-F12, and F13-F35 also use kitty
  canonical functional-key forms under disambiguate/report-all modes, keypad
  keys use kitty KP_* private-use codepoints under CSI-u reporting, and
  CapsLock, ScrollLock, NumLock, PrintScreen, Pause, and Menu/ContextMenu use
  kitty private-use functional codepoints, as do media transport, track, record,
  and volume keys exposed by the active input backend. Repeat/release events use
  kitty event-type subfields when flag 2 is active, including text keys when
  report-all is not active. Associated-text third fields
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
cargo run -p rssh-app -- start --frames 120
```

Start from a custom command:

```powershell
cargo run -p rssh-app -- window --frames 30 -- cmd.exe /C echo app-shell-smoke
cargo run -p rssh-app -- start --frames 30 -e cmd.exe /C echo app-shell-smoke
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
cargo test -p rssh-app ctrl_shift_alt_page_keys_are_not_default_tab_move_shortcuts
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
