# WezTerm Parity Gap Tracker (2026-06-09)

This tracker is scoped to MVP 6 (App Shell v1). It captures what is complete and
what remains before WezTerm-style parity in key UX/composition areas.

## App-Shell Parity

| Area | WezTerm baseline | R-SSH App Shell v1 | Status |
| --- | --- | --- | --- |
| Tabs | Dynamic tab model, selection, close, rename, relative/absolute movement, numbering, visible tab bar, explicit tab title, active-pane title fallback, close-tab last-active policy, `format-tab-title`, `new-tab-button-click` event | Workspace/tab model with active ID/actions, indexed activation, explicit tab title state/action, relative and absolute tab-order movement, wrapping/no-wrap relative tab activation, command-palette Rename Tab including `rename tab <title>` queries, keyboard dispatch, and a rendered native-window tab bar that prefers explicit tab titles before falling back to active-pane terminal titles from OSC 0/1/2 and Sun OSC L/l. Tab bar rendering now dispatches a typed `format-tab-title` hook in WezTerm-style two passes that can override the displayed title string or native Text/Foreground/Background/ResetAttributes plus Intensity Normal/Bold/Half, Italic true/false, and Underline None/Single/Double/Curly/Dotted/Dashed format items, consumes embedded SGR presentation escapes in Text items, and receives active, last-active, hover, first-pass WezTerm-default 16-cell `tab_max_width`, second-pass available-space `max_width`, an effective config snapshot for implemented window options, and TabInformation/PaneInformation-style snapshots for window id/title, all tabs in the window, explicit tab title, current-tab active pane and pane entries, active-tab top-level panes, pane geometry, pane titles, foreground process name, current working directory, unseen-output state, local domain name, tty name when known, user vars, and progress, with Lua event wiring plus full Lua config object still open. The tab bar also supports click activation, configurable visibility honoring `enable_tab_bar` and `hide_tab_bar_if_only_one_tab`, configurable placement honoring `tab_bar_at_bottom`, configurable mouse-wheel tab switching honoring `mouse_wheel_scrolls_tabs`, configurable close markers honoring `show_close_tab_button_in_tabs`, configurable active-tab close selection honoring `switch_to_last_active_tab_when_closing_tab` for default close-tab shortcuts, tab-bar clicks, and Close Current Tab command/confirmation paths, configurable tab index visibility honoring `show_tab_index_in_tab_bar`, configurable zero-based or one-based index labels honoring `tab_and_split_indices_are_zero_based`, configurable tab label visibility honoring `show_tabs_in_tab_bar`, a configurable new-tab button honoring the native `show_new_tab_button_in_tab_bar` effective-config field, static WezTerm-style Lua snippets for `tab_max_width` plus those boolean tab-bar config fields now parse into the same native override path, and a typed `new-tab-button-click` hook for Left/Right/Middle clicks that carries the default `NewTab` action for left click and can suppress it | ✅ Partial UI |
| Tab/pane navigation keys | `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+PageUp/Down`, `Ctrl+Shift+1..9`, `Super+1..9`, `Super+Shift+[/]`, `ActivateTab`, `ActivateLastTab`, `ActivateTabRelative`, `ActivateTabRelativeNoWrap`, `ActivatePaneByIndex`, `ActivatePaneDirection`, `Ctrl+Shift+Alt+Arrow` `AdjustPaneSize` resize defaults, `Ctrl+Shift+Z` zoom | `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+PageUp/Down`, `Ctrl+Shift+1..9` and `Super+1..9` via zero-based/negative `ActivateTabIndex`, command-palette Activate Tab 1..9 and Activate Last Tab, `Super+Shift+[/]` relative tab activation, command-palette wrapping and no-wrap Next/Previous Tab, command-palette Activate Pane By Index 1..8 plus native `WindowCommand::ActivatePaneByIndex(index)` payloads for arbitrary zero-based pane indices, `Ctrl+Shift+Arrow` `ActivatePaneDirection` plus command-palette Activate Pane Direction Left/Right/Up/Down/Next/Previous entries and native `WindowCommand::ActivatePaneDirection(direction)` payloads, move/resize-related defaults, `Ctrl+Shift+Alt+Arrow` `AdjustPaneSize` resize plus native `WindowCommand::AdjustPaneSize { direction, amount }` payloads, and `Ctrl+Shift+Z` zoom. Native default key-assignment entries now list the same implemented WezTerm defaults for tab navigation/movement, split creation, pane focus, and pane resize so `KEY_ASSIGNMENTS` launcher results reflect those bindings | ✅ |
| Panes | Split tree with active focus, indexed focus, focus navigation, resize, toggle/set zoom, pane rotation, pane-select modal, per-pane state | Pane model with ordered focus list, index activation, split metadata/deltas, zoomed-pane state with `TogglePaneZoomState` plus explicit `SetPaneZoomState`, configurable directional pane-switch unzoom behavior honoring `unzoom_on_switch_pane`, pane rotation preserving split positions/size deltas, per-pane snapshots, rendered split separators, click-to-focus with optional first-click swallowing honoring `swallow_mouse_click_on_pane_focus`, window-focus click swallowing honoring `swallow_mouse_click_on_window_focus`, configurable focus-follows-mouse honoring `pane_focus_follows_mouse`, configurable mouse-reporting bypass honoring `bypass_mouse_reporting_modifiers`, static WezTerm-style Lua snippets for those mouse/focus config fields now parse into the same native override path, native `disable_default_mouse_bindings` suppression for implemented default mouse assignments, native `hide_mouse_cursor_when_typing` cursor hiding/restoration, pane-local wheel routing, keyboard/palette `AdjustPaneSize` resizing, toggle zoom, command-palette `PaneSelect` default Activate plus ShowPaneIds/Swap/MoveToNewTab/MoveToNewWindow action paths, and native `PaneSelect { mode, show_pane_ids, alphabet }` action payload support | ✅ Partial UI |
| Workspaces | Domain-like workspace collection with active workspace switching | Workspace model with named workspaces, switch/rename action support, native window startup `--workspace` naming for the initial workspace, native `default_workspace` naming for the initial default workspace before spawn when no explicit startup workspace is present, command-palette `rename workspace <name>` and action-name `renameworkspace <name>` input, relative switching by lexicographic name order, a `SwitchToWorkspace` subset that switches to an existing workspace by name, creates missing named workspaces with the requested spawn command, uses native `default_prog` for newly created workspaces when the spawn command is omitted, or creates a randomly named workspace when the name is omitted, native `SwitchToWorkspaceArgs` payloads that carry the optional spawn command through the window action layer, command-palette `switch workspace <name> spawn <program> [args...]` and action-name `switchtoworkspace <name>` queries that carry the native SpawnCommand query subset into newly created workspaces while allowing quoted workspace names to contain `spawn`, command-palette `switch workspace spawn <program> [args...]` queries for randomly named workspaces with a spawn command, action-name `switchworkspacerelative <offset>` queries for relative workspace switching, and native `ShowLauncherArgs` `FUZZY` plus WezTerm-style pipe-separated `COMMANDS`/`DOMAINS`/`KEY_ASSIGNMENTS`/`LAUNCH_MENU_ITEMS`/`TABS`/`WORKSPACES` launcher selection through `show launcher <FLAGS>` queries. Native launcher alphabet support covers `show launcher <FLAGS> alphabet <chars>`, the `launcher_alphabet` effective-config fallback, one- or two-key direct selection, `j`/`k` selection movement, `/` fuzzy filtering in non-`FUZZY` launcher mode, and native `help_text`/`fuzzy_help_text` status prompts with WezTerm single-space default prompt fallbacks. `KEY_ASSIGNMENTS` covers native default plus native override key assignment entries, `DOMAINS` currently covers the local-domain spawn entry only, and `LAUNCH_MENU_ITEMS` covers native launch-menu overrides plus WezTerm-style static Lua `config.launch_menu` entries for the implemented `SpawnCommand` subset (`label`, optional `args`, `cwd`, `set_environment_variables`, and the local-domain selector), including default-program launch entries when `args` is omitted and indexed launch-menu entries with comments between `=` and item values. Remote/mux domains, richer default-mode UI styling, broader Lua config parsing, dynamic Lua menu construction, and Lua prompt callback wiring remain open | ✅ Partial |
| Action surface | Typed key assignments, key tables, command palette binding | `AppAction` typed command model and key binding path in app, with core `Nop` no-effect handling, native `WindowCommand::Nop` no-effect dispatch plus structured command-palette `nop` queries, native user `key_assignments` dispatch for matching regular key presses before built-in default shortcuts, native `WindowCommand::DisableDefaultAssignment` suppression for matching built-in app-shell, window-level, and scrollback shortcuts from user key assignments, native `disable_default_key_bindings` suppression for all implemented built-in default key assignments, core `Multiple` sequencing for already implemented `AppAction` values, native `WindowCommand::Multiple` sequencing for implemented window commands with first-failure stop semantics, and native `WindowCommand` coverage for WezTerm-style `SpawnWindow`, local-domain `SpawnTab`, native command-palette `SpawnCommandInNewTab`/`SpawnCommandInNewWindow` and split SpawnCommand query subsets, native `SpawnCommandInNewTab`/`SpawnCommandInNewWindow` action payloads carrying `args`/`cwd`/`set_environment_variables` plus the local-domain subset, with `SpawnCommandInNewWindow` also carrying `position` into the detached native window, native `SplitPane` action payloads carrying Left/Right/Up/Down direction, the local-domain subset `CurrentPaneDomain`, `DefaultDomain`, and `DomainName("local")`, optional command `args`/`cwd`/`set_environment_variables`, and Percent/Cells size for the new pane, native default `Ctrl+Shift+Alt+\"`/`%` key-assignment entries exposing WezTerm-style `SplitVertical={domain="CurrentPaneDomain"}` and `SplitHorizontal={domain="CurrentPaneDomain"}` payloads, native `PromptInputLine` `description`/`prompt`/`initial_value` line-input overlay with a typed native submission/cancel handler plus structured command-palette `prompt input line description <text>` queries for that payload subset, native `InputSelector` `title`/`choices`/`fuzzy`/`alphabet`/`description`/`fuzzy_description` selector overlay with typed native selected/cancel handler plus structured command-palette `input selector title <text> choices <id=label ; id=label>` queries for that payload subset, native `Confirmation` message overlays with required Yes and optional cancel actions plus typed accepted/cancel events and structured command-palette `confirmation message <text> action <command> [cancel <command>]` queries for typed nested native action subsets, native `EmitEvent` payloads dispatching custom event names with active window/pane metadata through a typed native handler, structured command-palette `emit event <name>` queries for those payloads, native `ActivateKeyTable`/`PopKeyTable`/`ClearKeyTableStack` payloads maintaining a per-window key-table activation stack with active-table status, timeout expiry, one-shot next-key auto-pop, prevent-fallback unmatched-key consumption, and reload-time clearing, structured command-palette `activate key table <name>` queries for native ActivateKeyTable payloads with timeout/one-shot/replace-current/until-unknown/prevent-fallback fields plus `pop key table` and `clear key table stack` queries for the stack mutation payloads, native `SendString` payloads writing raw string bytes to the active PTY input path, structured command-palette `send string <text>` queries for those payloads, native `SendKey` payloads encoding the requested key/modifiers through the active terminal input mode and writing the resulting bytes to the active PTY input path, structured command-palette `send key <mods+key>` queries for single-character payloads plus WezTerm-style logical named keys and F1-F35 identifiers, named `SwitchToWorkspace`, native `SwitchToWorkspaceArgs` payloads carrying optional spawn commands, native `SwitchWorkspaceRelative` payloads for arbitrary relative workspace offsets, `ActivateTab`, `MoveTab`, native `MoveTabRelative` payloads for arbitrary relative offsets, `SplitHorizontal`, `SplitVertical`, native `ActivatePaneDirection` payloads for Up/Down/Left/Right/Next/Previous, native `ActivatePaneByIndex` payloads for arbitrary zero-based pane indices, native `RotatePanes` payloads for clockwise/counter-clockwise pane rotation, native `AdjustPaneSize` payloads for direction plus cell amount, `TogglePaneZoomState`, native `SetPaneZoomState` boolean payloads, native `CompleteSelectionTo` and `CompleteSelectionOrOpenLinkAtMouseCursorTo` payloads for implemented copy destinations, `ShowTabNavigator`, `ToggleFullScreen`, native `CloseCurrentTab` and `CloseCurrentPane` payloads covering both immediate `confirm = false` close and `confirm = true` native confirmation overlays, structured command-palette `close current tab confirm true|false` and `close current pane confirm true|false` queries for those payloads, `Hide`, `HideApplication`, `QuitApplication`, `DecreaseFontSize`, `IncreaseFontSize`, `ResetFontSize`, `ResetFontAndWindowSize`, `ShowDebugOverlay`, `CharSelect`, `ReloadConfiguration` dispatch, default `Ctrl+Shift` and `Super` app-shell shortcuts for implemented tab/window actions, the default `Alt+Enter` full-screen shortcut, the default `Super+M` hide shortcut, the macOS-default `Super+H` application-hide shortcut, the default `Ctrl`/`Super` font-size shortcuts, the default `Ctrl+Shift+L` debug-overlay shortcut, the default `Ctrl+Shift+U` char-select shortcut, the default `Ctrl+Shift+R` reload shortcut, and native command-palette augment entries | ✅ Partial |
| Command palette | `ActivateCommandPalette` + Lua extension points | Minimal `Ctrl+Shift+P` palette with tab/pane/window/workspace actions, WezTerm-style `ActivateCommandPalette`, `SpawnWindow` through `Spawn Window`, local `SpawnTab` through `spawn tab current pane domain`, `spawn tab default domain`, and `spawn tab domain <name>` queries, native `new tab [--domain ...] [--cwd ...] [--env NAME=VALUE] [<program> [args...]]` and `spawn window [--domain ...] [--cwd ...] [--env NAME=VALUE] [--position ...] [<program> [args...]]` query subsets for `SpawnCommandInNewTab`/`SpawnCommandInNewWindow`, with omitted-program option queries applying supported options to the default-prog/inherited launch path, native `SpawnCommandInNewTab`/`SpawnCommandInNewWindow` action payloads carrying `args`/`cwd`/`set_environment_variables` plus the local-domain subset, with `SpawnCommandInNewWindow` also carrying `position` into the detached native window, named `SwitchToWorkspace` through `Switch To Workspace`, `switch workspace <name>`, and `switch workspace <name> spawn <program> [args...]` queries, `ActivateTab` through Activate Tab 1..9, `MoveTabRelative` through Move Tab Relative Left/Right, `ShowTabNavigator` through `Show Tab Navigator` with a native tab-list overlay that initially selects the active tab and can activate the chosen tab, `SplitHorizontal` through `Split Horizontal` plus `split horizontal <program> [args...]`/`split right <program> [args...]` queries, `SplitVertical` through `Split Vertical` plus `split vertical <program> [args...]`/`split down <program> [args...]` queries, native `SplitPane` action payloads carrying Left/Right/Up/Down direction, the local-domain subset `CurrentPaneDomain`, `DefaultDomain`, and `DomainName("local")`, optional command `args`/`cwd`/`set_environment_variables`, and Percent/Cells size for the new pane, `SelectTextAtMouseCursor` through Cell/Word/Line/Block/SemanticZone entries plus structured `selecttextatmousecursor <mode>` queries, `ExtendSelectionToMouseCursor` through Cell/Word/Line/Block/SemanticZone entries plus structured `extendselectiontomousecursor <mode>` queries, `ToggleFullScreen` through `Toggle Full Screen`, `Hide` through `Hide`, `HideApplication` through `Hide Application`, `QuitApplication` through `Quit Application`, `DecreaseFontSize`/`IncreaseFontSize`/`ResetFontSize`/`ResetFontAndWindowSize` through font-size entries, `ShowDebugOverlay` through `Show Debug Overlay`, `CharSelect` through `Char Select`, `ReloadConfiguration` typed native dispatch through the palette and default `Ctrl+Shift+R`/`Super+R` shortcuts, a visible native candidate overlay whose row count honors `command_palette_rows` while falling back to a terminal-height-based default and whose normal candidate rows honor `command_palette_bg_color`/`command_palette_fg_color`, command-palette frecency that promotes executed labels on empty queries and breaks equal fuzzy-score ties by use count then recency while persisting that state to JSON for later app instances, and a typed native `augment-command-palette` hook whose returned entries join the same fuzzy filtering, title status, selection, and execution flow using the implemented `WindowCommand` action subset while rendering optional `doc` text and known Nerd Font `icon` names beside the brief label. Lua event wiring, arbitrary Lua callbacks, full Nerd Font icon catalog coverage, richer styling, and full WezTerm action values remain open | ✅ Partial |
| Quick select | pattern matching mode for URLs/files/IP/email with label-based copy/paste selection | Implemented in v1 for Ctrl+Shift+Space and command-palette `QuickSelect` quick-select matching (`https?`, `file://`, `git@`, `git://`, `ssh://`, `ftp://`, markdown URLs, diff paths, docker SHA values, paths, colors, UUID/IPFS/SHA hashes, IPv4/IPv6, hex addresses, long numbers, email) with `Esc`, Tab/arrow navigation, `Ctrl+N`/`Ctrl+P`, PageDown/PageUp page-wise navigation, WezTerm-style Enter PriorMatch behavior, configurable labels honoring `quick_select_alphabet`, same-text candidate de-duplication before label assignment, configurable `quick_select_patterns` appended to default patterns, configurable `disable_default_quick_select_patterns` so configured patterns become the complete set, `quick_select_remove_styling` stripping pane colors, text styling, vertical alignment, hyperlink metadata, and inverse attributes before quick-select match/label highlights are applied, command-palette `quick select alphabet <chars>` covering the native `QuickSelectArgs { alphabet = ... }` subset, command-palette `quick select pattern <regex>` and `quick select patterns <regex> ; <regex>` covering native `QuickSelectArgs { patterns = ... }` override subsets, command-palette `quick select scope lines <n>` and `quick select scope_lines <n>` covering the native `QuickSelectArgs { scope_lines = ... }` subset, command-palette `quick select label <text>` covering the native status/overlay label subset, command-palette `quick select action open uri` covering a native open-uri action subset that dispatches the selected text through the same open-uri hook as hyperlink clicks, command-palette `quick select action copy to clipboard`/`copy to primary selection`/`copy to clipboard and primary selection` covering native `CopyTo` action subsets, command-palette `quick select action open uri skip action on paste`/`skip_action_on_paste` covering the native `skip_action_on_paste` subset for native action paths, native `QuickSelectArgs { patterns, alphabet, label, action, skip_action_on_paste, scope_lines }` action payload support, default `Ctrl+Shift+Space` key-assignment entries exposing `QuickSelect` with default native args while `EnterQuickSelect` remains an internal command-palette query alias, and label input: lowercase labels copy the match to ClipboardAndPrimarySelection, uppercase labels paste it into the pane, and partial label prefixes hide non-matching labels | ✅ |
| Copy mode | Vim-like keyboard selection in scrollback, copy, semantic-zone movement, and selection actions such as `ClearSelection` | Terminal mouse select exists with double-click word selection honoring `selection_word_boundary`; command-palette `SelectTextAtMouseCursor` Cell/Word/Line/Block selects text at the current mouse cell, command-palette `SelectTextAtMouseCursor` SemanticZone selects the OSC 133 semantic zone under the mouse, action-name `selecttextatmousecursor <mode>` queries dispatch the same native payloads, command-palette `ExtendSelectionToMouseCursor` Cell/Word/Line/Block extends the active selection to the current mouse cell, word, line, or rectangular block, command-palette `ExtendSelectionToMouseCursor` SemanticZone extends to the OSC 133 semantic zone under the mouse, and action-name `extendselectiontomousecursor <mode>` queries dispatch the same native payloads; copy-mode (Ctrl+Shift+X, Space/`v`, uppercase `V`, movement, copy/exit, line/char select) implemented, with command-palette `ActivateCopyMode` and Clear Selection, cross-scrollback semantic-zone movement via `z`/`Shift+Z`, typed Prompt/Input/Output movement via `Alt+P`, `Alt+I`, and `Alt+O`/`Alt+Z`, source-row selection/copy across retained history, `y` CopyTo ClipboardAndPrimarySelection plus ScrollToBottom/Close, Cell selection via Space/`v`, Line selection via uppercase no-modifier or shifted `V`, rectangular block selection via `Ctrl+V`, vertical/page movement through retained history, Enter/CR `MoveToStartOfNextLine`, `g`/`Shift+G` scrollback top/bottom, viewport `H`/`M`/`L` movement with shifted and uppercase no-modifier events, content-aware `^`/`Alt+m`/`$`/End first/last non-space movement, WezTerm-style word movement via `w`/`b`/`e`, Tab/Shift+Tab, Alt+Left/Right, and Alt+F/B, jump-to-char via `f`/`t`/`F`/`T` plus `;`/`,` repeat, selection-end movement via `o`/`O`, native `CopyMode` assignment payloads for implemented movement/selection-end/close actions plus single-name Lua table forms, full documented `SetSelectionMode` Cell/Word/Line/Block/SemanticZone modes, and search `NextMatch`/`PriorMatch`/`NextMatchPage`/`PriorMatchPage`/`ClearPattern`/`CycleMatchType`/`AcceptPattern`/`EditPattern`, ordinary copy-mode close with `ScrollToBottom` before `Close`, Escape/character ESC close in copy and search modes with search-status cleanup, `Ctrl+Shift+P` command-palette and `Ctrl+Shift+T` app-shell fallback from copy and copy-mode search, copy-mode search input plus next/prior match navigation including character CR PriorMatch, PageUp/PageDown page-wise match navigation, `Ctrl+R` match-type cycling, and `AcceptPattern`/`EditPattern` toggling whether typed text updates the search pattern; WezTerm-style Lua `config.keys`/`config.key_tables`/`config.leader` static snippets now parse native key tables and leader configuration for the implemented action subset, while full Lua config evaluation and file reload wiring remain open | ✅ Partial |
| Clipboard actions | `CopyTo` and `PasteFrom` for Clipboard/PrimarySelection buffers | Command-palette Copy To Clipboard plus native `CopyTo('Clipboard')` payloads and command-palette Paste From Clipboard plus native `PasteFrom('Clipboard')` payloads cover the system clipboard while plain `Ctrl+V` stays available to the PTY; default `Super+C`/`Ctrl+Shift+C`/`Copy` and `Super+V`/`Ctrl+Shift+V`/`Paste` shortcuts use the same native payload routing; native `canonicalize_pasted_newlines` covers `None`, `LineFeed`, `CarriageReturn`, and `CarriageReturnAndLineFeed` for non-bracketed paste while bracketed paste passes text unchanged inside bracketed-paste markers; native dropped-file paths are written to the active pane using `quote_dropped_files` modes `None`, `SpacesOnly`, `Posix`, `Windows`, and `WindowsAlwaysQuoted`, with WezTerm-style Windows/non-Windows defaults; static WezTerm-style Lua `config.canonicalize_pasted_newlines`, including boolean compatibility values, and `config.quote_dropped_files` snippets now parse into the same native paste/drop override path; deprecated native aliases `Copy`, `Paste`, and `PastePrimarySelection` route to the corresponding `CopyTo`/`PasteFrom` clipboard and primary-selection payloads for older config compatibility; OSC 52 and iTerm2 `OSC 1337;Copy=;base64` writes from ESC plus UTF-8 C1 OSC/ST active/inactive pane output, with legacy raw C1 compatibility, route through the same clipboard policy path; default OSC52 handling is WezTerm-style write-only with explicit `--osc52 read-write` for compatibility query responses; Copy To Primary Selection, Copy To Clipboard And Primary Selection, Paste From Primary Selection, native `CopyTo('PrimarySelection')`, native `CopyTo('ClipboardAndPrimarySelection')`, native `PasteFrom('PrimarySelection')`, default unmodified middle-click PrimarySelection paste, and `Ctrl+Insert`/`Shift+Insert` shortcut classification cover the PrimarySelection action routing; native OS PrimarySelection backend support remains open | ✅ Partial |
| Terminal reset | `ResetTerminal` injects `ESC c` on the pane output side | Command-palette Reset Terminal injects RIS into the active pane output path, resetting visible terminal state and scrollback through the terminal core | ✅ |
| Scrollback erase | `ClearScrollback('ScrollbackOnly')` and `ClearScrollback('ScrollbackAndViewport')` | Command-palette Clear Scrollback and native action payloads cover `ScrollbackOnly` on the active pane output side while preserving the viewport; Clear Scrollback And Viewport and native action payloads cover `ScrollbackAndViewport` by clearing active-pane history plus the viewport and preserving the prompt/cursor row as the new first visible line | ✅ |
| Scrollback navigation | `ScrollToTop`, `ScrollToBottom`, `ScrollByPage`, `ScrollByLine`, prompt-aware scroll actions, `ScrollByCurrentEventWheelDelta`, `scroll_to_bottom_on_input`, `alternate_buffer_wheel_scroll_speed`, `scrollback_lines`, `enable_scroll_bar`, and `min_scroll_bar_height` | Mouse wheel support plus default `Shift+PageUp`/`Shift+PageDown` shortcuts, with the same bindings listed in native `KEY_ASSIGNMENTS` launcher results as `ScrollByPage(-1)` and `ScrollByPage(1)`; command-palette Scroll To Top/Bottom, Scroll By Page Up/Down, Scroll By Line Up/Down, and Scroll To Prompt Previous/Next, plus native `ScrollByPage(amount)`, `ScrollByLine(amount)`, `ScrollToPrompt(amount)`, and `ScrollByCurrentEventWheelDelta` payloads backed by OSC 133 prompt row markers and the current vertical mouse-wheel event delta, with structured queries accepting spaced and action-name forms (`scroll by page <amount>`/`scrollbypage <amount>`, `scroll by line <amount>`/`scrollbyline <amount>`, and `scroll to prompt <amount>`/`scrolltoprompt <amount>`). Native per-window `scroll_to_bottom_on_input` defaults to true and scrolls the viewport back to bottom on terminal input; setting it false preserves the current scrollback viewport while writing input. In alternate screen with mouse reporting disabled, native wheel input writes Up/Down arrow-key sequences to the PTY using WezTerm's `alternate_buffer_wheel_scroll_speed = 3` default. Terminal history retention defaults to WezTerm's `scrollback_lines = 3500`, native config overrides apply to active and inactive pane runtimes, and shrinking the limit prunes retained semantic/image metadata while clamping scrollback viewports. The native scrollbar honors WezTerm's `enable_scroll_bar` default of false and renders/handles drag only when enabled; its thumb minimum defaults to WezTerm's `"0.5cell"`, native px, DPI-scaled pt, cell, and percent units for `min_scroll_bar_height` are applied to rendering and hit testing, and `colors.scrollbar_thumb` controls the rendered thumb color. Static WezTerm-style Lua `config.scroll_to_bottom_on_input`, `config.alternate_buffer_wheel_scroll_speed`, `config.scrollback_lines`, `config.enable_scroll_bar`, `config.min_scroll_bar_height`, and `config.colors.scrollbar_thumb` snippets now parse into the same native scroll/color override path | ✅ Partial |
| Semantic zones | OSC 133 Prompt/Input/Output/CommandFinished zones, command status metadata, `pane:get_semantic_zones()`, `pane:get_semantic_zone_at()`, text extraction, semantic-zone copy-mode movement | Terminal core records OSC 133 Prompt/Input/Output semantic zones across retained scrollback and visible grid, including line-scoped `I` input markers, OSC 133 `D` command-finished rows with exit status and `aid`, retained row/column region extraction with soft-wrap logical-line unwrapping, semantic-zone text extraction, cross-scrollback copy-mode zone movement including typed Prompt/Input/Output filters, selection/copy across retained history, and content-aware `^`/`Alt+m`/`$`/End line movement; Lua pane APIs and configurable key-table bindings remain open | ✅ Partial |
| Working directory metadata | OSC 7 / shell integration cwd tracking used for new panes/tabs | Terminal core records OSC 7 and iTerm2 `OSC 1337;CurrentDir`; app-shell pane launch metadata is updated from active and inactive pane runtimes, preferring OSC 7/current-dir metadata and falling back to the local session process tree cwd when the PTY backend exposes a pid, preferring child processes over the session root, then inherited by new tabs/splits, with `file://` cwd URI decoding for PTY spawn. Exact foreground process-group leader cwd detection remains open | ✅ Partial |
| PTY launch configuration | `default_prog` replaces the default shell when no program is specified; `term` controls `TERM`; `set_environment_variables` sets local-domain environment variables; `default_cwd` is used when no more specific cwd is resolved | Local PTY command construction defaults `TERM=xterm-256color` and `COLORTERM=truecolor`; native per-window config overrides now include `default_prog`, `default_domain`, `default_workspace`, `term`, `set_environment_variables`, and `default_cwd`, expose them through effective config snapshots, and apply them to newly spawned panes/windows so `term='wezterm'` launches panes with `TERM=wezterm`, configured environment entries are set, native new-tab/split/spawn-window and omitted-spawn new-workspace actions use `default_prog` while preserving inherited cwd, no-program `SpawnWindow` requests inherit the active pane launch/cwd when no `default_prog` override is active, local session process-tree cwd metadata is used before `default_cwd` when OSC 7/current-dir metadata is absent, pre-spawn native config overrides apply `default_prog` to the initial native window pane when startup would otherwise use the platform default shell, native `default_workspace` renames the initial default workspace before spawn when startup did not explicitly name one, native `default_domain` is retained in effective config and `SpawnTab(DefaultDomain)` maps to local spawning only while it is `local`, panes without a launch/OSC 7/process-tree cwd fall back to `default_cwd` and then the user home directory while explicit launch cwd still wins, local/window/console CLI startup accepts WezTerm-style `--cwd` for the initial child process, native window startup accepts WezTerm-style bare `<program> [args...]` and `-e` initial program forms, accepts WezTerm-style `--workspace` for the initial workspace name, `--class CLASS` for the Windows native window class name and retains it for later native windows spawned by the same app process, and `--position X,Y` plus `screen:X,Y`/`main:X,Y`/`active:X,Y`/`<monitor>:X,Y` for the initial native window screen position, accepts `--domain local` for the current local PTY domain, and accepts `--no-auto-connect`, `--always-new-process`, `--new-tab`, and `--attach` as compatibility no-ops until a GUI daemon and mux domain attachment exists. Static WezTerm-style Lua `config.default_prog`, `config.default_cwd`, `config.default_workspace`, `config.default_domain`, `config.term`, and `config.set_environment_variables` snippets now parse into the same native launch/workspace/domain override paths, including top-level `config['field']`, `config["field"]`, and `config[ [[field]] ]` bracket-index assignment forms, top-level `local config = { ... }` initializers, top-level `config = { ... }` table assignments, plus top-level static `return { ... }` config tables, while ignoring unrelated bare local fields, helper-function return tables, and helper-function config assignments. The static parser handles comments inside the brackets, comments between top-level config fields and `=`, comments between top-level config `=` and values, table field `--` line comments and `--[[...]]` long block comments even when they contain braces, bracketed table keys and numeric indices with comments inside the brackets, comments between table keys and `=`, comments between table `=` and values including indexed array entries, trailing comments after table values, and long-bracket table values that contain `}`, while ignoring Lua `--[[...]]` long block comments and `[[...]]` long bracket strings outside assignments. X11/Wayland class/app-id application, full CLI startup default-prog wiring, broader Lua config evaluation/reload, remote-domain filtering, WSL propagation, remote/named mux domains, and exact foreground process-group leader cwd probing remain open | ✅ Partial |
| Process exit behavior | `exit_behavior` controls whether a pane closes after its process exits: `Close`, `Hold`, or `CloseOnCleanExit`; `clean_exit_codes` adds non-zero exit codes that should count as clean; `exit_behavior_messaging` controls held-pane status text verbosity | Native per-window config overrides now include `exit_behavior` with WezTerm's current `Close` default, `clean_exit_codes` with WezTerm's implicit clean `0` behavior, and `exit_behavior_messaging` values `Verbose`, `Brief`, `Terse`, and `None`. PTY EOF handling waits for the process status, closes only the exited pane for `Close`, keeps the pane visible for `Hold`, closes successful exits plus configured clean exit codes for `CloseOnCleanExit`, and injects held-pane exit status text unless messaging is `None`; verbose/brief messages use WezTerm's documented success/failure prefixes and verbose messages report the actual `exit_behavior` value that caused the pane to remain open. Static WezTerm-style Lua `config.exit_behavior`, `config.clean_exit_codes`, and `config.exit_behavior_messaging` snippets now parse into the same native exit override path | ✅ Partial |
| Shell user variables | iTerm2/WezTerm `OSC 1337;SetUserVar` pane metadata, `pane:get_user_vars()`, and `user-var-changed` events | Terminal core base64-decodes `SetUserVar` values into pane user-var metadata; app-shell stores them per pane for active and inactive runtimes; native window dispatches a typed user-var change hook with the window id, originating pane id, name, and value for active/inactive pane metadata changes. Lua pane APIs/events remain open | ✅ Partial |
| iTerm2 badge metadata | `OSC 1337;SetBadgeFormat` base64 badge text | Terminal core base64-decodes `SetBadgeFormat`; app-shell stores it per pane for active and inactive runtimes; native rendering shows non-empty badge text as a pane-local top-right overlay. Badge interpolation covers pane user vars, `iterm2.pid`, `iterm2.localhostName`, `iterm2.effectiveTheme` plus tab/window/currentTab global-theme paths, implemented tab/window/current-session variables including explicit tab title overrides, profile name, OSC 133 last-command values, auto-name/auto-name-format values from OSC 1 icon title or loaded profile name, and latest reported `mouseInfo` arrays plus indices `[0]`, `[1]`, `[2]`, `[3]`, `[4]`, `[5]`, and `[6]` with iTerm2 modifier values and up/down/drag event-type values `0`/`1`/`2`, and implemented session variables including id/termid/pid/jobPid/tty/name/job/process/command/lastCommand/home/profile/host/path/title/size/mode/bell/selection values. The loaded TOML window profile name is exported as `RSSH_PROFILE` and exposed as `\(session.profileName)` plus the implemented current-session profile variables. Undefined badge variables evaluate to empty strings. Remaining badge variables and Lua/status formatting remain open; detailed variable mapping is listed below. | ✅ Partial |
| Notifications | OSC 9 iTerm2 notification and OSC 777 rxvt `notify` toast events | Native runtime extracts OSC 9 and OSC 777 `notify` events from ESC plus UTF-8 C1 OSC/ST active/inactive pane output, keeps legacy raw C1 compatibility, dispatches them through the window notification handler, and surfaces the latest notification in the window title. Native per-window `notification_handling` defaults to `AlwaysShow` and supports `NeverShow`, `SuppressFromFocusedPane`, `SuppressFromFocusedTab`, and `SuppressFromFocusedWindow` before handler dispatch and title-status updates. Static WezTerm-style Lua `config.notification_handling` snippets now parse into the same native override path. Local console filtering consumes OSC 9 notification/progress controls and OSC 777 notify controls so they do not leak to the host console. Native OS toast backend remains open | ✅ Partial |
| Pane progress | ConEmu-style `OSC 9;4;st;pr` progress state exposed through `pane:get_progress()` and tab/status formatting | Terminal runtime records `None`, percentage, error, and indeterminate progress states from ESC plus UTF-8 C1 OSC/ST 9;4 sequences, keeps legacy raw C1 compatibility, prevents progress reports from being misclassified as OSC 9 notifications, syncs the latest progress into active/inactive app-shell pane metadata, and shows active-pane progress in the native tab bar as `N%`, `err:N%`, or `~`. Lua pane API exposure and configurable status formatting remain open | ✅ Partial |
| Bell events | `bell` window event when ASCII BEL is emitted by any pane; `audible_bell` controls whether the system beep is played; `visual_bell` can flash the pane | Terminal core counts BEL events; native window dispatches a typed bell hook with the window id and originating pane id for active and inactive pane output while preserving metrics. Native per-window config overrides include WezTerm-style `audible_bell` values `SystemBeep` and `Disabled`; `Disabled` suppresses only the audible system beep path while preserving the bell hook and metrics. Native `visual_bell` supports WezTerm's zero-duration default no-op plus `BackgroundColor` pane flashes and `CursorColor` cursor-color flashes derived from the active rendered foreground color with a default text-foreground fallback; native `visual_bell_color` overrides stand in for WezTerm `colors.visual_bell`, background flashes include blank cells, native Constant/Linear/Ease/EaseIn/EaseOut/EaseInOut/CubicBezier easing blends over existing background/cursor colors across the configured fade-in/fade-out durations, and `CursorColor` fades return to the current rendered cursor color including `force_reverse_video_cursor` cursor-cell foreground behavior. Static WezTerm-style Lua `config.audible_bell`, `config.visual_bell`, and `config.colors.visual_bell` snippets now parse into the same native override path, including string easing names and `{ CubicBezier = { ... } }` table easing for visual-bell fade functions; Lua event wiring remains open | ✅ Partial |
| Window focus events | `window-focus-changed` event with the active pane when GUI focus changes | Native window dispatches a typed focus-change hook with the window id, focused/unfocused state, and the active pane while preserving CSI focus-reporting writes. Lua event wiring remains open | ✅ Partial |
| Window resize events | `window-resized` event with active pane, dimensions available via window APIs | Native window dispatches a typed resize hook after successful terminal/PTY resize and fullscreen/windowed transitions with the window id, active pane id, pixel size, terminal rows/columns, and `is_full_screen` state matching WezTerm window-dimensions metadata. Lua event wiring remains open | ✅ Partial |
| Window close confirmation | `window_close_confirmation` controls whether window-manager/decorations close requests prompt; `skip_close_confirmation_for_processes_named` skips stateless process targets; `quit_when_all_windows_are_closed` controls whether the GUI exits after the last window closes | Native per-window config overrides include `window_close_confirmation` with `AlwaysPrompt` default and `NeverPrompt`; window-manager/decorations close requests now enter a native `Close Window?` confirmation overlay by default and only request the actual window close after acceptance, while `NeverPrompt` requests close immediately. Native overrides also include WezTerm's default `skip_close_confirmation_for_processes_named` list and custom lists; close-window, close-tab, and close-pane confirmation targets skip the overlay when every affected pane's known local launch-program basename matches the list. Static WezTerm-style Lua `config.window_close_confirmation` and `config.skip_close_confirmation_for_processes_named` snippets now parse into the same native close-confirmation override path. The window manager honors WezTerm's `quit_when_all_windows_are_closed=true` default and keeps the process alive after the last window closes when the override is false. Full child process tree inspection and `mux-is-process-stateful` remain open | ✅ Partial |
| Window config reload events | `window-config-reloaded` event after automatic reload, `ReloadConfiguration`, or `window:set_config_overrides` | Command-palette `ReloadConfiguration` and the default `Ctrl+Shift+R` shortcut dispatch a typed native `window-config-reloaded` hook with the window id and active pane id. A typed native `set_config_overrides`/`get_config_overrides` subset stores per-window overrides for implemented effective-config fields (`tab_max_width`, `status_update_interval`, `max_fps`, `animation_fps`, `front_end`, `webgpu_power_preference`, `webgpu_force_fallback_adapter`, `webgpu_preferred_adapter`, `prefer_egl`, `enable_wayland`, `cursor_blink_rate`, `cursor_blink_ease_in`, `cursor_blink_ease_out`, `text_blink_rate`, `text_blink_rate_rapid`, `text_blink_ease_in`, `text_blink_ease_out`, `text_blink_rapid_ease_in`, `text_blink_rapid_ease_out`, `font_size`, `cell_width`, `cell_widths`, `line_height`, `bold_brightens_ansi_colors`, `default_cursor_style`, `cursor_thickness`, `underline_thickness`, `underline_position`, `strikethrough_position`, `force_reverse_video_cursor`, `window_padding`, `window_content_alignment`, `window_decorations`, `initial_cols`, `initial_rows`, `adjust_window_size_when_changing_font_size`, `command_palette_rows`, `command_palette_bg_color`, `command_palette_fg_color`, `char_select_bg_color`, `char_select_fg_color`, `launcher_alphabet`, `quick_select_alphabet`, `quick_select_patterns`, `disable_default_quick_select_patterns`, `quick_select_remove_styling`, `selection_word_boundary`, `term`, `audible_bell`, `visual_bell`, `compose_cursor_color`, `visual_bell_color`, `notification_handling`, `default_prog`, `default_domain`, `default_workspace`, `automatically_reload_config`, `check_for_updates`, `check_for_updates_interval_seconds`, `show_update_window`, `use_resize_increments`, `debug_key_events`, `log_unknown_escape_sequences`, `warn_about_missing_glyphs`, `default_cwd`, `detect_password_input`, `set_environment_variables`, `scroll_to_bottom_on_input`, `alternate_buffer_wheel_scroll_speed`, `canonicalize_pasted_newlines`, `quote_dropped_files`, `disable_default_key_bindings`, `disable_default_mouse_bindings`, `hide_mouse_cursor_when_typing`, `pane_focus_follows_mouse`, `swallow_mouse_click_on_pane_focus`, `swallow_mouse_click_on_window_focus`, `bypass_mouse_reporting_modifiers`, `enable_scroll_bar`, `min_scroll_bar_height`, `enable_tab_bar`, `hide_tab_bar_if_only_one_tab`, `unzoom_on_switch_pane`, `tab_bar_at_bottom`, `tab_and_split_indices_are_zero_based`, `mouse_wheel_scrolls_tabs`, `switch_to_last_active_tab_when_closing_tab`, `quit_when_all_windows_are_closed`, `window_close_confirmation`, `exit_behavior`, `clean_exit_codes`, `exit_behavior_messaging`, `skip_close_confirmation_for_processes_named`, `show_close_tab_button_in_tabs`, `show_new_tab_button_in_tab_bar`, `show_tab_index_in_tab_bar`, `show_tabs_in_tab_bar`, `treat_left_ctrlalt_as_altgr`, `treat_east_asian_ambiguous_width_as_wide`, `normalize_output_to_unicode_nfc`, `use_ime`, `ime_preedit_rendering`, and `xim_im_name`) and emits `window-config-reloaded` on every set. `window_content_alignment` keeps explicit non-cell-multiple window sizes, fills leftover gap pixels with the configured background, aligns the terminal grid, and reverse-maps mouse coordinates through the same offset. `automatically_reload_config` is stored with WezTerm's default `true` and included in effective config snapshots. `check_for_updates` is stored with WezTerm's default `true`, `check_for_updates_interval_seconds` with the default `86400`, and `show_update_window` with the compatibility default `false`; actual update checks and update-window UI remain open. `max_fps` is stored with WezTerm's default `60` and throttles native redraw requests from `about_to_wait` to the configured frame interval; `animation_fps` is stored with the default `10` and drives dedicated redraw scheduling for active cursor/text blink easing, visual bell fade, and animated inline-image frames while respecting the global `max_fps` ceiling. `front_end` is stored with WezTerm's current default `OpenGL`, `webgpu_power_preference` with `LowPower`, `webgpu_force_fallback_adapter` with `false`, optional static `webgpu_preferred_adapter` tables, and `prefer_egl`/`enable_wayland` with `true`; actual renderer front-end, WebGPU adapter, EGL, and Wayland/X11 startup selection remain open. `use_resize_increments`, `debug_key_events`, and `log_unknown_escape_sequences` are stored with WezTerm's default `false` and included in effective config snapshots; when enabled on X11/Wayland/macOS-capable builds, `use_resize_increments` advertises current cell-size resize hints and refreshes them after font/cell geometry changes, while unsupported platforms keep the WezTerm-style no-op behavior; `treat_left_ctrlalt_as_altgr` is stored with WezTerm's default `false`, and when enabled it routes Ctrl+Alt text key events as AltGr text input rather than triggering Ctrl+Alt key bindings, while exact platform left/right modifier source tracking remains open; `treat_east_asian_ambiguous_width_as_wide` is stored with WezTerm's default `false` and updates terminal character width calculation for ambiguous East Asian width characters, and static numeric `cell_widths` override tables parse from WezTerm-style Lua and take priority over that setting, while broader dynamic `cell_widths` Lua parity remains open; `use_ime` is stored with WezTerm's current default `true`, `ime_preedit_rendering` with WezTerm's `Builtin` default, and `xim_im_name` is retained as an optional XIM server name; native winit IME commit text is written to the active pane when `use_ime` is enabled and ignored when disabled; native winit IME preedit text renders as a Builtin overlay at the active pane cursor and is suppressed for `System` or disabled IME, while static `colors.compose_cursor` changes the cursor color during Builtin preedit, leader activation, or dead-key composition. Deeper platform IME/XIM setup remains open. `detect_password_input` is stored with WezTerm's default `true`; actual Unix local-pane termios probing and lock-cursor rendering remain open. `warn_about_missing_glyphs` is stored with WezTerm's default `true` and included in effective config snapshots. Static WezTerm-style Lua snippets for the implemented launch, paste/input, keyboard-protocol/IME, mouse/focus, diagnostics, update-check, frame-rate, render-backend, scroll, tab-bar, palette, quick-select, status, font/window-size, window-layout, cursor, text-blink/decoration, bell/notification, render-color, close-confirmation, and exit-behavior subsets now parse into the same native override path. Missing glyph codepoints detected in rendered cells are emitted once per native window as stderr `CONFIG ERROR missing glyph ...` diagnostics when `warn_about_missing_glyphs` is enabled. Unknown ESC/CSI sequences are recorded by the terminal runtime and emitted as native stderr warnings when `log_unknown_escape_sequences` is enabled. Native key events are emitted as stderr `INFO key_event` diagnostics when `debug_key_events` is enabled. Actual update checks, update-window UI, full WezTerm-style configuration error window UI, actual Lua config reload, automatic file watching, Lua `window:set_config_overrides` wiring, and broader config option coverage remain open | ✅ Partial |
| Window title formatting | `format-window-title` synchronous event can return a string to override the computed native window title | Native window title recomputation dispatches a typed formatter hook with the computed default title, active tab id, active pane id, tab count, active-tab pane count, and active key-table stack top plus TabInformation/PaneInformation-style snapshots for the active tab, active pane, all tabs in the window, and panes in the active tab, plus an effective config snapshot for implemented window options. Returning a string overrides the title; returning `None` keeps the default. Lua event wiring plus the full Lua config object remain open | ✅ Partial |
| Status events | `update-status`/`update-right-status` events can call `window:set_left_status` and `window:set_right_status` for tab bar status text | Native window dispatches typed `update-status` and deprecated `update-right-status` hooks from the native event loop with the window id and active pane id, scheduled by a WezTerm-style 1000ms `status_update_interval` default. The handlers can set stored left and right status strings, and native `set_left_status`/`set_right_status` methods update the same tab-bar state. The tab bar renders left status after the workspace label and right-aligns right status at the window edge, consumes SGR presentation escapes including blink/inverse/conceal/strikethrough/overline plus WezTerm underline style variants and ANSI/indexed/RGB foreground/background/underline color escapes in status strings, computes status layout from visible text, and clips over-wide right status from the left. Static Lua `config.status_update_interval` now parses into the native update interval override path; Lua window status API wiring remains open | ✅ Partial |
| Open URI events | `open-uri` window event before default URI opening; returning `false` suppresses default handling | Native window dispatches a typed open-uri hook for ctrl-clicked OSC 8 hyperlinks with the window id, active pane id, and URI before invoking the default opener. Terminal core tracks OSC 8 hyperlink metadata, preserves active hyperlinks across SGR reset, and clears them only when an empty OSC 8 URI closes the active link. Handlers can suppress the default opener by returning `false`. The command palette exposes WezTerm-style `CompleteSelection`, `OpenLinkAtMouseCursor`, and `CompleteSelectionOrOpenLinkAtMouseCursor`, completing active mouse selections into ClipboardAndPrimarySelection or opening the OSC 8 link under the mouse through the same open-uri hook. Lua event wiring remains open | ✅ Partial |
| Search UX | Search in pane + copy-mode navigation | Terminal scrollback search exists through default `Ctrl+Shift+F`/`Super+F` `Search(CaseSensitiveString="")`, command-palette `Search`, and native `Search` action payloads for `Regex`, `CaseSensitiveString`, `CaseInSensitiveString`, and `CurrentSelectionOrEmptyString`, including `search <pattern>`, `search regex <pattern>`, `search case-sensitive <pattern>`, `search case-insensitive <pattern>`, `search casesensitivestring <pattern>`, and `search caseinsensitivestring <pattern>` command-palette queries that open Search with an initial typed pattern, while plain `Ctrl+F` stays available to the PTY; search uses WezTerm-style search table navigation via Down/Up, `Ctrl+N`/`Ctrl+P`, PageUp/PageDown, `Ctrl+R` match-type cycling, `Ctrl+U` clear-pattern, character ESC close, and explicit current-selection query prefill collapsed to a single line, with F3/Shift+F3 handled only after search mode is active; copy mode now keeps copy-mode state while searching with `/`/`?`, Down/`Ctrl+N`, Up/Enter/CR/`Ctrl+P`, PageUp/PageDown match navigation, and `Ctrl+R` match-type cycling across case-sensitive, case-insensitive, and regex search. Broader Lua action wiring remains open | ✅ Partial |

## Terminal/Mux/System Parity

| Area | WezTerm baseline | R-SSH App Shell v1 | Status |
| --- | --- | --- | --- |
| Multiplexer | Local mux daemon, domains, SSH/TLS/Unix remote attachment | One PTY process model only | ❌ |
| Visual split layout | Split bars, size controls, zoomed panes, pane-select labels, pane focus visuals | Basic right/down split layout renders pane snapshots with separators; click/wheel hit-testing, configurable focus-follows-mouse, keyboard/palette resize, mouse drag resizing, zoom rendering, pane-select labels, WezTerm-style `foreground_text_hsb` transforms for terminal foreground/underline colors, WezTerm-style `bold_brightens_ansi_colors` handling for bold ANSI 0-7 foreground colors, terminal text `underline_thickness`, horizontal split divider `underline_thickness`, `colors.foreground` default text color, `colors.ansi`/`colors.brights` ANSI 0-15 palette overrides, `colors.indexed` indexed 16-255 palette overrides, `colors.selection_fg`/`colors.selection_bg` selection highlight overrides, `colors.cursor_bg` default cursor fill, `colors.cursor_border` block-cursor border and line-cursor color, `colors.cursor_fg` block-cursor text color, `colors.split` pane separator color, `text_background_opacity` alpha transforms for non-default terminal background cells, `window_background_opacity` alpha transforms for default terminal backgrounds, and `inactive_pane_hsb` color transforms for inactive pane Default/Indexed/RGB/RGBA cell colors are pane-local. Static WezTerm-style Lua snippets for `foreground_text_hsb`, `inactive_pane_hsb`, `bold_brightens_ansi_colors`, `colors.foreground`, `colors.ansi`, `colors.brights`, `colors.indexed`, `colors.selection_fg`, `colors.selection_bg`, `colors.cursor_bg`, `colors.cursor_border`, `colors.cursor_fg`, `colors.split`, `text_background_opacity`, and `window_background_opacity` now parse into the same native override path; remaining focus visual polish and dynamic palette-aware resolution remain | ⚠ Partial |
| GPU rendering | `wgpu` production render path with advanced shaping and font fallback | CPU bitmap demo renderer path used, limited shaping/fallback | ❌ |
| Font shaping | Ligatures, fallback stacks, color emoji | Basic fixed bitmap renderer | ❌ |
| Unicode | Broad protocol + shaping parity in progress | Strong xterm/core baseline plus documented C0 control handling, xterm cell/window size queries, DECALN `ESC # 8` screen alignment fill, DECSTR `CSI ! p` soft reset for insert/cursor-visibility/origin/scroll-region/left-right-margin/charset/saved-cursor state, screen reverse-video `?5` rendering with DECRQM reporting, cursor visibility/blink tracking for `?25`/`?12` with render snapshots carrying cursor blink state, renderer hidden-phase cursor suppression plus native cursor opacity interpolation for `cursor_blink_ease_in`/`cursor_blink_ease_out`, native `cursor_blink_rate` timing including `0` to disable cursor blinking, native text blink timing for SGR 5/6 with independent `text_blink_rate`/`text_blink_rate_rapid` phases plus normal/rapid easing options that interpolate foreground/decorations toward the rendered background, native `bold_brightens_ansi_colors` rendering for bold ANSI 0-7 foreground colors with `No`/`BrightAndBold`/`BrightOnly`, native `default_cursor_style` reset behavior for steady/blinking block, underline, and bar cursors, native `cursor_thickness` rendering for underline/bar cursors using px, DPI-scaled pt, percent-of-default, and cell-fraction units, native `underline_thickness` rendering for terminal text underlines using the same unit forms, native `underline_position` rendering for terminal text underlines using signed px, DPI-scaled pt, percent-of-default, and cell-fraction units against the current default underline-row baseline approximation, native `strikethrough_position` rendering for terminal text strikethroughs using px, DPI-scaled pt, percent-of-default, and cell-fraction units, and native `force_reverse_video_cursor` rendering from the cursor cell's effective foreground color unless OSC 12 set an explicit cursor color that OSC 112 can reset, reverse-wrap `?45` handling for BS at the left boundary with DECRQM reporting, `DECLRMM ?69` and `DECSLRM` left-right margin state for CR/BS and origin-mode `CUP`/`HVP`, SGR indexed/RGB/RGBA color state including WezTerm mode 6 alpha values, WezTerm SGR 5/6 blink state, WezTerm SGR 73/74/75 vertical-align state, DECRQSS SGR/cursor/vertical-margin/conformance/left-right-margin query responses, OSC 4/10/11/12 color queries/changes including multi-pair OSC 4 palette updates, multi-index queries, and RGBA dynamic color specs, iTerm2 `OSC 1337;ReportCellSize`, XTGETTCAP `TN`/`name` terminal-name aliases backed by the active terminal name, XTGETTCAP `RGB` color-depth reporting, XTGETTCAP `Cr`/`Cs` cursor-color templates, XTGETTCAP WezTerm-style per-name DCS responses with uppercase name/value hex, `0+r<name>` unknown-name reporting, and ST response framing, XTGETTCAP `Sync` synchronized-output template reporting, XTGETTCAP WezTerm SGR/style/select-color/title/title-stack/palette/keypad/reset-init/cursor-visibility/meta-key/printer/memory-lock templates, XTGETTCAP official boolean and numeric size/tab-interval reporting, XTGETTCAP tab-stop/erase/repeat/scroll-region/control/save-restore/ACS/query/mouse template reporting for implemented parser/input controls, and XTGETTCAP `kf13`-`kf63` modified function-key reporting for implemented input encoders; advanced sequences pending | ⚠ Partial |
| Selective erase | `DECSCA` protected character attributes plus `DECSED`/`DECSEL` | Terminal core stores protected-cell state from `CSI Ps " q`; DEC selective display/line erase (`CSI ? Ps J/K`) skips protected cells, while ordinary `ED`/`EL` still clears the addressed range | ✅ |
| Keyboard protocols | xterm defaults plus Meta-key mode, `modifyOtherKeys`, CSI-u, kitty progressive keyboard handling, and native Win32 input mode | xterm-style key encoding, application cursor/keypad modes, xterm Meta-key mode `?1034` with DECRQM and XTGETTCAP `km`/`smm`/`rmm`, xterm `modifyOtherKeys`, kitty keyboard progressive-enhancement state negotiation, and native-window/local console Win32 input mode are implemented: native runtime and console filtering consume `CSI > 4 ; N m`, answer `CSI ? 4 m`, encode modified other keys with `CSI 27 ; modifier ; code ~`, consume `CSI = flags ; mode u` plus `CSI > flags u`/`CSI < n u`, maintain the kitty flags stack, answer `CSI ? u`, and native windows honor the kitty negotiation/flags query path only when `enable_kitty_keyboard` is true. Native `allow_win32_input_mode` defaults to true, tracks ConPTY `CSI ? 9001 h/l`, can be disabled through native-window overrides, and makes native-window and local console input emit Win32 key records ahead of CSI-u/kitty encoding while active, including side-specific left/right Shift/Ctrl/Alt modifier-key VK/control-state data where available. Input encoders encode Ctrl/Alt ASCII character keys as kitty `CSI-u` events when the disambiguate flag is active, encode plain text keys plus Enter/Tab/Backspace as canonical `CSI-u` when report-all is active, use kitty canonical forms for navigation, editing, F1-F12 including current F3 `CSI 13~`, and F13-F35 functional keys, encode keypad keys with kitty KP_* private-use codepoints under CSI-u reporting including local-console `KP_BEGIN` as `CSI 57427~`, encode CapsLock/ScrollLock/NumLock/PrintScreen/Pause/Menu private-use functional key codepoints plus media transport/track/record/volume key codepoints, report repeat/release event types with `modifier:event` subfields including event-types-only text-key repeat/release, emit associated-text third fields when flag 16 is active with report-all, emit console/native-window text-key shifted alternate subfields when flag 4 is active, report crossterm-provided console Super/Hyper/Meta and CapsLock/NumLock modifier bits plus modifier-key private-use codepoints for left/right Shift/Ctrl/Alt/Super/Hyper/Meta and ISO level shifts, emit native-window printable PC-101 plus `IntlBackslash`/`IntlRo`/`IntlYen` physical base-layout alternate subfields, suppress duplicate native-window shifted/base-layout alternate subfields when they match the primary key or no physical base-layout key is available, and report native-window Super/Cmd/Windows modifier bits in kitty sequences; broader kitty alternate-key variants remain open | ⚠ Partial |
| Graphics/protocols | Kitty, iTerm2, sixel/image protocol support | iTerm2 `ReportCellSize` handshake is implemented for cell metrics, iTerm2/WezTerm `OSC 1337;File=...` inline image metadata and decoded payload bytes are retained by the terminal core, carried through render snapshots for live/scrollback/overlaid panes, and PNG/JPEG/GIF payloads are drawn into the framebuffer with delay-aware animated GIF frame selection by elapsed render time, cell/`px` dimensions, plus damage-region redraw coverage; native windows refresh the renderer animation clock on each framebuffer render so animated GIF frames advance across redraws; Kitty Graphics Protocol direct `a=T` payloads are parsed and rendered for single-block and chunked, uncompressed and zlib-compressed raw RGB/RGBA plus encoded image data, regular-file `t=f` simple-file transfers are parsed/rendered with optional `O`/`S` file slicing, temporary-file `t=t` transfers are parsed/rendered with guarded `tty-graphics-protocol` temp-file deletion, minimal stored image flow supports `a=t,i=<id>` and omitted-action default `a=t` uploads with `i` OK and invalid-parameter/payload `EINVAL` responses, `a=t,I=<number>` terminal-assigned image-number uploads with `i`/`I` OK responses, and `a=p` placement by image id or image number at the current cursor, direct and stored placements support basic `x`/`y`/`w`/`h` source-rectangle cropping, single-axis `c`/`r` aspect-ratio derivation, `X`/`Y` target pixel offsets, and cursor movement with `C=1` suppression, Kitty `a=q` support queries return `OK`/`EINVAL` for single-block/chunked direct payloads plus regular-file `t=f` and temporary-file `t=t` payloads without storing/displaying queried images, stored-image existence queries and stored placements return `OK` or `ENOENT` for present/missing image ids or image numbers, Kitty `q=1`/`q=2` OK/error response suppression is honored, `i`/`I` mutual exclusion is enforced, repeated `(image id, placement id)` pairs replace old placements, basic `a=d` deletion removes all visible Kitty placements, placements for a specific image id, placements for the latest image assigned to an image number, placements in an image-id range, a specific `(image id, placement id)` pair, cursor-cell placements, explicit-cell placements, visible-column placements, z-index placements, or cell-plus-z-index placements, basic `U=1` virtual placements, including combined `a=T,U=1` uploads, render from `U+10EEEE` placeholder cells with foreground image-id encoding, row/column diacritics, optional image-id high-byte diacritic, non-origin placeholder origin derivation, first-column row-only placeholders, stored left-cell inheritance for omitted placeholder diacritics, stale placeholder cleanup across control sequences, erase/reset cleanup for placeholder metadata, scroll-region movement plus scrollback rebase for placeholder metadata, and alternate-screen metadata isolation/restore, and visible placement deletion retains image data while a virtual placement still references it, terminal erase display cleanup removes retained inline images for `CSI 2J`/`CSI 3J`, `?1049` alternate-screen switching isolates main/alternate image placements, scroll operations move inline-image placements with affected text rows, and the renderer applies Kitty z-index layer ordering below/above text; basic Sixel DCS `q` payloads with VT340 default palette entries, RGB plus DEC HLS hue palette definitions, color selection, DCS `P1` macro pixel aspect, DECGRA `Pan`/`Pad` aspect override plus `Ph`/`Pv` minimum background dimensions, DCS `P2` transparent/opaque background mode, repeat introducers, carriage returns, sixel newlines, WezTerm-style DECSDM `?80h` active-graphics-origin placement with preserved text cursor, and xterm/WezTerm `?8452` right-edge cursor advancement are normalized into raw RGBA inline images and rendered; xterm dynamic color query/change handling includes WezTerm-style multi-pair OSC 4 palette updates, multi-index queries, and RGBA dynamic foreground/background/cursor color specs; Kitty shared-memory transfers/remaining richer placement controls/broader query responses beyond current direct/local-file payload validation and stored-image existence checks, full Sixel protocol coverage, and remaining sixel pan edge cases remain open | ⚠ Partial |
| Config layer | Lua config, events, hot reload, plugins | TOML profile system only for launch/runtimes | ⚠ Partial |
| Connectivity | Mux domains, serial/TLS domains, robust remote attach | Local PTY + SSH CLI/native russh paths, no mux domains | ⚠ Partial |

## What V1 Completes

- Deterministic `rssh-app window`/`rssh-app start` startup shell state: one
  workspace, one tab, one pane (IDs start at `1`).
- Tab/pane/workspace action dispatch in `rssh-core` and native-window integration.
- Keyboard shortcuts for new tab, close tab, tab cycling, split-right,
  split-down, plus app-shell last-active tab tracking.
- App-shell state now exposes WezTerm-style indexed tab activation through
  `ActivateTabIndex`, with `Ctrl+Shift+1..9` routed to indices `0..7/-1`
  and command-palette Activate Tab 1..9 entries plus
  `activate tab <index>`, `activate tab index <index>`, and
  `activatetab <index>` plus WezTerm-style
  `wezterm.action.ActivateTab(<index>)` function-call queries for direct
  selection. Action-name
  `activatelasttab` and `activatetab1` through `activatetab9` queries dispatch
  the corresponding fixed entries. Native `WindowCommand::ActivateTab(index)` payloads dispatch
  arbitrary positive or negative tab indices through the same app-shell path. The default
  `Ctrl+Shift+1..9` and `Super+1..9` key-assignment entries expose
  `ActivateTab(0..7/-1)` payloads while retaining numbered `Activate Tab 1..9`
  launcher labels.
- App-shell state now exposes WezTerm-style `ActivateTabRelative` wrapping and
  `ActivateTabRelativeNoWrap` clamping; the command palette includes both
  wrapping and no-wrap Next/Previous Tab entries plus
  `activate tab relative <offset>` and
  `activate tab relative no wrap <offset>` plus action-name
  `activatetabrelative <offset>` and `activatetabrelativenowrap <offset>` plus
  WezTerm-style `wezterm.action.ActivateTabRelative(<offset>)` and
  `wezterm.action.ActivateTabRelativeNoWrap(<offset>)` function-call queries.
  Action-name `nexttab`, `previoustab`, `nexttabnowrap`, and
  `previoustabnowrap` queries dispatch the corresponding fixed entries. Native
  `WindowCommand::ActivateTabRelative(offset)` and
  `WindowCommand::ActivateTabRelativeNoWrap(offset)` payloads dispatch arbitrary
  relative offsets through the same app-shell paths. The default `Ctrl+Tab`,
  `Ctrl+Shift+Tab`, `Ctrl+PageUp`, `Ctrl+PageDown`, and `Super+Shift+[/]`
  key-assignment entries expose `ActivateTabRelative` payloads while the
  command palette keeps Next/Previous Tab aliases.
- `MoveTabRelative` now reorders the active tab within the current workspace
  while preserving it as the active tab, and the command palette exposes
  Move Tab Relative Left/Right entries for one-step relative movement plus
  `move tab relative <offset>` / `movetabrelative <offset>` plus
  WezTerm-style `wezterm.action.MoveTabRelative(<offset>)` function-call
  queries. Native `MoveTabRelative(offset)` action payloads dispatch the same
  path for arbitrary relative offsets; action-name `movetabrelativeleft` and
  `movetabrelativeright` queries dispatch the fixed one-step entries.
- `MoveTab` now reorders the active tab to an absolute zero-based index, with
  command-palette Move Tab To 1..8 entries, `move tab <index>` and
  `move tab to <index>` plus `movetab <index>` queries, WezTerm-style
  `wezterm.action.MoveTab(<index>)` function-call queries, native
  `MoveTab(index)` action payloads for arbitrary indices, action-name
  `movetabto1` through `movetabto8` fixed entries, and typed
  out-of-range errors.
- Native window title includes shell state for easy smoke verification.
- Native window frame now defaults to a one-row tab bar with workspace/tab state,
  explicit tab title priority, active-pane terminal title fallback, mouse
  activation, configurable visibility honoring `enable_tab_bar` and
  `hide_tab_bar_if_only_one_tab`, configurable top/bottom placement honoring
  `tab_bar_at_bottom`, configurable mouse-wheel tab switching honoring
  `mouse_wheel_scrolls_tabs`, configurable close markers honoring
  `show_close_tab_button_in_tabs`, configurable active-tab close selection
  honoring `switch_to_last_active_tab_when_closing_tab` for default close-tab
  shortcuts, tab-bar clicks, and Close Current Tab command/confirmation paths,
  configurable tab index visibility honoring
  `show_tab_index_in_tab_bar`, configurable zero-based tab index labels honoring
  `tab_and_split_indices_are_zero_based`, configurable tab-label visibility honoring
  `show_tabs_in_tab_bar`, and a configurable new-tab button honoring
  `show_new_tab_button_in_tab_bar`. The retro tab bar strip honors
  `colors.tab_bar.background` for blank tab-bar cells, and active-tab,
  inactive-tab, plus new-tab button labels honor the corresponding
  `colors.tab_bar.active_tab`, `colors.tab_bar.inactive_tab`, and
  `colors.tab_bar.new_tab` `fg_color`/`bg_color` plus
  `intensity`/`underline`/`italic`/`strikethrough` entries; inactive-tab hover
  and new-tab button hover labels honor `colors.tab_bar.inactive_tab_hover`
  and `colors.tab_bar.new_tab_hover` `fg_color`/`bg_color` plus
  `intensity`/`underline`/`italic`/`strikethrough`. Retro tab labels and the
  new-tab button also honor configured `tab_bar_style` left/right edge
  `wezterm.format` items for active, inactive, inactive-hover, new-tab, and
  new-tab-hover states. Static Lua config parsing covers
  `enable_tab_bar`, `hide_tab_bar_if_only_one_tab`, `unzoom_on_switch_pane`,
  `tab_bar_at_bottom`, `tab_and_split_indices_are_zero_based`,
  `mouse_wheel_scrolls_tabs`, `switch_to_last_active_tab_when_closing_tab`,
  `quit_when_all_windows_are_closed`, `show_close_tab_button_in_tabs`,
  `show_new_tab_button_in_tab_bar`, `show_tab_index_in_tab_bar`,
  `show_tabs_in_tab_bar`, `colors.tab_bar.background`,
  `colors.tab_bar.active_tab`/`inactive_tab`/`inactive_tab_hover`/`new_tab`/
  `new_tab_hover` `fg_color`/`bg_color` plus
  `intensity`/`underline`/`italic`/`strikethrough`, and `tab_bar_style`
  `active_tab_left`/`active_tab_right`/`inactive_tab_left`/
  `inactive_tab_right`/`inactive_tab_hover_left`/`inactive_tab_hover_right`/
  `new_tab_left`/`new_tab_right`/`new_tab_hover_left`/`new_tab_hover_right`
  edge entries.
- App-shell state now exposes WezTerm-style `SpawnWindow`: the default
  `Ctrl+Shift+N` and `Super+N` shortcuts plus command-palette `Spawn Window`
  entry create a pending native-window app with a fresh default-launch tab and
  pane, and the multi-window manager materializes it as an additional OS
  window.
- App-shell keyboard routing now covers WezTerm-style `Super` defaults for
  implemented tab actions: `Super+T` `SpawnTab(CurrentPaneDomain)` new tab,
  `Super+Shift+T` `SpawnTab(DefaultDomain)` with configured `default_domain`
  validation,
  `Super+W` `CloseCurrentTab(confirm=true)` close-tab confirmation,
  `Super+1..9` indexed tab activation, and
  `Super+Shift+[` / `Super+Shift+]` relative tab activation. Native `SpawnTab`
  action payloads cover the local-domain subset by mapping `CurrentPaneDomain`,
  `DefaultDomain`, and `DomainName("local")` to the native `NewTab` launch path
  when they resolve to local. Remote/mux named domain spawning remains future
  mux/domain parity work.
- Command-palette close-current queries now accept both spaced forms
  `close current tab confirm true|false` /
  `close current tab confirm=true|false` /
  `close current pane confirm true|false` /
  `close current pane confirm=true|false` and WezTerm-style action-name forms
  `closecurrenttab confirm true|false` /
  `closecurrenttab confirm=true|false` /
  `closecurrentpane confirm true|false` /
  `closecurrentpane confirm=true|false`, all routing to the same typed
  `CloseCurrentTab` / `CloseCurrentPane` payloads. WezTerm-style
  `wezterm.action.CloseCurrentPane { confirm = ... }` and
  `wezterm.action.CloseCurrentTab { confirm = ... }` table-call queries
  dispatch the same payloads, including bracketed string table keys with
  long-bracket values. Action-name `closepane` and `closetab` queries dispatch
  the no-argument immediate-close aliases.
- Native window state now exposes WezTerm-style `ToggleFullScreen`: the default
  `Alt+Enter` shortcut and command-palette `Toggle Full Screen` entry toggle
  the native window fullscreen state and dispatch the typed resize hook with
  current fullscreen dimension metadata.
- Native window state now exposes WezTerm-style `StartWindowDrag`: the command
  palette entry and default `SUPER` + left mouse / `CTRL|SHIFT` + left mouse
  bindings request native drag-to-move via the window backend when available.
  Static WezTerm-style `config.mouse_bindings` now parses the native
  `Down`/`Up`/`Drag` plus `Left`/`Middle`/`Right`/vertical `WheelUp`/
  `WheelDown`/`streak = 1` subset with `mods`, `mouse_reporting`,
  `alt_screen`, and implemented native `action` payloads, so custom bindings
  such as `ALT` + left drag can dispatch `StartWindowDrag`, middle-button
  release can dispatch `PastePrimarySelection`, and `CTRL` + wheel-up can
  dispatch `IncreaseFontSize`. Matching user mouse bindings suppress the
  implemented default mouse assignment for the same button, modifiers,
  mouse-reporting state, and alternate-screen state; default mouse assignments
  are skipped while the pane has captured mouse reporting unless the configured
  bypass modifier is held. Broader wheel-delta/current-event details and click
  streak semantics remain open.
- Native window state now exposes WezTerm-style `ActivateWindow`,
  `ActivateWindowRelative`, and `ActivateWindowRelativeNoWrap` action payloads.
  The current window records a manager-level focus request; the multi-window
  manager orders native windows by app window id, uses zero-based absolute
  indexes for `ActivateWindow`, wraps only for `ActivateWindowRelative`, and
  restores/focuses the target OS window when one exists. Structured command
  palette queries now accept `activate window <index>`,
  `activate window index <index>`, `activatewindow <index>`,
  `activate window relative <offset>`, `activatewindowrelative <offset>`,
  `activate window relative no wrap <offset>`, and
  `activatewindowrelativenowrap <offset>` plus WezTerm-style
  `wezterm.action.ActivateWindow(<index>)`,
  `wezterm.action.ActivateWindowRelative(<offset>)`, and
  `wezterm.action.ActivateWindowRelativeNoWrap(<offset>)` function-call queries
  for those same payloads.
- Native `SetWindowLevel` action payloads now accept WezTerm's
  `AlwaysOnBottom`, `Normal`, and `AlwaysOnTop` values and update the native
  app's remembered window level. When a platform window exists, the level is
  mapped to winit's `WindowLevel` and applied through the OS-window backend;
  platforms without z-order support retain the remembered logical level. The
  command-palette queries `set window level <value>` and
  `setwindowlevel <value>` map AlwaysOnBottom/Normal/AlwaysOnTop spellings to
  the same native payload, including WezTerm-style
  `wezterm.action.SetWindowLevel '<value>'` and
  `wezterm.action.SetWindowLevel('<value>')` Lua action queries.
- Native window state now exposes WezTerm-style `ToggleAlwaysOnTop` and
  `ToggleAlwaysOnBottom`, toggling the remembered window level between the
  requested z-order and `Normal`.
- Native window state now exposes WezTerm-style `Show`, clearing a prior native
  hide request and, when a platform window exists, restoring visibility,
  unminimizing, and requesting focus.
- Native window state now exposes WezTerm-style `Hide`: the default `Super+M`
  shortcut and command-palette `Hide` entry request hide/minimize state,
  minimizing the platform window when available.
- Native window state now exposes WezTerm-style `HideApplication`: the
  macOS-default `Super+H` shortcut, command-palette `Hide Application` entry,
  and action-name `hideapplication` query record an application-hide request
  and use native window minimization as the current platform fallback when
  available. The default `KEY_ASSIGNMENTS` list includes `Super+H` only on
  macOS, matching WezTerm's platform-specific default.
- Native window state now exposes WezTerm-style `QuitApplication` through the
  command-palette `Quit Application` entry and action-name `quitapplication`
  query, requesting whole-application shutdown, dropping pending native-window
  apps, and preserving final metrics.
- Native window state now exposes WezTerm-style `DecreaseFontSize`,
  `IncreaseFontSize`, `ResetFontSize`, and command-palette
  `ResetFontAndWindowSize`: the default `Ctrl`/`Super` `-`, `=`, and `0`
  shortcuts plus command-palette entries update the logical font-size scale
  using WezTerm's 10% step or reset it to baseline. Action-name
  `decreasefontsize`, `increasefontsize`, `resetfontsize`, and
  `resetfontandwindowsize` queries dispatch the same commands. Native `font_size` defaults
  to WezTerm's `12.0` points and scales the fixed native base cell metrics.
  Native `cell_width` defaults to WezTerm's `1.0` ratio and further scales
  horizontal cell geometry, while native `line_height` defaults to WezTerm's
  `1.0` ratio and further scales vertical cell geometry used for rendering, hit
  testing, terminal size calculation, and frame sizing; shortcut zoom remains an
  additional scale over that configured baseline. Native
  `adjust_window_size_when_changing_font_size` defaults to the non-tiling
  WezTerm effective behavior of true, preserving terminal rows/columns by
  resizing the native frame and requesting the matching OS-window inner size
  when a native window exists; setting it false keeps the current window size
  and recomputes terminal rows/columns from the scaled cell size. Reset Font
  And Window Size also restores the native frame to the configured initial rows
  and columns. Native config overrides now expose `font_size`, `cell_width`,
  `cell_widths`,
  `line_height`, deprecated WezTerm-compatible `font_antialias`/`font_hinting`,
  `font_rasterizer`, `font_shaper`, `custom_block_glyphs`,
  `anti_alias_custom_block_glyphs`, `allow_square_glyphs_to_overflow_width`,
  `freetype_load_target`, `freetype_render_target`, `freetype_load_flags`,
  `freetype_interpreter_version`, `freetype_pcf_long_family_names`,
  `display_pixel_geometry`, `initial_cols`, `initial_rows`, and
  `adjust_window_size_when_changing_font_size`; static WezTerm-style Lua
  snippets for those fields now parse into the same native override path, with
  `freetype_render_target` following the effective load target when unset and
  `freetype_load_flags` defaulting to `DEFAULT` below 100 DPI or `NO_HINTING`
  at 100 DPI or higher. Custom block glyph, square-glyph overflow, FreeType
  interpreter-version, PCF long-family-name, display pixel-geometry, and font
  shaper options are retained in effective config with WezTerm defaults, while
  actual renderer glyph strategy, shaping-engine application, FreeType
  interpreter application, subpixel geometry application, PCF font-resolution
  changes, and full Lua config evaluation remain open.
- Native window state now exposes WezTerm-style `ShowDebugOverlay`: the default
  `Ctrl+Shift+L` shortcut, command-palette `Show Debug Overlay` entry, and
  action-name `showdebugoverlay` query record debug-overlay state for the
  active window and render a visible native diagnostic overlay with current
  window/tab/pane/workspace and runtime state plus recent native diagnostic log
  lines from key-event, unknown-escape, and missing-glyph warnings. Bare `Esc`
  closes the overlay without forwarding input to the PTY; Lua REPL support and
  full external log-source integration remain open.
- Native window state now exposes WezTerm-style `CharSelect`: the default
  `Ctrl+Shift+U` shortcut and command-palette `Char Select` entry enter native
  character-selection mode while closing other active overlays. Native
  `CharSelectArgs` payloads carry `copy_on_select`, `copy_to`, and `group`
  into the overlay state, and structured command-palette
  `char select copy_on_select <bool> copy_to <destination> group <name>` plus
  WezTerm-style action-name
  `charselect` default and
  `charselect copy_on_select <bool> copy_to <destination> group <name>` queries
  open the same typed payload paths with quote-aware field parsing and
  `field=value` assignment forms including `copy-on-select=false`,
  `copy-to=<destination>` / `copy-to="primary selection"`, and
  `group=<name>` / `group="<name with spaces>"`,
  so quoted group values with spaces do not retain their quotes. Duplicate
  `copy_on_select`, `copy_to`, and `group` fields are rejected instead of
  silently overriding an earlier field. When `group` is omitted, the overlay
  resolves it to `RecentlyUsed` after an accepted character selection and to
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
  category candidates including NerdFonts private-use glyphs, with normal
  candidate rows honoring WezTerm-style `char_select_bg_color` and
  `char_select_fg_color`; typed fuzzy queries and hex codepoint input also
  match the built-in NerdFonts names. Static WezTerm-style Lua
  `config.char_select_bg_color` and `config.char_select_fg_color` snippets now
  parse into the same native override path.
  ArrowUp/ArrowDown moves the selected candidate before Enter acceptance while
  scrolling the overlay past the first visible rows. RecentlyUsed candidates use
  persisted JSON selection counts plus a last-used sequence across app
  instances. Rendering the full categorized picker/database plus exact WezTerm
  frecency scoring remains open.
- Terminal title state now records OSC 0/1/2 plus Sun OSC L/l aliases, matching
  WezTerm's title/icon-title compatibility for tab fallback labels.
- App runtime and console output filtering now track kitty keyboard
  progressive-enhancement flags from `CSI = flags ; mode u` plus
  `CSI > flags u` / `CSI < n u`, including replace/set/reset flag application,
  with native windows honoring those mode sequences and answering `CSI ? u`
  only when `enable_kitty_keyboard` is true; console and native-window input now
  encode Kitty-compatible legacy Ctrl ASCII letters plus digit/symbol controls
  (`Ctrl+2` NUL, `Ctrl+3` ESC, `Ctrl+4` FS, `Ctrl+5` GS, `Ctrl+6` RS,
  `Ctrl+7`/`Ctrl+/` US, `Ctrl+8` DEL, `Ctrl+~` RS), encode disambiguated
  Ctrl/Alt ASCII character keys as kitty `CSI-u` events when flag 1 is active,
  and report plain text keys plus
  Enter/Tab/Backspace as canonical `CSI-u` when flag 8 is active. Kitty
  canonical forms now cover `Ctrl+Shift+Tab` as `CSI 9;6u` under
  disambiguate mode, navigation, editing, F1-F12 including the current Kitty
  `CSI 13~` F3 form, and F13-F35 functional keys plus
  KP_* keypad private-use codepoints under
  disambiguate/report-all modes, including local-console `KP_BEGIN` as
  `CSI 57427~`. The default console and native-window input paths encode
  Menu/ContextMenu with the legacy `CSI 29~` functional sequence. Xterm
  `modifyOtherKeys` negotiation from
  `CSI > 4 ; N m`, `CSI ? 4 m` query replies, and modified other-key encoding
  are implemented in console and native-window input paths; kitty event-type
  reporting now covers repeat/release events, including event-types-only text
  keys, with `modifier:event` subfields,
  CapsLock/ScrollLock/NumLock/PrintScreen/Pause
  and Menu/ContextMenu use kitty private-use functional key codepoints, and
  media transport, track, record, and volume keys use kitty private-use
  functional key codepoints where the input backend exposes them. Associated
  text third fields are emitted when flag 16 is active with report-all,
  including modified Enter (`Ctrl+Enter` -> `CSI 13;5;13u`,
  `Ctrl+Shift+Enter` -> `CSI 13;6;13u`), and pure text events without a known
  physical key now use kitty key number `0` while carrying the produced Unicode
  codepoints in that associated-text field.
  Console and native-window input now infer PC-101 unshifted primary codes for
  shifted ASCII punctuation such as `!` -> `1`, `+` -> `=`, and `>` -> `.`
  under CSI-u/report-all, disambiguate-only, and alternate-key reporting,
  including native-window events where the physical key is unavailable. Console
  and native-window text-key input emits kitty shifted alternate subfields when
  flag 4 is active, and
  console input now reports crossterm-provided Super/Hyper/Meta plus
  CapsLock/NumLock state modifier bits plus modifier-key private-use codepoints
  for left/right Shift/Ctrl/Alt/Super/Hyper/Meta and ISO level shifts.
  Native-window input additionally emits printable PC-101 plus
  `IntlBackslash`/`IntlRo`/`IntlYen` physical base-layout subfields plus
  Super/Cmd/Windows modifier bits, and suppresses duplicate
  shifted/base-layout alternate subfields when they match the primary key or
  no physical base-layout key is available;
  broader alternate-key variants remain a later keyboard-protocol slice. Native/local
  `allow_win32_input_mode` now defaults to true, tracks ConPTY
  `CSI ? 9001 h/l`, emits Win32 key records ahead of CSI-u/kitty encoding
  while active in native-window and local console input paths, and maps common
  Windows `F13`-`F24`, numpad virtual keys, plus OEM punctuation keys such as
  `+`, `-`, `/`, brackets, quotes, and space to their Win32 virtual-key codes
  even when no native physical key is available, and maps left/right
  Shift/Ctrl/Alt modifier-key events to their side-specific Win32 virtual-key
  codes and control-state bits where the input backend exposes the side. Native
  config now retains `treat_left_ctrlalt_as_altgr` with WezTerm's default
  `false`, routes Ctrl+Alt text key events as AltGr text input when it is
  enabled so Ctrl+Alt key bindings do not fire for those events,
  `treat_east_asian_ambiguous_width_as_wide` with WezTerm's default `false`,
  `normalize_output_to_unicode_nfc` with WezTerm's default `false`,
  `use_ime` with WezTerm's current default `true`,
  `ime_preedit_rendering` with the `Builtin` default, plus optional
  `xim_im_name`; `treat_east_asian_ambiguous_width_as_wide` updates terminal
  character width calculation for ambiguous East Asian width characters, and
  static numeric `cell_widths` override tables parse from WezTerm-style Lua and
  take priority over that ambiguous-width setting. When enabled,
  `normalize_output_to_unicode_nfc` normalizes contiguous ordinary terminal
  output runs to Unicode NFC before rendering, including leading combining
  marks that arrive in the next PTY chunk when they compose with the prior
  cell without changing display width. Native winit IME commit text
  now writes to the active pane when `use_ime` is enabled and is ignored when
  disabled; native winit IME preedit text now renders as a Builtin overlay at
  the active pane cursor, is suppressed for `System` or disabled IME, and is
  cleared by commit or empty preedit. Static Lua `colors.compose_cursor`
  overrides the cursor color while Builtin preedit text, the leader modifier,
  or a dead key is active. Exact platform left/right modifier source tracking,
  deeper platform IME/XIM setup, and broader dynamic `cell_widths` Lua parity
  remain open.
- Terminal core now implements DECALN `ESC # 8` screen alignment display,
  filling the visible grid with `E` cells and resetting margins/origin mode.
- Terminal core now implements DECSTR `CSI ! p` soft reset without clearing
  cells or scrollback, exits the alternate screen, restores WezTerm/xterm-style
  auto-wrap and default style, clears reverse-wrap, screen reverse-video,
  application cursor keys, application keypad, modifyOtherKeys, Kitty
  placements, and stored Kitty image data, restores cursor visibility, and app
  runtime/console filtering track ESC plus C1 DECSTR for mode reports,
  including restoring `?7`/`?25` to set and `?45`/`?5`/`?1` to reset while
  XTGETTCAP exposes WezTerm `is2`/`rs2` reset/init templates.
- Terminal core now implements `DECSCA` protected-cell state and uses it for
  DEC selective display/line erase (`DECSED`/`DECSEL`), preserving protected
  cells while ordinary display/line erase still clears the addressed range.
- Terminal core now ignores WezTerm-documented non-printing C0 controls while
  preserving BEL/BS/HT/LF/VT/FF/CR/ESC special handling.
- Terminal core now matches WezTerm C0 line-feed handling: bare `LF`, `VT`, and
  `FF` move down while preserving the active column, `CR` returns to the active
  left margin, and tests use explicit `CRLF` when they need the next line to
  begin at column 0.
- Terminal core now consumes WezTerm-documented `ESC =`/`ESC >` application
  keypad mode escapes, while the app/runtime mode tracker handles the
  corresponding input encoding state.
- Terminal core plus app/runtime output filtering now consume standalone ST
  controls (`ESC \`, UTF-8 C1 `U+009C`, and legacy raw C1 `0x9C`) as no-effect
  sequences, so string terminators seen outside control strings do not render
  into the grid or local/native display output.
- Terminal core now tracks reverse-wrap mode `?45`; when auto-wrap is enabled,
  BS at the left boundary wraps to the previous row's right boundary, and app
  runtime plus console filtering answer `?45` DECRQM status queries.
- Terminal core now tracks DEC screen reverse-video mode `?5`; renderer
  snapshots apply it across the full viewport, including blank cells, and app
  runtime plus console filtering answer `?5` DECRQM status queries.
- Terminal core and renderer now preserve WezTerm SGR mode 6 RGBA colors for
  foreground, background, and underline color state.
- Terminal core now maps WezTerm SGR `6` rapid blink onto the existing blink
  cell attribute, matching SGR `5` visibility behavior and SGR `25` reset.
- Terminal core and renderer now preserve WezTerm SGR 73/74/75 vertical-align
  state for superscript, subscript, and baseline; app-shell DECRQSS SGR
  responses serialize active 73/74 state.
- App runtime and console output filtering now answer WezTerm-documented DECRQSS
  `"` `p` conformance-level and `s` left/right-margin queries. The `s` response
  reports modeled DECSLRM state, and DECRQM reports DECLRMM `?69` for ESC and
  C1 CSI forms.
- App runtime and console output filtering now answer common CSI terminal
  queries, including WezTerm-style primary, secondary (`CSI > c`/`>0c`), and
  tertiary (`CSI = c`/`=0c`) device attributes and terminal-parameter
  (`CSI x`/`0x`/`1x`) responses, WezTerm's default answered window reports
  (`CSI 14t`, `16t`, and `18t`, including accepted empty/extra numeric
  parameter forms in runtime and console filtering), and shared mode-status probes from
  UTF-8 C1 CSI (`U+009B`) input without leaking UTF-8 prefix bytes, while
  retaining legacy raw C1 CSI compatibility for existing tests and terminals.
  App runtime and console output filtering also answer WezTerm/iTerm2-compatible
  `OSC 1337;ReportCellSize` queries with the fixed cell pixel dimensions.
  WezTerm-unhandled DEC private cursor-position reports (`CSI ?6n`) are
  consumed without replying across ESC, raw C1, and UTF-8 C1 CSI forms.
  Console output filtering also consumes WezTerm-parsed device-attribute
  reports (`CSI ?1;0c`, `?1;2c`, `?6c`, and `?62/63/64...c`) without replying
  instead of leaking them to the host console.
  Runtime and console filtering also mirror WezTerm's default no-response
  handling for unanswered `CSI ... t` window reports and control operations:
  after answering implemented `CSI 14t`, `16t`, and `18t` report forms, they
  consume the remaining `CSI ... t` controls without a reply, including
  recognized no-response forms (`CSI 1t`-`8t`, `9;0..3t`, `10;0..2t`, `11t`,
  `13t`, `13;2t`, `14;2t`, `15t`, `19t`, `20t`, `21t`, and `22/23;0..2t`),
  unknown operations, and malformed parameters before they can mutate terminal
  title state or leak to the host console.
- Console output filtering now mirrors WezTerm's default
  `enable_checksum_rectangular_area=false` behavior for DECRQCRA
  (`CSI ... * y`): checksum requests are consumed without replying instead of
  leaking to the host console.
- App runtime and console output filtering now answer WezTerm-style
  XTSMGRAPHICS (`CSI ? ... S`) queries for color-register counts and
  Sixel/ReGIS graphics geometry, including read-attribute, read-maximum,
  reset-to-default, invalid-item, invalid-action, and large numeric parameter
  statuses across ESC, raw C1, and UTF-8 C1 CSI forms.
- App runtime and console output filtering now track WezTerm ANSI automatic
  newline mode (`CSI 20 h/l`) and report it through DECRQM (`CSI 20 $ p`)
  across ESC and raw C1 CSI forms.
- App runtime and console output filtering now track WezTerm ANSI
  bidirectional-support mode (`CSI 8 h/l`), reset it on DECSTR, and report it
  through DECRQM (`CSI 8 $ p`) across ESC and raw C1 CSI forms.
- App runtime and console output filtering now match additional WezTerm
  private-mode DECRQM reports: unsupported DECCOLM `?3` reports reset,
  grapheme clustering `?2027` reports permanently enabled, and private
  per-graphic color registers `?1070` track set/reset state across ESC and raw
  C1 CSI forms.
- App runtime and console output filtering now mirror WezTerm's DECRQM gaps for
  alternate-screen and save-cursor private modes: `?47`, `?1047`, `?1048`, and
  `?1049` report unknown (`;0$y`) even after their set/reset controls are
  consumed.
- App runtime and console output filtering now track WezTerm DEC/ANSI private
  mode `?2` and report its set/reset state through DECRQM across ESC and raw
  C1 CSI forms.
- App runtime, local console input, and native-window input now track WezTerm
  SGR-pixels mouse mode `?1016`, report it through DECRQM, keep it as the
  active extended mouse protocol while active, reset extended mouse encodings
  back to X10 on `?1005l`/`?1006l`/`?1015l`/`?1016l` like WezTerm, and encode
  native window mouse reports with terminal-relative 1-based pixel coordinates.
- App runtime and console output filtering now answer DECRQSS and XTGETTCAP
  queries wrapped in UTF-8 C1 DCS (`U+0090`), protect UTF-8 C1 DCS payloads
  from nested query matching, and retain legacy raw C1 DCS/ST compatibility.
- App runtime, console output filtering, shared mode-prefix tracking, and
  visible-output filtering now distinguish standalone raw C1 controls from
  `0x80..0x9f` bytes that are UTF-8 continuation bytes, so ordinary UTF-8 text
  such as bytes ending in `0x9b` is not consumed as a CSI/control sequence
  while raw C1 and UTF-8 C1 controls continue to work.
- App runtime and console output filtering now answer XTGETTCAP with
  WezTerm-style one-DCS-per-capability framing, uppercase response name/value
  hex, unknown capability names as `0+r<name>` including invalid-hex raw text
  and non-UTF-8 decoded-name fallbacks, and ST response terminators even when
  the query arrived with C1 ST.
- App runtime and console output filtering now answer XTGETTCAP `TN` and
  official WezTerm `name` terminal-name aliases. Native-window runtimes follow
  the current `term` config override, while the console filter follows the
  PTY command `TERM` environment and defaults to `xterm-256color`.
- App runtime and console output filtering now answer XTGETTCAP `RGB` with the
  WezTerm color-depth value `8/8/8`.
- App runtime and console output filtering now answer XTGETTCAP `Cr`/`Cs` with
  WezTerm cursor-color templates using BEL terminators.
- App runtime and console output filtering now answer XTGETTCAP `Sync` with the
  WezTerm synchronized-output terminfo template for the existing `2026` mode.
- App runtime and console output filtering now answer XTGETTCAP `Smol`,
  `smxx`, `rmxx`, `op`, `oc`, `sgr`, `sgr0`, `smso`, `rmso`, `setaf`, and
  `setab` using WezTerm-style style/color templates.
- App runtime and console output filtering now track cursor blink mode `?12`
  and Meta-key mode `?1034`, report them through DECRQM for ESC/C1 CSI forms,
  and answer XTGETTCAP `civis`, official WezTerm `cnorm`, `cvvis`, and
  `km`/`smm`/`rmm` cursor visibility/blink and Meta-key templates. Renderer
  snapshots now carry blinking cursor state, and the pixel renderer hides
  blinking cursors when the shared blink phase is hidden. Native window
  overrides expose WezTerm-style `cursor_blink_rate`, including `0` to keep
  blinking cursors visible, and the native redraw loop interpolates blinking
  cursor opacity with `cursor_blink_ease_in` and `cursor_blink_ease_out`
  using Constant/Linear/Ease-style easing functions. Native window overrides
  also expose WezTerm-style `text_blink_rate`, `text_blink_rate_rapid`,
  `text_blink_ease_in`, `text_blink_ease_out`,
  `text_blink_rapid_ease_in`, and `text_blink_rapid_ease_out`; SGR 5 and SGR
  6 text blink keep separate opacity phases and interpolate foreground plus
  text decorations toward the rendered background. Native overrides also
  expose `default_cursor_style` and `cursor_thickness` using px,
  DPI-scaled pt, percent-of-default, and cell-fraction units. Native
  `underline_thickness` applies the same units to terminal text underline
  decorations. Native `underline_position` applies signed px, DPI-scaled pt,
  percent-of-default, and cell-fraction units to terminal text underline
  placement using the current default underline row as a baseline
  approximation. Native `strikethrough_position` applies px, DPI-scaled pt,
  percent-of-default, and cell-fraction units to terminal text strikethrough
  decorations, and native
  `force_reverse_video_cursor` rendering uses the cursor cell's effective
  foreground color unless OSC 12 set an explicit cursor color, and OSC 112
  resets that override. `DECSCUSR 0` plus full terminal reset restore the
  configured steady or blinking block/underline/bar default. Static WezTerm-style
  Lua `config.cursor_blink_rate`, `config.cursor_blink_ease_in`,
  `config.cursor_blink_ease_out`, `config.text_blink_rate`,
  `config.text_blink_rate_rapid`, `config.text_blink_ease_in`,
  `config.text_blink_ease_out`, `config.text_blink_rapid_ease_in`,
  `config.text_blink_rapid_ease_out`, `config.default_cursor_style`,
  `config.cursor_thickness`, `config.underline_thickness`,
  `config.underline_position`, `config.strikethrough_position`, and
  `config.force_reverse_video_cursor` snippets now parse into the same native
  override path, including string dimensions with units and bare numeric pixel
  values for the decoration dimensions plus `{ CubicBezier = { ... } }` table
  easing for cursor and text blink easing fields.
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
- Command palette now exposes Rename Tab, including `rename tab <title>` and
  action-name `renametab <title>` query input with quote-aware text parsing,
  and writes an explicit title for the active tab. Rename Workspace also
  accepts quote-aware `rename workspace <name>` and action-name
  `renameworkspace <name>` query input for naming the active workspace.
- App-shell state now exposes a named WezTerm-style `SwitchToWorkspace` subset
  plus native `SwitchToWorkspaceArgs` and `SwitchWorkspaceRelative` payload
  dispatch:
  an existing workspace with the requested name becomes active without creating
  another workspace, while a missing named workspace is created with the
  requested spawn command and selected. Native `SwitchToWorkspaceArgs` carries
  that optional spawn command through the window action layer without replacing
  existing workspace launches. Missing workspaces created without an explicit
  spawn command use native `default_prog` for the new pane when configured.
  Native `default_workspace` overrides rename the initial `default` workspace
  before spawn while preserving explicit startup workspace names.
  Omitted-name actions create randomly named workspaces, including
  `wezterm.action.SwitchToWorkspace` no-argument action-name queries. Relative
  payloads switch by arbitrary signed offsets using the
  same sorted workspace order as Next/Previous Workspace, and structured
  command-palette `switch workspace relative <offset>`, action-name
  `switchworkspacerelative <offset>`, and WezTerm-style
  `wezterm.action.SwitchWorkspaceRelative(<offset>)` queries dispatch those
  native payloads.
  The command palette exposes named switching through `Switch To Workspace`,
  `switch workspace <name>`, and action-name `switchtoworkspace <name>`
  queries; `switch workspace <name> spawn [--domain ...] [--cwd ...]
  [--env NAME=VALUE] [--set-environment-variables NAME=VALUE]
  [<program> [args...]]` carries the same native `SpawnCommand` query subset into newly
  created workspaces, and quoted workspace names can contain the word `spawn`
  without being treated as the spawn-command delimiter. When the program is
  omitted, supported spawn options are applied to the default-prog/inherited
  launch path. `switch workspace spawn [--domain ...] [--cwd ...] [--env
  NAME=VALUE] [--set-environment-variables NAME=VALUE] [<program> [args...]]`
  creates a randomly named workspace with
  the requested launch command or commandless spawn options. WezTerm-style
  `wezterm.action.SwitchToWorkspace { name = ..., spawn = { ... } }` and
  `wezterm.action.SwitchToWorkspace({ name = ..., spawn = { ... } })` table
  queries dispatch the same implemented `name` plus native `SpawnCommand`
  subset, including bracketed string table keys with long-bracket values for
  nested commandless spawn options and `set_environment_variables` entries.
  Native `ShowLauncher` opens the default Launcher Menu for
  local-domain spawning plus native
  launch-menu items. Native `ShowLauncherArgs` accepts WezTerm-style
  pipe-separated flags through `show launcher <FLAGS>` and action-name
  `showlauncherargs <FLAGS>` / `showlauncher <FLAGS>` /
  `showlauncherargs flags=<FLAGS>` queries, accepting case-insensitive flag aliases and
  `_`/`-`/compact spellings for multi-word flags. `FUZZY` and,
  with `COMMANDS`, `DOMAINS`,
  `KEY_ASSIGNMENTS`, `LAUNCH_MENU_ITEMS`, `TABS`, and/or `WORKSPACES`, opens a
  launcher-scoped palette for built-in commands, the local domain spawn entry,
  native default plus override key assignment entries, native launch-menu items,
  active-workspace tabs, and existing workspaces. `FUZZY` without item flags
  opens an empty launcher. Native `ShowLauncherArgs` also carries an `alphabet`
  subset through `show launcher <FLAGS> alphabet <chars>` and
  `showlauncherargs <FLAGS> alphabet <chars>` / `showlauncher <FLAGS>
  alphabet <chars>` queries; non-`FUZZY`
  launcher mode treats configured one- or two-key shortcuts as direct entry
  execution, falling back to `launcher_alphabet` when the action omits
  `alphabet`, handles `j`/`k` selection movement, and uses `/` to enter fuzzy
  filtering. Native payloads and quote-aware `show launcher <FLAGS> help_text
  <text> fuzzy_help_text <text>` / `showlauncherargs <FLAGS> help_text <text>
  fuzzy_help_text <text>` / `showlauncher <FLAGS> help_text <text>
  fuzzy_help_text <text>` query fields plus `help text`/`fuzzy help text`
  and hyphenated field-key aliases also carry `help_text` and
  `fuzzy_help_text` status prompts plus alphabet/title strings, keep field-key
  words inside help/title text when a later valid field boundary exists,
  accept both `field <text>` and `field=<text>` forms, and use WezTerm
  single-space default prompt fallbacks. Structured
  `show launcher <FLAGS>` queries reject unknown top-level fields instead of
  silently discarding them. Static WezTerm-style `config.launch_menu` snippets
  now feed native launch-menu entries for the implemented `SpawnCommand`
  subset (`label`, optional `args`, `cwd`, `set_environment_variables`, and the
  local-domain selector), including bracketed string table keys with
  long-bracket values for launch-menu item fields and environment entries, plus
  default-program launch entries when `args` is omitted. Remote/mux domains,
  richer default-mode UI styling, broader Lua key assignment/config parsing,
  dynamic Lua `launch_menu`
  construction, Lua `ShowLauncherArgs` config/event wiring, Lua
  `PromptInputLine` callback wiring, and Lua event/config wiring remain open.
- Command palette now exposes WezTerm-style `ActivateCommandPalette`, matching
  the default `Ctrl+Shift+P` shortcut and reopening a fresh palette when the
  action is invoked from the command palette itself; action-name
  `activatecommandpalette` queries dispatch the same command.
- Command palette now exposes WezTerm-style `ShowTabNavigator` as `Show Tab
  Navigator`, opening a native tab-list overlay with the active tab initially
  selected and Enter activating the selected tab; action-name
  `showtabnavigator` queries dispatch the same command.
- Command palette now exposes WezTerm-style `SpawnWindow` as `Spawn Window`,
  using the same pending native-window path as the default `Ctrl+Shift+N`
  shortcut. Action-name `spawnwindow` queries dispatch the same default
  command.
- Command palette now exposes native query subsets for WezTerm-style
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
  `SpawnCommandInNewWindow` action payloads now carry a WezTerm-style
  `SpawnCommand` `args`/`cwd`/`set_environment_variables` subset through the
  same tab/window launch paths, accept and ignore the launch-menu-only `label`
  field for key-assignment action parsing, accept the local-domain subset
  `CurrentPaneDomain`, `DefaultDomain`, and `DomainName("local")` including
  the WezTerm `domain = { DomainName = "local" }` table form, and
  `SpawnCommandInNewTab`/`SpawnCommandInNewWindow` accept both
  `wezterm.action.SpawnCommandInNewTab { ... }` and
  `wezterm.action.SpawnCommandInNewTab({ ... })`-style Lua table action forms.
  `SpawnCommandInNewWindow` carries the WezTerm-style `position` payload into
  the detached native window's initial position, including Lua table
  `{ x = ..., y = ..., origin = ... }` values for screen, main-screen,
  active-screen, and named-screen origins, including bracketed string table keys
  with long-bracket values on the position table and nested named-origin table.
  Remote/mux domains and full Lua parsing remain open.
- Native `SpawnTab` action payloads now carry a local-domain subset:
  `CurrentPaneDomain`, `DefaultDomain`, and `DomainName("local")` create and
  activate a new tab through the same native `NewTab` launch path. Structured
  command-palette `spawn tab current pane domain`, `spawn tab default domain`,
  and `spawn tab domain <name>` queries plus action-name `spawntab ...` aliases
  dispatch the same payload subset with quote-aware domain-name parsing, and
  no-argument `spawntab` dispatches the current-pane-domain default. WezTerm-style
  `wezterm.action.SpawnTab 'CurrentPaneDomain'`,
  `wezterm.action.SpawnTab('CurrentPaneDomain')`,
  `wezterm.action.SpawnTab 'DefaultDomain'`,
  `wezterm.action.SpawnTab('DefaultDomain')`,
  `wezterm.action.SpawnTab { DomainName = 'local' }`, and
  `wezterm.action.SpawnTab({ DomainName = 'local' })` queries dispatch the same
  implemented local-domain subset, including bracketed string table keys with
  long-bracket values. Remote/mux named domain spawning remains open.
- Native `AttachDomain`/`DetachDomain` action parsing now recognizes official
  WezTerm-style domain action forms including `AttachDomain 'devhost'`,
  `AttachDomain('devhost')`, `DetachDomain 'CurrentPaneDomain'`, and
  `DetachDomain { DomainName = 'devhost' }`, including bracketed string table
  keys with long-bracket values for the domain-name table form. Because the
  current domain model is local-only, executing those actions returns the
  existing unsupported-action result instead of pretending to attach or detach a
  mux domain; actual remote domain import/removal behavior remains open.
- Command palette split entries now use WezTerm action names:
  native `SplitHorizontal` dispatches the right-side split path and is exposed
  as Split Horizontal, while native `SplitVertical` dispatches the downward
  split path and is exposed as Split Vertical. The native default
  `Ctrl+Shift+Alt+\"` and `Ctrl+Shift+Alt+%` key-assignment entries expose
  WezTerm-style `SplitVertical={domain="CurrentPaneDomain"}` and
  `SplitHorizontal={domain="CurrentPaneDomain"}` payloads while retaining the
  shorter command-palette aliases. Native
  `split horizontal <program> [args...]` / `split right <program> [args...]`
  and `split vertical <program> [args...]` / `split down <program> [args...]`,
  plus `split left <program> [args...]` and `split up <program> [args...]`,
  queries create the split with an explicit launch command. The WezTerm
  action-name forms `splitpane <right|down|left|up> ...` and
  `splitpane direction <right|down|left|up> ...` build the same native payload
  with the same quoted-value support for launch command option values and args.
  These query forms also accept `--percent N`/`--percent=N`, `--cells N`/
  `--cells=N`, `--top-level`/`--top-level=true|false`, and supported
  `--domain`/`--cwd`/`--env`/`--set-environment-variables`/
  `--set_environment_variables` options in any order before the optional launch
  command; commandless split queries apply supported spawn fields to the
  default-prog/inherited launch path.
  Native `SplitPane` action payloads now carry
  Left/Right/Up/Down `direction` values, the local-domain subset
  `CurrentPaneDomain`, `DefaultDomain`, and `DomainName("local")`, plus
  optional `SpawnCommand` `args`/`cwd`/`set_environment_variables` subset
  through the same split launch path, and support
  `size = { Percent = ... }` / `size = { Cells = ... }` for the new pane's
  initial size. Action-name `splitvertical` and `splithorizontal` queries
  dispatch the corresponding default split directions. WezTerm-style
  `wezterm.action.SplitPane({ ... })`,
  `wezterm.action.SplitHorizontal({ ... })`, and
  `wezterm.action.SplitVertical({ ... })` parenthesized Lua table calls now
  dispatch through the same implemented split table payload parser, with
  `SplitHorizontal`/`SplitVertical` also accepting top-level SpawnCommand
  fields such as `args`, `cwd`, and `set_environment_variables`, including
  bracketed string table keys with long-bracket values for the implemented
  split, size, and spawn-command fields. Native `SplitPane` payloads also
  support `top_level = true` by splitting the full active-tab root region and
  compressing the existing layout into the source side. Full Lua table parsing
  remains open.
- Command palette now exposes WezTerm-style `ToggleFullScreen` as
  `Toggle Full Screen`, using the same native fullscreen toggle as the default
  `Alt+Enter` shortcut; action-name `togglefullscreen` queries dispatch the
  same command.
- Command palette now exposes WezTerm-style `StartWindowDrag` as
  `Start Window Drag`, sharing the same native drag-to-move request path as
  the default modified-left-mouse bindings; static `config.mouse_bindings`
  covers the native `Down`/`Up`/`Drag` plus `Left`/`Middle`/`Right`/vertical
  `WheelUp`/`WheelDown`/`streak = 1` subset with `mouse_reporting` and
  `alt_screen` filters for implemented action payloads, and matching user
  mouse bindings override the implemented default mouse assignment for the same
  button/modifiers/reporting/alternate-screen state. Action-name
  `startwindowdrag` queries dispatch the same command.
- Native action payload dispatch now covers WezTerm-style `ActivateWindow`,
  `ActivateWindowRelative`, and `ActivateWindowRelativeNoWrap`, including
  manager-level absolute and wrap/no-wrap target selection across materialized
  OS windows. Structured command-palette queries cover
  `activate window <index>`, `activate window index <index>`,
  `activatewindow <index>`, `activate window relative <offset>`,
  `activatewindowrelative <offset>`,
  `activate window relative no wrap <offset>`, and
  `activatewindowrelativenowrap <offset>` plus WezTerm-style
  `wezterm.action.ActivateWindow(<index>)`,
  `wezterm.action.ActivateWindowRelative(<offset>)`, and
  `wezterm.action.ActivateWindowRelativeNoWrap(<offset>)` function-call
  queries.
- Native `SetWindowLevel` action payloads cover `AlwaysOnBottom`, `Normal`,
  and `AlwaysOnTop` values in the native action layer and apply them to the
  platform window through winit's `WindowLevel` when backend support exists.
  The command-palette query `set window level <value>` covers the same value
  set with quote-aware value parsing, including WezTerm-style
  `wezterm.action.SetWindowLevel '<value>'` and
  `wezterm.action.SetWindowLevel('<value>')` Lua action queries.
- Command palette and native action payloads now expose WezTerm-style
  `ToggleAlwaysOnTop` and `ToggleAlwaysOnBottom`, sharing the remembered native
  window-level state with `SetWindowLevel`; action-name `togglealwaysontop`
  and `togglealwaysonbottom` queries dispatch the corresponding commands.
- Command palette and native action payloads now expose WezTerm-style `Show`,
  mapping it to the native show/unminimize/focus path for the current window;
  action-name `show` queries dispatch the same command.
- Command palette now renders a visible native candidate overlay and honors the
  WezTerm-style `command_palette_rows` effective-config value; when unset, the
  visible row count is derived from terminal height. Normal candidate rows honor
  WezTerm-style `command_palette_bg_color` and `command_palette_fg_color`.
  Static WezTerm-style Lua `config.command_palette_rows`,
  `config.command_palette_bg_color`, `config.command_palette_fg_color`, and
  `config.launcher_alphabet` snippets now parse into the same native override
  path.
- Quick-select labels now honor the WezTerm-style `quick_select_alphabet`
  effective-config value, while retaining the documented default alphabet when
  unset. Native per-window config now supports `quick_select_patterns`, adding
  configured regexes to the default quick-select set, and
  `disable_default_quick_select_patterns`, making configured regexes the full
  set when true. `quick_select_remove_styling` strips pane colors, text styling,
  vertical alignment, hyperlink metadata, and inverse attributes before applying
  quick-select match/label highlights.
  Same-text quick-select candidates are de-duplicated before labels are assigned,
  matching WezTerm's duplicate-result label behavior.
  Static WezTerm-style Lua `config.quick_select_alphabet`,
  `config.quick_select_patterns`, `config.disable_default_quick_select_patterns`,
  and `config.quick_select_remove_styling` snippets now parse into the same
  native override path.
  The quote-aware command-palette query `quick select alphabet <chars>` covers the
  native `QuickSelectArgs { alphabet = ... }` subset, and
  `quick select pattern <regex>` plus
  `quick select patterns <regex> ; <regex>` cover native
  `QuickSelectArgs { patterns = ... }` override subsets, splitting only on
  unquoted ` ; ` separators so quoted regexes can include semicolons.
  `quick select scope lines <n>` and `quick select scope_lines <n>` cover the native
  `QuickSelectArgs { scope_lines = ... }` subset with a complete numeric value,
  including WezTerm's documented minimum of the current viewport height.
  `quick select label <text>` covers the native status/overlay label subset
  with quote-aware text parsing, and `quick select action open uri` with quoted
  or unquoted action names covers a native open-uri action subset using the same
  open-uri hook as hyperlink clicks.
  `quick select action copy
  to clipboard`, `quick select action copy to primary selection`, and
  `quick select action copy to clipboard and primary selection` cover native
  `CopyTo` action subsets with quoted or unquoted destinations, avoiding
  implicit destinations not requested by the explicit action.
  `quick select action open uri skip action on paste`/`skip_action_on_paste`/
  `skip-action-on-paste`, including `=true|false` suffixes, covers the native
  `skip_action_on_paste` subset for valid native action paths, including
  `action=<action> skip_action_on_paste=true|false` assignment forms.
  WezTerm-style action-name `quickselectargs pattern`/`patterns`/`alphabet`/
  `label`/`action`/`scope lines`/`scope_lines`/`scope-lines` query prefixes,
  with `pattern=<regex>`, `patterns=<regex>[;<regex>]`, `alphabet=<chars>`, `label=<text>`,
  `action=<action>`, `scope_lines=<n>`, and `scope-lines=<n>` assignment forms plus legacy
  `quickselect ...` aliases, with assignment fields combinable in the same query,
  map to the same implemented `QuickSelectArgs`
  fields. The native action payload also carries
  `QuickSelectArgs { patterns, alphabet, label, action, skip_action_on_paste,
  scope_lines }` directly for command-palette augmentation and later config
  wiring. The default `Ctrl+Shift+Space`
  key-assignment entry exposes `QuickSelect` with default native args, while
  WezTerm-style `quickselect`, `quickselectargs`,
  `wezterm.action.QuickSelect`, and `wezterm.action.QuickSelectArgs` action
  names dispatch that same default entry; `EnterQuickSelect` remains an
  internal command-palette query alias and action-name `enterquickselect`
  queries dispatch that default entry.
  WezTerm-style `wezterm.action.QuickSelectArgs { patterns = { ... },
  alphabet = ..., label = ... }` Lua table calls parse the same native options
  subset, including bracketed string table keys with long-bracket values, and
  the `action` field now accepts nested implemented `CopyTo`
  KeyAssignment values such as `wezterm.action.CopyTo 'Clipboard'`,
  `act.CopyTo('PrimarySelection')`, and
  `wezterm.action { CopyTo = 'ClipboardAndPrimarySelection' }`, including
  bracketed string keys on that nested wrapper table, plus explicit
  `Nop` KeyAssignments such as `wezterm.action.Nop` and `act.Nop()`, and
  destination-style `CompleteSelection`/`CompleteSelectionOrOpenLinkAtMouseCursor`
  KeyAssignments such as `wezterm.action.CompleteSelection 'Clipboard'` and
  `act.CompleteSelectionOrOpenLinkAtMouseCursor('PrimarySelection')`, plus
  `PasteFrom` KeyAssignments such as `wezterm.action.PasteFrom 'Clipboard'`,
  `act.PasteFrom('PrimarySelection')`, and
  `wezterm.action { PasteFrom = 'Clipboard' }`, plus `SendString`
  KeyAssignments such as `wezterm.action.SendString 'text'`,
  `act.SendString('text')`, `wezterm.action.SendString { string = 'text' }`,
  and `wezterm.action { SendString = { string = 'text' } }`, plus `SendKey`
  KeyAssignments such as `wezterm.action.SendKey { key = 'b', mods = 'ALT' }`,
  `act.SendKey({ key = 'LeftArrow', mods = 'ALT' })`, and
  `wezterm.action { SendKey = { key = 'b', mods = 'ALT' } }`, plus
  `EmitEvent` KeyAssignments such as `wezterm.action.EmitEvent 'name'`,
  `act.EmitEvent({ name = 'name' })`, and
  `wezterm.action { EmitEvent = { name = 'name' } }`, plus `Multiple`
  KeyAssignments such as `wezterm.action.Multiple { wezterm.action.SendString
  'text', wezterm.action.EmitEvent 'name' }` and `act.Multiple({ ... })` for
  the implemented nested command subset, plus `ActivateKeyTable` KeyAssignments
  such as `wezterm.action.ActivateKeyTable { name = 'resize_pane' }` and
  `act.ActivateKeyTable({ name = 'resize_pane' })`, plus key-table stack
  KeyAssignments such as `wezterm.action.PopKeyTable` and
  `act.ClearKeyTableStack()`, plus static `wezterm.action_callback(...)`
  custom actions as native-handler placeholders. Actual Lua callback execution
  remains open.
- Native `PromptInputLine` action payloads now carry `description`, optional
  `prompt`, and optional `initial_value`, open a modal line-input overlay, use
  WezTerm's `"> "` default prompt when `prompt` is omitted, submit `Some(line)`
  to a typed native handler on Enter, and submit `None` on Escape or `Ctrl+C`.
  Structured command-palette `prompt input line description <text> [prompt
  <text>] [initial_value <text>]` and action-name `promptinputline description
  <text> [prompt <text>] [initial_value <text>]` queries dispatch the same
  native payload subset with quote-aware text parsing, accepting
  `initial_value`, `initial value`, and `initial-value` field keys in both
  `field <text>` and `field=<text>` forms, and keep field-key words inside text
  values when a later valid field boundary exists. WezTerm-style
  `wezterm.action.PromptInputLine { description = ..., prompt = ...,
  initial_value = ... }` table-call queries also dispatch that native field
  subset, including bracketed string table keys with long-bracket values.
  Static Lua `wezterm.format { { Text = ... } }` values for `description` and
  `prompt` are reduced to their visible text for the native overlay, and static
  `action = wezterm.action_callback(...)` fields are accepted as native-handler
  placeholders, while styled prompt-line rendering and actual Lua
  `wezterm.action_callback` wiring remain open.
- Native `InputSelector` action payloads now carry `title`, `choices`, optional
  `alphabet`, optional `description`, optional `fuzzy_description`, and `fuzzy`,
  open a modal selector, support default-mode alphabet shortcuts, `/` fuzzy
  filtering, `j`/`k` plus arrow/Ctrl movement, Enter selection, and
  left-click row selection, plus Escape/`Ctrl+C`/`Ctrl+G` cancellation.
  Default-mode text that is not in `alphabet` is ignored until fuzzy mode is
  entered, matching WezTerm's split between shortcut selection and fuzzy
  filtering. The selector dispatches selected `id`/`label` or cancel `None`
  values through a typed native handler.
  Structured
  command-palette `input selector title <text> choices <id=label ; id=label>
  [alphabet <chars>] [description <text>] [fuzzy_description <text>] [fuzzy
  true|false|fuzzy=true|false]` queries plus action-name `inputselector ...`
  queries dispatch the same native payload subset with quote-aware field
  parsing, accepting
  `fuzzy_description`, `fuzzy description`, and `fuzzy-description` field keys,
  supporting both `field <text>` and `field=<text>` forms for selector fields,
  split choices only on unquoted semicolon separators including compact
  `id=label;id=label` forms so quoted labels can include semicolons, and keep
  field-key words inside title/description values when a later valid field boundary exists. Known
  fields following `choices` are treated as the earliest structured boundary,
  and duplicate `fuzzy` fields are rejected instead of silently overriding them.
  WezTerm-style
  `wezterm.action.InputSelector { title = ..., choices = "...", alphabet = ...,
  description = ..., fuzzy_description = ..., fuzzy = ... }` table-call queries
  also dispatch that native field subset, including bracketed string table keys
  with long-bracket values, when `choices` uses either the existing semicolon-
  delimited string form or WezTerm's Lua table-of-tables choice form with
  `{ label = ..., id = ... }` entries, including bracketed string keys on those
  nested choice tables. Static `wezterm.format { { Text = ... } }` label values
  are reduced to their text for native selector labels, while style items are
  ignored until styled selector rows are implemented. Static
  `action = wezterm.action_callback(...)` fields are accepted as native-handler
  placeholders, while actual Lua `wezterm.action_callback` wiring remains open.
- Native `Confirmation` action payloads now carry a message string, required Yes
  action, and optional No/cancel action. They open a modal confirmation overlay,
  dispatch typed native `accepted = true` events on Enter/`Y`/Space before
  running the Yes action, and dispatch `accepted = false` events on
  Escape/`N`/`Ctrl+C`/`Ctrl+G` before running the optional cancel action.
  Structured command-palette `confirmation message <text> action <command>
  [cancel <command>]` queries and action-name `confirmationmessage ...` aliases
  dispatch the same native payload subset for typed nested commands such as
  `send string`, `send key`, `emit event`, key-table stack mutations,
  copy/paste, clear-scrollback, and close-current-pane/tab confirmations, while
  using quote-aware message parsing and keeping field-key words inside
  message/action text when a later valid field boundary exists. The
  `message`/`action`/`cancel` fields accept both `field <text>` and
  `field=<text>` forms, and omitted messages default to WezTerm's
  ` Really continue?` prompt. WezTerm-style
  `wezterm.action.Confirmation { message = ..., action = ..., cancel = ... }`
  table-call queries also dispatch the same native nested-command subset,
  including bracketed string table keys with long-bracket values, and accept
  static `action`/`cancel = wezterm.action_callback(...)` fields as
  native-handler placeholders. Static Lua
  `wezterm.format { { Text = ... } }` values for `message` are reduced to
  their visible text for the native overlay, while styled confirmation
  rendering and actual Lua `wezterm.action_callback` wiring remain open.
- Native `EmitEvent` action payloads now carry a custom event name and dispatch
  it through a typed native handler with the active window id and pane id.
  Structured command-palette `emit event <name>` and action-name
  `emitevent <name>` queries dispatch the same typed payload path with
  quote-aware event-name parsing. WezTerm-style
  `wezterm.action.EmitEvent { name = ... }` and
  `wezterm.action.EmitEvent({ name = ... })` table-call queries dispatch the
  same typed payload path, including bracketed string table keys with
  long-bracket values. Lua `wezterm.on`/`wezterm.emit` wiring remains open.
- Native `ActivateKeyTable`, `PopKeyTable`, and `ClearKeyTableStack` action
  payloads now maintain a per-window key-table activation stack and show the
  active table in native window status and title-formatting snapshots; reload
  configuration clears that stack as WezTerm documents, and timed activations
  expire via `timeout_milliseconds`. Matching native key-table assignments
  reset that timeout, and one-shot activations pop on the next native key
  press. `prevent_fallback` activations consume unmatched native key presses so
  they do not fall through to default shortcuts or PTY input, while
  `until_unknown` activations pop when an unmatched native key press is seen.
  Structured command-palette `activate key table <name> [timeout <ms>] [one shot
  true|false] [replace current true|false] [until unknown true|false] [prevent
  fallback true|false]` queries dispatch native `ActivateKeyTable` payloads,
  with snake_case and hyphenated field aliases such as `timeout_milliseconds`/
  `timeout-milliseconds`, `one_shot`/`one-shot`, `replace_current`/
  `replace-current`, `until_unknown`/`until-unknown`, and `prevent_fallback`/
  `prevent-fallback`, and single-token assignment forms such as
  `timeout=<ms>`, `one_shot=false`, and `prevent-fallback=true`. `one shot`
  defaults to true when omitted and single or double quotes group key-table
  names that contain spaces. Duplicate option fields are rejected instead of
  silently overriding earlier values. WezTerm-style
  `wezterm.action.ActivateKeyTable { ... }` and
  `wezterm.action.ActivateKeyTable({ ... })` table-call queries dispatch the
  same implemented activation payload fields, including bracketed string table
  keys with long-bracket values. Action-name
  `activatekeytable ...`, `popkeytable`, and `clearkeytablestack` aliases
  dispatch the same activation and stack mutation payloads as their spaced query
  forms. Native `key_tables` overrides now match table entries from the
  activation stack top downward and execute the matched native action,
  including implemented `CopyMode` assignment payloads. WezTerm-style static
  Lua snippets for `config.keys`, `config.key_tables`, and `config.leader`
  now parse native key assignment tables and leader configuration into the same
  override/runtime path for the implemented action subset, including bracketed
  string table keys with long-bracket values for leader fields, key-table names,
  and nested assignment fields. Static `config.keys` and `config.key_tables`
  action fields also accept `wezterm.action_callback(...)` values as no-op
  native placeholders so official callback-shaped bindings can load; full Lua
  callback execution, full Lua config evaluation, default key-table merging,
  and config-file reload wiring remain open.
- Native `Nop` action payloads now map to the no-effect app-shell action, so a
  native key/palette action can consume a trigger without mutating window state.
  Structured command-palette `nop` queries dispatch the same payload directly.
- Native user `key_assignments` now dispatch matching regular key presses
  before built-in default shortcuts and execute the configured native
  `WindowCommand` subset. Native key strings accept WezTerm-style `|`
  modifier grouping, such as `CTRL|ALT+D` and `LEADER|SHIFT+|`, in addition to
  the existing `+`-separated shorthand, and honor the documented
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
  `CSI ? 9001 h/l`, and makes native-window and local console input emit Win32
  key records for that mode before CSI-u/kitty encoding, including Windows
  `F13`-`F24`, numpad, and OEM punctuation virtual-key codes when only the
  produced character is known, plus side-specific left/right Shift/Ctrl/Alt
  modifier-key records where the input backend exposes the side. Static
  WezTerm-style Lua snippets for `config.key_map_preference`,
  `config.swap_backspace_and_delete`, `config.ui_key_cap_rendering`,
  `config.enable_csi_u_key_encoding`,
  `config.enable_kitty_keyboard`, `config.allow_win32_input_mode`,
  `config.treat_left_ctrlalt_as_altgr`,
  `config.treat_east_asian_ambiguous_width_as_wide`,
  `config.normalize_output_to_unicode_nfc`, `config.use_ime`,
  `config.ime_preedit_rendering`, and `config.xim_im_name` now parse into the
  same native override path. `config.ui_key_cap_rendering` now changes native
  command-palette key-assignment display labels across the documented UnixLong,
  Emacs, AppleSymbols, WindowsLong, and WindowsSymbols styles. The
  ambiguous-width option is applied to terminal character width calculation for
  active and new panes; static numeric `config.cell_widths` tables parse into
  the same native override path and take priority over the ambiguous-width
  option. `config.normalize_output_to_unicode_nfc` normalizes contiguous
  ordinary terminal output runs to Unicode NFC before cells are written,
  including leading combining marks that arrive in the next PTY chunk when
  they compose with the prior cell without changing display width.
  `treat_left_ctrlalt_as_altgr`
  routes Ctrl+Alt text key events as AltGr text input rather than triggering
  Ctrl+Alt key bindings. Native winit IME commit text writes to the active pane
  when `use_ime` is enabled and is ignored when disabled. Native winit IME
  preedit text renders through the Builtin overlay path at the active pane
  cursor and is suppressed for `System` or disabled IME. Static Lua
  `colors.compose_cursor` overrides the cursor color while Builtin preedit text
  or the leader modifier is active, and dead-key input uses the same cursor
  override while composition is pending. Exact platform left/right modifier
  source tracking, deeper platform IME/XIM setup, broader dynamic `cell_widths`
  Lua parity, and broader kitty alternate-key variants remain open.
- Native `leader` overrides now expose a WezTerm-style `LEADER` modal modifier
  subset for native user `key_assignments`: the configured leader key arms the
  virtual modifier until the next key press or `timeout_milliseconds`, only
  `LEADER` assignments match while active, and unmatched keys are swallowed
  before normal input resumes. Static WezTerm-style `config.leader` snippets
  now parse `key`, `mods`, and optional `timeout_milliseconds` into the same
  native leader runtime, including bracketed string table keys with
  long-bracket values; full keys config-file wiring remains open.
- Native `DisableDefaultAssignment` action payloads now suppress matching
  built-in default app-shell, window-level, and scrollback shortcuts from native
  user key assignments, leaving the key for the later input path. Structured
  command-palette `disabledefaultassignment` queries parse to the same native
  action payload for command/payload coverage.
- Native `SendString` action payloads now write the provided string bytes
  directly to the active PTY input path as typed input, without bracketed-paste
  wrapping. Structured command-palette `send string <text>` and action-name
  `sendstring <text>` queries dispatch the same typed payload path.
  WezTerm-style `wezterm.action.SendString { string = ... }` and
  `wezterm.action.SendString({ string = ... })` table-call queries dispatch the
  same typed payload path. Quoted action strings decode Lua-style escapes such
  as `\x1b`, `\027`, `\u{1b}`, and `\z` whitespace elision before dispatching
  the payload bytes, and accept Lua long bracket strings such as `[[text]]`,
  including inside Lua action payload table fields, bracketed string table keys,
  and indexed action forms such as `act["SendString"] [[text]]`.
- Native `SendKey` action payloads now encode the specified key and modifiers
  through the active terminal input mode, write the resulting bytes directly to
  the active PTY input path, and do not re-match key assignments. Structured
  command-palette `send key <mods+key>` and action-name `sendkey <mods+key>`
  queries cover single-character key payloads such as `send key ALT+B` plus
  WezTerm-style logical named keys and F1-F35 identifiers such as
  `send key ALT+LeftArrow` and `send key CTRL+SHIFT+F5`. WezTerm-style
  `wezterm.action.SendKey { key = ..., mods = ... }` and
  `wezterm.action.SendKey({ key = ..., mods = ... })` table-call queries route
  to the same implemented key/modifier payload parser, including bracketed
  string table keys with long-bracket values.
- Structured action queries now also accept WezTerm's older
  `wezterm.action { ActionName = value }` / `wezterm.action({ ActionName =
  value })` wrapper-table syntax for the implemented native action subset,
  including scalar action parameters such as `ActivateTabRelative = -1` and
  table payloads such as `SplitHorizontal = { domain = "CurrentPaneDomain" }`.
- Structured action queries now accept the common documented
  `local act = wezterm.action` alias form for implemented action constructors
  and wrapper tables, including `act.PromptInputLine { ... }` and
  `act { PasteFrom = "Clipboard" }`.
- Structured action queries now accept Lua indexed action constructors such as
  `wezterm.action["ToggleFullScreen"]`, `act["SendString"]("text")`, and
  `act["SendString"] "text"`, normalizing them through the same implemented
  action-name parser as dot constructors.
- Native `Multiple` action payloads now sequence implemented `WindowCommand`
  values in order and stop on the first failed command, matching WezTerm's
  multi-action key assignment model for the covered native action subset.
  Structured command-palette
  `multiple <command> ; <command> [; <command>...]` queries dispatch the same
  typed nested native action subset, splitting only on unquoted ` ; `
  separators so quoted `send string` payloads can contain semicolons.
  WezTerm-style Lua table calls now accept both
  `wezterm.action.Multiple { ... }` and
  `wezterm.action.Multiple({ ... })` forms for the same implemented nested
  action subset, including nested `wezterm.action.QuickSelectArgs({ ... })`,
  `wezterm.action.Search({ ... })`, and
  `wezterm.action.SwitchToWorkspace({ ... })` payload tables without dropping
  their typed options, plus structured nested split command queries such as
  `split horizontal <program> [args...]` without dropping their launch command,
  and option-only nested `SpawnCommandInNewTab`/`SpawnCommandInNewWindow`
  queries that apply `cwd`/environment/domain/window-position options to the
  default launch. Nested `rename tab <title>` and
  `rename workspace <name>` queries now retain their explicit payloads instead
  of falling back to generated rename labels when sequenced in `Multiple`.
  Nested pane-select shortcut queries now also retain their explicit alphabet,
  mode, and show-pane-ids options instead of falling back to the effective
  quick-select alphabet or default activate-mode display.
- Command palette now exposes WezTerm-style `ReloadConfiguration`, and the
  default `Ctrl+Shift+R` shortcut dispatches the same typed native
  `window-config-reloaded` hook with the window id and active pane id.
  Action-name `reloadconfiguration` queries dispatch the same command.
  `automatically_reload_config` is retained with WezTerm's default `true` in
  the native effective config. `check_for_updates` is retained with WezTerm's
  default `true`, `check_for_updates_interval_seconds` with the default
  `86400`, and `show_update_window` with the compatibility default `false`;
  actual update checks and update-window UI remain open. `max_fps` is retained
  with WezTerm's default `60` and throttles native redraw requests from
  `about_to_wait` to the configured frame interval; `animation_fps` drives
  dedicated redraw scheduling for active cursor/text blink easing, visual bell
  fade, and animated inline-image frames while respecting the global `max_fps`
  ceiling. `front_end` is retained
  with WezTerm's current default `OpenGL`, `webgpu_power_preference` with
  `LowPower`, and `prefer_egl`/`enable_wayland` with `true`; actual renderer
  front-end, WebGPU adapter, EGL, and Wayland/X11 startup selection remain open.
  `use_resize_increments`,
  `debug_key_events`, and `log_unknown_escape_sequences` are retained with
  WezTerm's default `false` in the native effective config. When
  `use_resize_increments` is enabled on X11/Wayland/macOS-capable builds, the
  native window advertises resize increments based on the current terminal cell
  width and height and refreshes them after font, `cell_width`, or
  `line_height` changes; unsupported platforms keep the WezTerm-style no-op
  behavior.
  `warn_about_missing_glyphs` is retained with WezTerm's default `true` in the
  native effective config. Static Lua config snippets for those diagnostics,
  update-check, and render-backend fields parse into the same native override path.
  Unknown ESC/CSI sequences
  are recorded by the terminal runtime and emitted as native stderr warnings when
  `log_unknown_escape_sequences` is enabled. Native key events are emitted as
  stderr `INFO key_event` diagnostics when `debug_key_events` is enabled.
  Missing glyph codepoints detected in rendered cells are emitted once per
  native window as stderr `CONFIG ERROR missing glyph ...` diagnostics when
  `warn_about_missing_glyphs` is enabled. Full WezTerm-style configuration
  error window UI, Lua config reload, automatic file watching, and Lua
  `window:set_config_overrides` wiring remain open.
- Command palette now dispatches a typed native `augment-command-palette` hook
  when opened, carrying the window id and active pane id. Returned native
  entries provide `brief`, optional `doc`/`icon`, and an implemented
  `WindowCommand` action; those entries join fuzzy filtering, palette status,
  selection, and execution, with optional `doc` text plus known Nerd Font
  `icon` names including `md_rename_box`, `fa_clock_o`, and `cod_github`
  rendered beside the brief label. Lua event wiring, arbitrary Lua callbacks,
  full Nerd Font icon catalog coverage, and the full WezTerm action surface
  remain open.
- Command palette now keeps frecency for executed command labels in memory and
  persists it to a JSON state file for later app instances. Empty queries prefer
  higher-use and then more-recently used entries, while fuzzy queries still sort
  first by match score and use frecency only as a tie-breaker. Exact WezTerm
  scoring remains open.
- Command palette now exposes WezTerm-style `ClearSelection`, and structured
  `clearselection` action-name queries clear active selection state and remove
  rendered selection highlights.
- Command palette and native action payloads now expose WezTerm-style
  `SelectTextAtMouseCursor` and `ExtendSelectionToMouseCursor` for Cell, Word,
  Line, Block, and SemanticZone modes using the current mouse cell. Structured
  action-name
  queries `selecttextatmousecursor <mode>` and
  `extendselectiontomousecursor <mode>` dispatch the same typed payloads, as do
  WezTerm-style `wezterm.action.SelectTextAtMouseCursor '<mode>'` and
  `wezterm.action.ExtendSelectionToMouseCursor '<mode>'` Lua action queries.
  SemanticZone selection and extension both use the OSC 133 semantic zone under
  the mouse.
- Command palette and native action payloads now expose WezTerm-style
  `ClearScrollback('ScrollbackOnly')`, clearing active-pane history on the
  output side while preserving the viewport. The structured command-palette
  queries `clear scrollback scrollback only` and
  `clearscrollback scrollback only` map to the same payload and accept quoted
  or unquoted mode text. WezTerm-style
  `wezterm.action.ClearScrollback('ScrollbackOnly')` string calls and
  `wezterm.action.ClearScrollback { mode = ... }` table-call queries dispatch
  the same native payload path, including bracketed string table keys with
  long-bracket values.
- Command palette and native action payloads now expose WezTerm-style
  `ClearScrollback('ScrollbackAndViewport')`, clearing active-pane history plus
  the viewport while preserving the prompt/cursor row as the new first visible
  line. The structured command-palette query
  `clear scrollback scrollback and viewport` and
  `clearscrollback scrollback and viewport` map to the same payload and accept
  quoted or unquoted mode text. Action-name `clearscrollbackandviewport`
  queries dispatch the no-argument compatibility command.
- Command palette and native action payloads now expose WezTerm-style
  `CopyTo('Clipboard')` for the active selection, and default
  `Super+C`/`Ctrl+Shift+C`/`Copy` map to the same Clipboard destination.
  Structured command-palette `copy to <destination>` and
  `copyto <destination>` queries map Clipboard, PrimarySelection, and
  ClipboardAndPrimarySelection spellings, quoted or unquoted, to native
  `CopyTo(destination)` payloads; action-name `copytoclipboard`,
  `copytoprimaryselection`, and `copytoclipboardandprimaryselection` queries
  dispatch the same commands.
- Command palette and native action payloads now expose WezTerm-style
  `CopyTo('PrimarySelection')` and `CopyTo('ClipboardAndPrimarySelection')`
  routing for the active selection.
- Command palette and native action payloads now expose WezTerm-style
  `PasteFrom('Clipboard')` for the active pane, and default
  `Super+V`/`Ctrl+Shift+V`/`Paste` map to the same Clipboard source while plain
  `Ctrl+V` stays available to the PTY. Structured command-palette
  `paste from <source>` and `pastefrom <source>` queries map Clipboard and
  PrimarySelection spellings, quoted or unquoted, to native
  `PasteFrom(source)` payloads; action-name `pastefromclipboard` and
  `pastefromprimaryselection` queries dispatch the same commands, as do
  WezTerm-style `wezterm.action.PasteFrom '<source>'` and
  `wezterm.action.PasteFrom('<source>')` Lua action queries.
- Command palette and native action payloads now expose WezTerm-style
  `PasteFrom('PrimarySelection')` routing for the active pane, and shortcut
  classification maps `Ctrl+Insert`/`Shift+Insert` to WezTerm's
  PrimarySelection defaults.
- Deprecated native WezTerm aliases `Copy`, `Paste`, and
  `PastePrimarySelection` now route to `CopyTo('Clipboard')`,
  `PasteFrom('Clipboard')`, and `PasteFrom('PrimarySelection')` respectively
  for compatibility with older action payloads. Action-name `copy`, `paste`,
  and `pasteprimaryselection` queries dispatch those aliases directly.
- WezTerm-style zero-argument Lua action queries such as
  `wezterm.action.ActivateLastTab()`, `wezterm.action.ShowTabNavigator()`, and
  `wezterm.action.ToggleFullScreen()` now normalize through the same no-argument
  command-palette action table as their bare action-name forms. Generic
  `wezterm.action { ActionName = {} }` empty-table wrappers, including
  whitespace-only empty tables such as `wezterm.action({ ToggleFullScreen =
  { } })`, also dispatch through the same no-argument action path.
- WezTerm-style close-current table actions now accept parenthesized Lua table
  calls such as `wezterm.action.CloseCurrentPane({ confirm = false })` and
  `wezterm.action.CloseCurrentTab({ confirm = true })`, in addition to the
  existing `Action { confirm = ... }` form, including bracketed string table
  keys with long-bracket values.
- WezTerm-style clear-scrollback table actions now accept parenthesized Lua
  table calls such as
  `wezterm.action.ClearScrollback({ mode = "ScrollbackOnly" })`, in addition
  to the existing `Action { mode = ... }` form.
- WezTerm-style clear-scrollback Lua action calls now accept parenthesized
  mode strings such as
  `wezterm.action.ClearScrollback('ScrollbackAndViewport')`, matching the
  existing structured and table-call payload path.
- WezTerm-style launcher table actions now accept Lua table calls such as
  `wezterm.action.ShowLauncherArgs { flags = "TABS|WORKSPACES", title = "Jump" }`
  and parenthesized calls such as
  `wezterm.action.ShowLauncherArgs({ flags = "TABS|WORKSPACES", title = "Jump" })`,
  routing the same `flags`, `title`, `alphabet`, `help_text`, and
  `fuzzy_help_text` fields as the existing command-palette query forms,
  including bracketed string table keys with long-bracket values.
- WezTerm-style character-selection table actions now accept Lua table calls
  such as
  `wezterm.action.CharSelect { copy_on_select = false, copy_to = "PrimarySelection", group = "PeopleAndBody" }`
  and parenthesized calls such as
  `wezterm.action.CharSelect({ copy_on_select = false, copy_to = "PrimarySelection", group = "PeopleAndBody" })`,
  routing the same `copy_on_select`, `copy_to`, and `group` fields as the
  existing command-palette query forms, including bracketed string table keys
  with long-bracket values.
- WezTerm-style quick-select table actions now accept parenthesized Lua table
  calls such as
  `wezterm.action.QuickSelectArgs({ pattern = "ticket-[0-9]+", action = "open-uri", alphabet = "12" })`,
  routing the same `pattern`/`patterns`, `action`, `alphabet`, `label`,
  `skip_action_on_paste`, and `scope_lines` fields as the existing
  command-palette query forms, including bracketed string table keys with
  long-bracket values.
- WezTerm-style pane-selection table actions now accept parenthesized Lua table
  calls such as
  `wezterm.action.PaneSelect({ mode = "SwapWithActive", show_pane_ids = true, alphabet = "12" })`,
  routing the same `mode`, `show_pane_ids`, and `alphabet` fields as the
  existing command-palette query forms.
- WezTerm-style prompt-input table actions now accept parenthesized Lua table
  calls such as
  `wezterm.action.PromptInputLine({ description = "Rename tab", prompt = "name: ", initial_value = "old name" })`,
  routing the same `description`, `prompt`, and `initial_value` fields as the
  existing `Action { ... }` and command-palette query forms.
- WezTerm-style input-selector table actions now accept parenthesized Lua table
  calls such as
  `wezterm.action.InputSelector({ title = "Pick Reply", choices = "decline=No thanks ; lgtm=LGTM", alphabet = "ab" })`,
  routing the same `title`, `choices`, `alphabet`, `description`,
  `fuzzy_description`, and `fuzzy` fields as the existing `Action { ... }` and
  command-palette query forms, including table-of-table choices and text-only
  `wezterm.format` labels.
- WezTerm-style confirmation table actions now accept parenthesized Lua table
  calls such as
  `wezterm.action.Confirmation({ message = "Send command?", action = "sendstring yes", cancel = "sendstring no" })`,
  routing the same `message`, `action`, and optional `cancel` fields as the
  existing `Action { ... }` and command-palette query forms.
- WezTerm-style destination actions now accept single-argument function-call
  forms including `wezterm.action.CopyTo('PrimarySelection')`,
  `wezterm.action.CompleteSelection('PrimarySelection')`, and
  `wezterm.action.CompleteSelectionOrOpenLinkAtMouseCursor('PrimarySelection')`,
  matching the existing bare-string routing to implemented clipboard
  destinations.
- WezTerm-style mouse-selection mode actions now accept single-argument
  function-call forms including
  `wezterm.action.SelectTextAtMouseCursor('SemanticZone')` and
  `wezterm.action.ExtendSelectionToMouseCursor('Block')`, matching the existing
  bare-string routing to implemented selection modes.
- Default `Super+R`, `Super+K`/`Ctrl+Shift+K`, and `Super+F` shortcuts now
  route to the same reload-configuration, clear-scrollback, and search paths as
  the existing WezTerm-style actions.
- Command palette now exposes WezTerm-style `ResetTerminal` as Reset Terminal,
  injecting RIS on the active pane output side. Action-name `resetterminal`
  queries dispatch the same command.
- Command palette now exposes WezTerm-style scrollback navigation for top,
  bottom, page, line, and OSC 133 prompt movement; native `ScrollByPage`,
  `ScrollByLine`, `ScrollToPrompt`, and `ScrollByCurrentEventWheelDelta`
  payloads route signed WezTerm amounts and the current vertical mouse-wheel
  event delta through the same helpers. Action-name `scrolltotop`,
  `scrolltobottom`, `scrollpageup`, `scrollpagedown`, `scrolllineup`,
  `scrolllinedown`, `scrollbycurrenteventwheeldelta`,
  `scrolltopreviousprompt`, and `scrolltonextprompt` dispatch the
  corresponding no-argument commands. The structured queries
  `scroll by page <amount>`/`scrollbypage <amount>`,
  `scroll by line <amount>`/`scrollbyline <amount>`, and
  `scroll to prompt <amount>`/`scrolltoprompt <amount>` dispatch arbitrary
  signed native payloads, with WezTerm-style
  `wezterm.action.ScrollByPage(<amount>)` and
  `wezterm.action.ScrollByLine(<amount>)` function-call queries dispatching
  the same signed page-scroll and line-scroll payloads, and
  `wezterm.action.ScrollToPrompt(<amount>)` function-call queries dispatching
  signed prompt-relative payloads.
  Native `KEY_ASSIGNMENTS` launcher results now also list WezTerm's default
  `Shift+PageUp`/`Shift+PageDown` page-scroll bindings as
  `ScrollByPage(-1)` and `ScrollByPage(1)`.
- Terminal core now records OSC 133 Prompt/Input/Output semantic zones and can
  query zones by retained row/column.
- Terminal core now records WezTerm shell-integration OSC 133 `D`
  command-finished metadata, including the retained row, exit status, and
  `aid`.
- Terminal core now records OSC 7 and iTerm2 `OSC 1337;CurrentDir` current
  working directory metadata; app-shell launch metadata syncs it per pane,
  including inactive panes, falls back to local session process tree cwd when
  the PTY backend exposes a pid, prefers child processes over the session root,
  inherits it for new tabs/splits, and decodes `file://` cwd URIs before PTY
  spawn.
- Terminal core now base64-decodes iTerm2/WezTerm `OSC 1337;SetUserVar`
  metadata into terminal user vars; app-shell syncs those values per pane for
  active and inactive pane runtimes, and the native window dispatches a typed
  user-var change hook with the window id, pane id, name, and value when a pane
  value changes.
- Terminal core now base64-decodes iTerm2 `OSC 1337;SetBadgeFormat` metadata
  into terminal badge format state; app-shell syncs that value per pane for
  active and inactive pane runtimes, and native rendering displays non-empty
  badge text as a pane-local top-right overlay with `\(user.NAME)` values
  interpolated from pane user vars, `\(iterm2.pid)` interpolated from the
  current app process id, `\(iterm2.localhostName)` from the local host name,
  `\(iterm2.effectiveTheme)`, `\(tab.iterm2.effectiveTheme)`,
  `\(tab.window.iterm2.effectiveTheme)`, and
  `\(tab.window.currentTab.iterm2.effectiveTheme)` as `dark` for the current
  fixed dark native UI,
  `\(tab.window.id)`/`\(tab.window.number)`/`\(tab.window.frame)`/
  `\(tab.window.style)`/
  `\(tab.window.isHotkeyWindow)`/
  `\(tab.window.titleOverrideFormat)`/`\(tab.window.titleOverride)`/
  `\(tab.window.currentTab.id)`/`\(tab.window.currentTab.title)`/
  `\(tab.window.currentTab.titleOverrideFormat)`/
  `\(tab.window.currentTab.titleOverride)`/
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
  `\(tab.window.currentTab.currentSession.selectionLength)`/`\(tab.id)`/
  `\(tab.title)`/`\(tab.titleOverrideFormat)`/`\(tab.titleOverride)`/
  `\(tab.currentSession.id)`/
  `\(tab.currentSession.pid)`/`\(tab.currentSession.jobPid)`/
  `\(tab.currentSession.tty)`/
  `\(tab.currentSession.autoName)`/
  `\(tab.currentSession.autoNameFormat)`/`\(tab.currentSession.name)`/
  `\(tab.currentSession.presentationName)`/
  `\(tab.currentSession.jobName)`/`\(tab.currentSession.processTitle)`/
  `\(tab.currentSession.commandLine)`/
  `\(tab.currentSession.lastCommand)`/
  `\(tab.currentSession.homeDirectory)`/
  `\(tab.currentSession.sshIntegrationLevel)`/
  `\(tab.currentSession.username)`/`\(tab.currentSession.hostname)`/
  `\(tab.currentSession.shell)`/`\(tab.currentSession.uname)`/
  `\(tab.currentSession.path)`/`\(tab.currentSession.profileName)`/
  `\(tab.currentSession.terminalIconName)`/
  `\(tab.currentSession.terminalWindowName)`/
  `\(tab.currentSession.applicationKeypad)`/
  `\(tab.currentSession.bellCount)`/
  `\(tab.currentSession.mouseReportingMode)`/
  `\(tab.currentSession.mouseInfo)`/
  `\(tab.currentSession.mouseInfo[0/1/2/3/4/5/6])`/
  `\(tab.currentSession.columns)`/`\(tab.currentSession.rows)`/
  `\(tab.currentSession.selection)`/
  `\(tab.currentSession.selectionLength)`
  interpolated from the native window id/number, latest native window frame
  `[x, y, width, height]`, current normal/full-screen window style,
  non-hotkey window state, active tab id/title/explicit tab title,
  active tab current pane
  id/title/launch program/command line/path/profile name/OSC 1 icon title/OSC 2
  window title, active tab id/title/explicit tab title, and active tab current
  pane id/title/session name/auto-name from OSC 1 icon title or profile
  name/launch program/command line/local home directory/
  SSH-integration level/local user/host/shell/uname/current working directory/
  profile name/OSC 1 icon title/OSC 2 window title/keypad state/BEL count/mouse
  reporting mode/latest mouse-info array and indexed values/size/selection,
  plus `\(session.id)`, `\(session.termid)`, `\(session.pid)`,
  `\(session.jobPid)`, `\(session.tty)`, `\(session.autoName)`,
  `\(session.autoNameFormat)`, `\(session.name)`,
  `\(session.presentationName)`, `\(session.jobName)`, `\(session.processTitle)`,
  `\(session.commandLine)`, `\(session.lastCommand)`, `\(session.homeDirectory)`,
  `\(session.profileName)`,
  `\(session.sshIntegrationLevel)`, `\(session.username)`,
  `\(session.hostname)`, `\(session.shell)`, `\(session.uname)`, `\(session.path)`,
  `\(session.terminalIconName)`, and `\(session.terminalWindowName)` values
  interpolated from the app-shell pane id, current window/tab/pane identifiers,
  with the same termid value injected into spawned PTY children as
  `TERM_SESSION_ID`, live PTY child process id, live PTY child process id, PTY name when exposed,
  auto-name and auto-name-format from OSC 1 icon title or loaded profile name,
  pane title/session name, pane title/session title,
  pane launch program, pane launch program, pane launch program plus args,
  most recent OSC 133 shell-integration input command, local host home
  directory, loaded TOML profile name from `RSSH_PROFILE` when present, native/local
  no-SSH-integration level `0`, local host user name, local host name, local
  host shell, local host OS/architecture description, current working directory,
  OSC 1 icon title, and OSC 2 window title.
  `\(session.columns)` and
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
  source-rectangle aspect ratio. Basic `a=q` support queries
  return `OK`/`EINVAL` for supported direct, regular-file, and temporary-file
  payloads, including single-block and chunked direct payloads, without
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
  allowed up to eight parent levels and deeper chains return `ETOODEEP`;
  relative placements whose parent is a `U=1` virtual placement derive the
  parent origin from the minimum row/column of all matching Unicode placeholder
  renders before applying `H`/`V`. Basic
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
  `?80l` output starts at the text cursor and advances below the image while
  preserving the left-edge column; xterm/WezTerm `?8452h` moves that post-Sixel
  cursor to the right edge. DECSDM `?80h` output starts at the active
  graphics-page origin and keeps the text cursor fixed. `?80` and `?8452` are
  reported through DECRQM/DECRPM, WezTerm's tmux-control `DCS 1000 q` is
  ignored instead of being classified as Sixel, and the image renders through
  the same snapshot path. Native windows advance the renderer animation clock
  before each framebuffer render so elapsed-time GIF frames refresh across
  redraws. Kitty shared-memory transfers,
  remaining richer placement controls, broader query responses beyond current
  direct/local-file payload validation and stored-image existence checks, full
  Sixel protocol coverage, sixel scrolling/pan edge cases, and pane sync remain
  open.
- App runtime now extracts WezTerm-documented OSC 9 and OSC 777 `notify`
  notification events from ESC plus UTF-8 C1 OSC/ST active and inactive pane
  output, with legacy raw C1 compatibility, and dispatches them through the
  native-window notification handler. Native per-window
  `notification_handling` defaults to `AlwaysShow` and supports `NeverShow`,
  `SuppressFromFocusedPane`, `SuppressFromFocusedTab`, and
  `SuppressFromFocusedWindow` before handler dispatch and latest-notification
  title updates. The native window title now shows the latest notification as
  `Notification: ...`. Static WezTerm-style Lua
  `config.notification_handling` snippets now parse into the same native
  override path. Console output filtering also consumes OSC 9
  notification/progress controls and OSC 777 notify controls so
  local sessions do not leak them to the host console; native OS toast
  integration remains open.
- App runtime now records WezTerm-documented ConEmu-style OSC 9;4 progress
  state as None, percentage, error, or indeterminate from ESC plus UTF-8 C1
  OSC/ST forms, does not treat progress reports as OSC 9 notifications, and
  syncs active/inactive pane progress into app-shell pane metadata. The native
  tab bar now marks active-pane progress as `N%`, `err:N%`, or `~`; Lua pane API
  exposure remains open.
- Native window now dispatches typed bell hooks with the window id and
  originating pane id for ASCII BEL from active and inactive pane output while
  preserving bell metrics. Native per-window `audible_bell` overrides support
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
  behavior. Static WezTerm-style Lua `config.audible_bell`,
  `config.visual_bell`, `config.colors.foreground`, `config.colors.background`,
  `config.colors.selection_fg`, `config.colors.selection_bg`,
  `config.colors.cursor_bg`, `config.colors.cursor_border`,
  `config.colors.cursor_fg`, and
  `config.colors.visual_bell` snippets now parse into
  the same native override path, including bracketed string table keys with
  long-bracket values for visual-bell fields, nested `CubicBezier` easing
  tables, and `colors.visual_bell`. Lua event wiring remains open.
- Native window now dispatches a typed focus-change hook with the window id,
  active pane id, and focused/unfocused state while preserving CSI
  focus-reporting writes.
  Lua event wiring remains open.
- Native window now dispatches a typed resize hook after successful terminal and
  PTY resize or fullscreen/windowed transitions with the window id, active pane
  id, pixel size, terminal rows/columns, and `is_full_screen` state so native
  handlers see the same fullscreen dimension metadata exposed by WezTerm's
  window dimensions APIs. Lua event wiring remains open.
- Native window now dispatches a typed `window-config-reloaded` hook for
  command-palette `ReloadConfiguration` and the default `Ctrl+Shift+R`
  shortcut, carrying the window id and active pane id. A typed native
  `set_config_overrides`/`get_config_overrides` subset stores
  per-window overrides for `tab_max_width`, `status_update_interval`,
  `max_fps`, `animation_fps`, `front_end`, `webgpu_power_preference`, `webgpu_force_fallback_adapter`, `webgpu_preferred_adapter`, `prefer_egl`, `enable_wayland`, `cursor_blink_rate`, `cursor_blink_ease_in`, `cursor_blink_ease_out`,
  `text_blink_rate`, `text_blink_rate_rapid`, `text_blink_ease_in`,
  `text_blink_ease_out`, `text_blink_rapid_ease_in`,
  `text_blink_rapid_ease_out`,
  `font_size`, `cell_width`, `cell_widths`, `line_height`, `font_antialias`, `font_hinting`, `font_rasterizer`, `font_shaper`, `custom_block_glyphs`, `anti_alias_custom_block_glyphs`, `allow_square_glyphs_to_overflow_width`, `freetype_load_target`, `freetype_render_target`, `freetype_load_flags`, `freetype_interpreter_version`, `freetype_pcf_long_family_names`, `display_pixel_geometry`, `default_cursor_style`, `cursor_thickness`, `underline_thickness`, `underline_position`, `strikethrough_position`, `force_reverse_video_cursor`,
  `initial_cols`, `initial_rows`, `adjust_window_size_when_changing_font_size`,
  `command_palette_rows`, `command_palette_bg_color`, `command_palette_fg_color`, `char_select_bg_color`, `char_select_fg_color`, `launcher_alphabet`, `quick_select_alphabet`, `quick_select_patterns`,
  `disable_default_quick_select_patterns`, `quick_select_remove_styling`, `selection_word_boundary`,
  `term`, `audible_bell`, `visual_bell`, `foreground_color`, `background_color`, `ansi_palette`, `indexed_palette`, `cursor_bg_color`, `cursor_border_color`, `cursor_fg_color`, `compose_cursor_color`, `visual_bell_color`, `notification_handling`, `default_prog`, `default_domain`, `default_workspace`, `automatically_reload_config`, `check_for_updates`, `check_for_updates_interval_seconds`, `show_update_window`, `max_fps`, `animation_fps`, `use_resize_increments`, `debug_key_events`, `log_unknown_escape_sequences`, `warn_about_missing_glyphs`, `default_cwd`, `detect_password_input`, `set_environment_variables`,
  `key_map_preference`, `ui_key_cap_rendering`, `swap_backspace_and_delete`,
  `enable_csi_u_key_encoding`, `enable_kitty_keyboard`, `allow_win32_input_mode`, `treat_left_ctrlalt_as_altgr`, `treat_east_asian_ambiguous_width_as_wide`, `normalize_output_to_unicode_nfc`, `use_ime`, `ime_preedit_rendering`, `xim_im_name`, `scroll_to_bottom_on_input`,
  `alternate_buffer_wheel_scroll_speed`,
  `canonicalize_pasted_newlines`, `quote_dropped_files`,
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
  `show_update_window`, `max_fps`, `animation_fps`, `use_resize_increments`, `debug_key_events`, and
  `log_unknown_escape_sequences` are retained in effective config snapshots.
  Static Lua config parsing covers the paste/drop/default-input subset:
  `canonicalize_pasted_newlines`, `quote_dropped_files`,
  `disable_default_key_bindings`, `disable_default_mouse_bindings`, and
  `hide_mouse_cursor_when_typing`, `detect_password_input`, plus the implemented tab-bar boolean
  subset from `enable_tab_bar` through `show_tabs_in_tab_bar`, and the
  diagnostic/reload/update/frame-rate fields `automatically_reload_config`,
  `check_for_updates`, `check_for_updates_interval_seconds`,
  `show_update_window`, `max_fps`, `animation_fps`, `use_resize_increments`, `debug_key_events`,
  `log_unknown_escape_sequences`, and `warn_about_missing_glyphs`, plus
  the palette/quick-select/status/selection subset:
  `status_update_interval`, `command_palette_rows`, `command_palette_bg_color`,
  `command_palette_fg_color`, `char_select_bg_color`, `char_select_fg_color`,
  `launcher_alphabet`,
  `quick_select_alphabet`, `quick_select_patterns`,
  `disable_default_quick_select_patterns`, `quick_select_remove_styling`, and
  `selection_word_boundary`, plus the implemented font/window/cursor subset:
  `font_size`, `cell_width`, `cell_widths`, `line_height`, `initial_cols`, `initial_rows`,
  `adjust_window_size_when_changing_font_size`, `cursor_blink_rate`,
  `cursor_blink_ease_in`, `cursor_blink_ease_out`, `default_cursor_style`, and
  `force_reverse_video_cursor`, `window_decorations`, plus the implemented bell/notification subset:
  `audible_bell`, `visual_bell`, `colors.foreground`, `colors.background`, `colors.cursor_bg`, `colors.cursor_border`, `colors.cursor_fg`, `colors.compose_cursor`, `colors.visual_bell`, and
  `notification_handling`, plus the implemented render color subset:
  `foreground_text_hsb`, `inactive_pane_hsb`, `bold_brightens_ansi_colors`,
  `text_background_opacity`, `window_background_opacity`, `colors.foreground`, `colors.background`, `colors.ansi`, `colors.brights`, `colors.indexed`, `colors.selection_fg`, `colors.selection_bg`, `colors.cursor_bg`, `colors.cursor_border`, `colors.cursor_fg`, `colors.compose_cursor`, plus
  `window_close_confirmation` and
  `skip_close_confirmation_for_processes_named`.
  `warn_about_missing_glyphs` is retained with WezTerm's default `true`.
  Unknown ESC/CSI sequences are recorded by the terminal runtime and emitted
  as native stderr warnings when `log_unknown_escape_sequences` is enabled.
  Native key events are emitted as stderr `INFO key_event` diagnostics when
  `debug_key_events` is enabled. Missing glyph codepoints detected in rendered
  cells are emitted once per native window as stderr `CONFIG ERROR missing
  glyph ...` diagnostics when `warn_about_missing_glyphs` is enabled.
  `use_resize_increments` applies current cell-size resize hints on
  X11/Wayland/macOS-capable builds and remains a WezTerm-style no-op on
  unsupported platforms. Lua event wiring, full WezTerm-style configuration
  error window UI, actual Lua config reload, automatic file watching, Lua
  `window:set_config_overrides` wiring, and broader config option coverage
  remain open.
- Native command palette now dispatches a typed `augment-command-palette` hook
  when opened, carrying the window id and active pane id. Returned entries can
  add native `WindowCommand` actions to the same fuzzy-filtered palette list,
  and optional entry `doc` text plus known Nerd Font `icon` names, including
  `md_rename_box`, `fa_clock_o`, and `cod_github`, are rendered beside the brief
  label. Lua event wiring, arbitrary Lua callbacks, full Nerd Font icon catalog
  coverage, and full action-value parity remain open.
- Native tab bar rendering now dispatches a typed `format-tab-title` hook after
  computing the default tab title. The event carries the default title, tab id,
  active pane id, tab index, tab count, active-tab pane count, active state, and
  last-active state plus current tab-bar hover state and `max_width`, and runs
  in WezTerm-style two passes: first with `hover=false` and the WezTerm-default
  16-cell `tab_max_width`, then with the computed hover state and an
  available-space title width. Returning a string
  overrides the displayed title, and native Text/Foreground/Background format items can style the title segment;
  native Text items consume embedded SGR presentation escapes including
  blink/inverse/conceal/strikethrough/overline while layout uses only their
  visible text, native ResetAttributes restores the tab segment style,
  native Intensity
  Normal/Bold/Half toggles tab-title bold/faint rendering, native Italic
  true/false toggles tab-title italic rendering, native Underline
  None/Single/Double/Curly/Dotted/Dashed maps to tab-title underline style, and
  returning `None` keeps the default. The typed event also carries
  TabInformation/PaneInformation-style snapshots with window id/title, all tabs
  in the window, explicit tab title, current-tab active pane and pane entries,
  plus active-tab pane entries for the top-level `panes` parameter. Pane
  snapshots include geometry, titles, foreground process name, current working
  directory, unseen-output state, local domain name, tty name when known, user
  vars, and progress. The typed event carries an effective config snapshot for
  implemented window options including `tab_max_width`,
  `status_update_interval`, `cursor_blink_rate`, `cursor_blink_ease_in`,
  `cursor_blink_ease_out`, `text_blink_rate`, `text_blink_rate_rapid`,
  `text_blink_ease_in`, `text_blink_ease_out`,
  `text_blink_rapid_ease_in`, `text_blink_rapid_ease_out`, `font_size`, `cell_width`, `cell_widths`, `line_height`, `font_antialias`, `font_hinting`, `font_rasterizer`, `font_shaper`, `custom_block_glyphs`, `anti_alias_custom_block_glyphs`, `allow_square_glyphs_to_overflow_width`, `freetype_load_target`, `freetype_render_target`, `freetype_load_flags`, `freetype_interpreter_version`, `freetype_pcf_long_family_names`, `display_pixel_geometry`, `foreground_text_hsb`, `bold_brightens_ansi_colors`, `text_background_opacity`, `window_background_opacity`, `window_decorations`, `default_cursor_style`, `cursor_thickness`, `underline_thickness`, `underline_position`, `strikethrough_position`, `force_reverse_video_cursor`, `window_content_alignment`, `initial_cols`, `initial_rows`, `adjust_window_size_when_changing_font_size`, `inactive_pane_hsb`, `command_palette_rows`, `command_palette_bg_color`, `command_palette_fg_color`, `char_select_bg_color`, `char_select_fg_color`, `launcher_alphabet`, `quick_select_alphabet`, `quick_select_patterns`, `disable_default_quick_select_patterns`, `quick_select_remove_styling`, `selection_word_boundary`, `term`, `audible_bell`, `visual_bell`, `foreground_color`, `background_color`, `ansi_palette`, `indexed_palette`, `selection_fg_color`, `selection_bg_color`, `cursor_bg_color`, `cursor_border_color`, `cursor_fg_color`, `compose_cursor_color`, `visual_bell_color`, `notification_handling`, `default_prog`, `default_domain`, `default_workspace`, `automatically_reload_config`, `check_for_updates`, `check_for_updates_interval_seconds`, `show_update_window`, `max_fps`, `animation_fps`, `use_resize_increments`, `debug_key_events`, `log_unknown_escape_sequences`, `warn_about_missing_glyphs`, `default_cwd`, `detect_password_input`, `set_environment_variables`, `key_map_preference`, `swap_backspace_and_delete`, `enable_csi_u_key_encoding`, `enable_kitty_keyboard`, `allow_win32_input_mode`, `treat_left_ctrlalt_as_altgr`, `treat_east_asian_ambiguous_width_as_wide`, `normalize_output_to_unicode_nfc`, `use_ime`, `ime_preedit_rendering`, `xim_im_name`, `scroll_to_bottom_on_input`, `alternate_buffer_wheel_scroll_speed`, `canonicalize_pasted_newlines`, `quote_dropped_files`, `disable_default_key_bindings`, `disable_default_mouse_bindings`, `hide_mouse_cursor_when_typing`, `pane_focus_follows_mouse`, `swallow_mouse_click_on_pane_focus`, `swallow_mouse_click_on_window_focus`, `bypass_mouse_reporting_modifiers`, `enable_scroll_bar`, `min_scroll_bar_height`, `enable_tab_bar`,
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
  `show_close_tab_button_in_tabs`, `show_new_tab_button_in_tab_bar`,
  `show_tab_index_in_tab_bar`, and `show_tabs_in_tab_bar`; Lua event
  wiring plus the full Lua config object remain open.
- Native window now dispatches a typed `format-window-title` hook after
  computing the default title. The event carries the default title, active tab
  id, active pane id, tab count, active-tab pane count, and the active
  key-table stack top plus
  TabInformation/PaneInformation-style snapshots for the active tab, active
  pane, all tabs in the window, and panes in the active tab. Returning a string
  overrides the native title; returning `None` keeps the default. The typed
  event carries the same effective config snapshot; Lua event wiring plus the
  full Lua config object remain open.
- Native window now dispatches typed `update-status` and deprecated
  `update-right-status` hooks from the native event loop with the window id and
  active pane id, scheduled by a WezTerm-style 1000ms
  `status_update_interval` default. The
  handlers can update stored left and right status strings; the native tab bar
  renders left status after the workspace label, consumes SGR presentation
  escapes including blink/inverse/conceal/strikethrough/overline plus WezTerm
  underline style variants and ANSI/indexed/RGB
  foreground/background/underline color escapes in status strings, computes
  status layout from visible text, and right-aligns right status at the window
  edge, clipping over-wide right status from the left.
  Native `set_left_status` and `set_right_status` methods update the same
  tab-bar state directly. Static WezTerm-style Lua
  `config.status_update_interval` snippets now parse into the same native
  update interval override path. Lua `window:set_left_status` /
  `set_right_status` wiring remains open.
- Native window now dispatches a typed `new-tab-button-click` hook for
  Left/Right/Middle clicks on the tab bar `+` button, carrying the window id and
  active pane id. Left click carries the default `NewTab` action in the event
  payload, while Right/Middle clicks have no default action; returning `false`
  suppresses any default action. Lua event wiring remains open.
- Native window now dispatches a typed open-uri hook for ctrl-clicked OSC 8
  hyperlinks before invoking the default opener, carrying the window id, active
  pane id, and URI. Returning `false` suppresses the default opener. The command
  palette now exposes WezTerm-style `CompleteSelection`,
  `OpenLinkAtMouseCursor`, and `CompleteSelectionOrOpenLinkAtMouseCursor`,
  completing active mouse selections into ClipboardAndPrimarySelection or
  opening the OSC 8 link under the mouse through the same open-uri hook.
  Structured `completeselection`, `openlinkatmousecursor`, and
  `completeselectionoropenlinkatmousecursor` action-name queries resolve to the
  same native behavior. Native
  `CompleteSelectionTo(destination)` and
  `CompleteSelectionOrOpenLinkAtMouseCursorTo(destination)` payloads complete
  active selections into a specific implemented copy destination, and structured
  command-palette queries now accept `complete selection to <destination>`,
  `completeselectionto <destination>`,
  `complete selection open link to <destination>`, and
  `completeselectionoropenlinkatmousecursorto <destination>` for quoted or
  unquoted `Clipboard`, `PrimarySelection`, or `ClipboardAndPrimarySelection`;
  WezTerm-style `wezterm.action.CompleteSelection '<destination>'` and
  `wezterm.action.CompleteSelectionOrOpenLinkAtMouseCursor '<destination>'`
  Lua action queries dispatch the same destination-specific payloads.
  Lua event wiring remains open.
- Terminal core now aligns OSC 8 hyperlink reset behavior with WezTerm: SGR
  reset preserves the active hyperlink, and an empty OSC 8 URI clears it.
- Terminal core can extract text from retained row/column regions and semantic
  zones while unwrapping soft-wrapped physical rows to logical-line text.
- Command palette now exposes WezTerm-style `ActivateCopyMode` as Activate Copy
  Mode, and native `WindowCommand::ActivateCopyMode` payloads enter the same
  copy-mode path. Structured `activatecopymode` and `entercopymode` action-name
  queries resolve to the same copy-mode paths. The default `Ctrl+Shift+X`
  key-assignment entry now exposes that WezTerm-style payload while the older
  native `EnterCopyMode` alias remains accepted.
- Native `CopyMode` assignment payloads now parse WezTerm-style
  `wezterm.action.CopyMode 'MoveToStartOfLine'`, function-call, and command
  query forms for implemented copy-mode movement, selection-end, selection-mode
  clearing, and close actions, so native key-table assignments can trigger the
  same copy-mode helpers. WezTerm-style
  `wezterm.action.CopyMode { SetSelectionMode = 'Block' }` table forms now
  route the documented `Cell`, `Word`, `Line`, `Block`, and `SemanticZone`
  selection modes, including bracketed string table keys with long-bracket
  values. Search
  `NextMatch`, `PriorMatch`, `NextMatchPage`, `PriorMatchPage`, `ClearPattern`,
  and `CycleMatchType` assignment values dispatch the same copy-mode search
  helpers as the default key table. `PageUp`, `PageDown`, and
  `MoveByPage = +/-0.5` assignment values dispatch the same copy-mode page
  movement path as the default copy-mode keys. `JumpAgain` and `JumpReverse`
  assignment values reuse the same repeat/reverse jump path as `;` and `,`.
  `JumpForward = { prev_char = ... }` and
  `JumpBackward = { prev_char = ... }` assignment tables start the same
  target-character jump flow as `f`/`t`/`F`/`T`, including bracketed string
  table keys with long-bracket values.
  `MoveForwardSemanticZoneOfType = 'Input'` and
  `MoveBackwardSemanticZoneOfType = 'Prompt'` assignment values reuse the
  typed OSC 133 semantic-zone movement path.
  Single-name Lua table forms such as
  `wezterm.action.CopyMode { 'ClearSelectionMode' }` now reuse the same
  assignment parser. `AcceptPattern` and `EditPattern` now toggle whether
  typed copy-mode search input edits the current search pattern. WezTerm-style
  static Lua snippets for `config.keys`, `config.key_tables`, and
  `config.leader` now parse the implemented native assignment subset and
  leader configuration into runtime key-table overrides, including bracketed
  string table keys with long-bracket values for key-table names and nested
  assignment fields; full Lua config evaluation, default key-table merging, and
  config-file reload wiring remain open.
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
- Mouse word selection now honors WezTerm-style `selection_word_boundary`,
  including the documented default boundary set and per-window overrides. Static
  WezTerm-style Lua `config.selection_word_boundary` snippets now parse into
  the same native override path.
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
- Default `Ctrl+Shift+F`/`Super+F` search shortcuts now expose WezTerm-style
  `Search(CaseSensitiveString="")`, while command-palette Search exposes the
  same overlay with search table navigation via Down/Up, `Ctrl+N`/`Ctrl+P`,
  PageDown/PageUp, `Ctrl+R` match-type cycling, `Ctrl+U` clear-pattern, and
  character ESC close. Command-palette `search <pattern>`,
  `search regex <pattern>`, `search case-sensitive <pattern>`, and
  `search case-insensitive <pattern>` / `search case insensitive <pattern>`
  queries open Search with that initial typed pattern using quote-aware parsing.
  WezTerm-style action field names `search casesensitivestring <pattern>` and
  `search caseinsensitivestring <pattern>` dispatch the same typed payloads,
  and `search current selection or empty string` maps to WezTerm-style
  `CurrentSelectionOrEmptyString`. Native `Search` action payloads now support
  typed `Regex`, `CaseSensitiveString`, and `CaseInSensitiveString` patterns
  through the same search path, plus
  `CurrentSelectionOrEmptyString` to reuse the selected text collapsed to a
  single line or open an empty search overlay when nothing is selected.
  WezTerm-style `wezterm.action.Search { Regex = ... }`,
  `CaseSensitiveString`, `CaseInSensitiveString`, and
  bracketed string table keys with long-bracket values such as
  `wezterm.action.Search { [[=[Regex]=]] = [[\d+]] }`, plus
  `wezterm.action.Search("CurrentSelectionOrEmptyString")` plus
  `wezterm.action.Search 'CurrentSelectionOrEmptyString'` Lua action queries
  dispatch the same native payload subset. Plain `Ctrl+F` stays available to
  the PTY.
- Command palette now exposes Activate Last Tab backed by app-shell last-active
  tab state.
- App-shell close-tab state handling now supports WezTerm's
  `switch_to_last_active_tab_when_closing_tab` selection policy, and native tab
  bar close markers, default close-tab shortcuts, and Close Current Tab
  command/confirmation paths honor the same effective-config option.
- Native window renders basic right/down pane splits from app-shell split state,
  including per-pane snapshot placement and split separators.
- Native split rendering applies WezTerm-style `inactive_pane_hsb` defaults
  (`saturation = 0.9`, `brightness = 0.8`) and native overrides to inactive
  pane Default/Indexed/RGB/RGBA cell colors while leaving the active pane
  unchanged. Static WezTerm-style Lua `config.inactive_pane_hsb` snippets now
  parse into the same native override path, including bracketed string table
  keys with long-bracket values; dynamic palette-aware resolution for this
  option remains later parity work.
- Native terminal rendering applies WezTerm-style `foreground_text_hsb`
  overrides to foreground and underline Default/Indexed/RGB/RGBA cell colors
  while preserving background colors. Static WezTerm-style Lua
  `config.foreground_text_hsb` snippets now parse into the same native override
  path, including bracketed string table keys with long-bracket values; dynamic
  palette-aware resolution for this option remains later parity work.
- Native terminal rendering applies WezTerm-style `bold_brightens_ansi_colors`
  to bold ANSI 0-7 foreground colors, defaulting to `BrightAndBold` and
  supporting `No` plus `BrightOnly`. Static WezTerm-style Lua
  `config.bold_brightens_ansi_colors` snippets now parse into the same native
  override path, including configured `colors.ansi`/`colors.brights` palette
  slots.
- Native terminal rendering applies WezTerm-style `text_background_opacity`
  overrides to non-default cell backgrounds while preserving default
  backgrounds. Static WezTerm-style Lua `config.text_background_opacity`
  snippets now parse into the same native override path; dynamic palette-aware
  resolution for this option remains later parity work.
- Native terminal rendering applies WezTerm-style `window_background_opacity`
  overrides to default cell backgrounds while preserving explicit text
  backgrounds. Static WezTerm-style Lua `config.window_background_opacity`
  snippets now parse into the same native override path; dynamic palette-aware
  resolution for this option remains later parity work.
- Native terminal rendering applies WezTerm-style `colors.background` as the
  default framebuffer background for full and damage renders. Static
  WezTerm-style Lua `config.colors.background` snippets now parse into the same
  native override path for SVG/CSS3 color names, `#RRGGBB`, and CSS-style
  `rgb(...)` color strings with comma or space separators, percentage channels,
  ignored non-selection alpha, plus WezTerm `hsl:` and CSS-style
  `hsl(...)`/`hsla(...)`/`hwb(...)`/`hsv(...)` color strings; copy-mode, split,
  scrollbar, compose, tab-bar, and other non-terminal color fields remain later
  parity work.
- Native terminal rendering applies WezTerm-style `colors.foreground` as the
  default text foreground for full and damage renders. Static WezTerm-style Lua
  `config.colors.foreground` snippets now parse into the same native override
  path for SVG/CSS3 color names, `#RRGGBB`, and CSS-style `rgb(...)` color
  strings with comma or space separators, percentage channels, ignored
  non-selection alpha, plus WezTerm `hsl:` and CSS-style
  `hsl(...)`/`hsla(...)`/`hwb(...)`/`hsv(...)` color strings; copy-mode, split,
  scrollbar, compose, tab-bar, and other
  non-terminal color fields remain later parity work.
- Native terminal rendering applies WezTerm-style `colors.ansi` and
  `colors.brights` as the ANSI 0-15 palette for foreground, background,
  underline, bold-brightening, and force-reverse cursor color resolution.
  Static WezTerm-style Lua `config.colors.ansi` and `config.colors.brights`
  snippets now parse into the same native override path for SVG/CSS3 color
  names, `#RRGGBB`, and the implemented CSS-style
  `rgb(...)`/`hsl(...)`/`hsla(...)`/`hwb(...)`/`hsv(...)` color-string subset.
- Native terminal rendering applies WezTerm-style `colors.indexed` entries for
  indexed palette slots 16-255 for foreground, background, underline, and
  force-reverse cursor color resolution. Static WezTerm-style Lua
  `config.colors.indexed = { [136] = '#af8700' }` snippets now parse into the
  same native override path, including SVG/CSS3 color names and the implemented
  CSS-style `rgb(...)`/`hsl(...)`/`hsla(...)`/`hwb(...)`/`hsv(...)`
  color-string subset; unspecified indexed entries continue to use the xterm
  256-color cube/grayscale mapping.
- Native terminal selection rendering applies WezTerm-style
  `colors.selection_fg` and `colors.selection_bg` to selected cells. Static
  WezTerm-style Lua `config.colors.selection_fg` and
  `config.colors.selection_bg` snippets now parse into the same native override
  path for SVG/CSS3 color names, `#RRGGBB`, and the implemented CSS-style
  `rgb(...)`/`hsl(...)`/`hsla(...)`/`hwb(...)`/`hsv(...)` subset, including
  slash alpha for `selection_bg`, `selection_fg = 'none'` preserving the
  current cell foreground, plus `selection_bg = 'rgba(r,g,b,a)'`,
  `selection_bg = 'hsla(h,s,l,a)'`, `hwb(h w b / a)`, and `hsv(h s v / a)`
  alpha blending over the current cell background. Copy-mode/quick-select
  label/match `Color`/`AnsiColor` tables now parse into native/effective config,
  `copy_mode_active_highlight_bg`/`copy_mode_active_highlight_fg` apply to
  copy-mode selections, `copy_mode_inactive_highlight_bg`/
  `copy_mode_inactive_highlight_fg` apply to non-current copy-mode search
  matches, `quick_select_label_bg`/`quick_select_label_fg` plus
  `quick_select_match_bg`/`quick_select_match_fg` apply to quick-select label
  and match rendering, and `input_selector_label_bg`/
  `input_selector_label_fg` plus `launcher_label_bg`/`launcher_label_fg` now
  parse into native/effective config and apply to default-mode selector/
  launcher shortcut labels.
- Native cursor rendering applies WezTerm-style `colors.cursor_bg` as the
  default block cursor fill, `colors.cursor_border` as block-cursor border and
  bar/underline cursor color, and `colors.cursor_fg` as block-cursor text
  foreground unless OSC cursor color or `force_reverse_video_cursor` takes
  precedence. Static WezTerm-style Lua `config.colors.cursor_bg`,
  `config.colors.cursor_border`, and `config.colors.cursor_fg` snippets now
  parse into the same native override path; `colors.split` also parses into
  native/effective config and applies to pane separator rendering;
  `colors.scrollbar_thumb` parses into native/effective config and applies to
  scrollbar thumb rendering; `colors.tab_bar.background` parses into
  native/effective config and applies to blank retro tab-bar cells; retro
  tab-bar `active_tab`, `inactive_tab`, `inactive_tab_hover`, `new_tab`, and
  `new_tab_hover` `fg_color`/`bg_color` plus
  `intensity`/`underline`/`italic`/`strikethrough` entries parse into
  native/effective config and apply to tab/new-tab labels; `tab_bar_style`
  active/inactive/inactive-hover/new-tab/new-tab-hover left/right edge
  entries parse static `wezterm.format` item arrays and apply to retro
  tab/new-tab labels;
  top-level `command_palette_bg_color`/`command_palette_fg_color` parse into
  native/effective config and apply to normal command-palette candidate rows;
  top-level `char_select_bg_color`/`char_select_fg_color` parse into
  native/effective config and apply to normal Char Select candidate rows;
  copy-mode/quick-select/input-selector/launcher label `Color`/`AnsiColor`
  tables also parse into native/effective config, and copy-mode
  active/inactive highlight plus quick-select label/match colors apply to
  overlay rendering, while input-selector/launcher label colors apply to
  default-mode shortcut labels. Native/fancy titlebar styling and other
  non-terminal color fields remain later parity work.
- Native window creation parses WezTerm-style `window_decorations` flags and
  maps `NONE` to a borderless winit window while retaining `TITLE`/`RESIZE` and
  macOS-specific flags in effective config snapshots. Fine-grained titlebar,
  resize-border, integrated button, and macOS shadow/corner behavior remain
  later OS-specific parity work.
- Native terminal rendering applies WezTerm-style `text_blink_rate`,
  `text_blink_rate_rapid`, `text_blink_ease_in`, `text_blink_ease_out`,
  `text_blink_rapid_ease_in`, and `text_blink_rapid_ease_out` to SGR 5/6
  blinking text. Normal and rapid blink keep distinct phases, and foreground
  glyphs plus text decorations interpolate toward the rendered background.
  Static WezTerm-style Lua snippets for those fields now parse into the same
  native override path, including string easing names and
  `{ CubicBezier = { ... } }` table easing forms.
- Native terminal rendering applies WezTerm-style `underline_thickness`
  overrides to terminal text underline decorations using px, DPI-scaled pt,
  percent-of-default, and cell-fraction units. Horizontal split dividers use
  the same thickness and `colors.split` foreground when rendered through the
  native window snapshot. Static WezTerm-style Lua
  `config.underline_thickness` snippets now parse string dimensions with units
  or bare numeric pixel values into the same native override path; font-metric
  defaults and custom-glyph line use remain later parity work.
- Native terminal rendering applies WezTerm-style `underline_position`
  overrides to terminal text underline placement using signed px, DPI-scaled
  pt, percent-of-default, and cell-fraction units against the current default
  underline-row baseline approximation. Static WezTerm-style Lua
  `config.underline_position` snippets now parse string dimensions with units
  or bare signed numeric pixel values into the same native override path; exact
  font-metric-derived baseline/default behavior remains later parity work.
- Native terminal rendering applies WezTerm-style `strikethrough_position`
  overrides to terminal text strikethrough decorations using px, DPI-scaled pt,
  percent-of-default, and cell-fraction units. Static WezTerm-style Lua
  `config.strikethrough_position` snippets now parse string dimensions with
  units or bare numeric pixel values into the same native override path;
  font-metric-derived defaults remain later parity work.
- Split panes now have pane-local mouse hit testing for click-to-focus,
  optional focus-follows-mouse via `pane_focus_follows_mouse`, and wheel scroll
  routing.
- Inactive-pane clicks preserve WezTerm's default click-through behavior, while
  `swallow_mouse_click_on_pane_focus=true` focuses the pane and consumes the
  initial click.
- Window-refocusing clicks honor `swallow_mouse_click_on_window_focus`: when
  true the focus click is consumed before pane handling; when false it passes
  through to pane mouse processing. The default follows WezTerm's platform rule,
  true on macOS and false elsewhere.
- Mouse reporting can be bypassed with the configured
  `bypass_mouse_reporting_modifiers` value. The default `SHIFT` modifier keeps
  Shift-click local for selection even when the terminal application has enabled
  mouse reporting; custom values such as `ALT` are honored by native config
  overrides. Static WezTerm-style Lua snippets for
  `config.pane_focus_follows_mouse`,
  `config.swallow_mouse_click_on_pane_focus`,
  `config.swallow_mouse_click_on_window_focus`, and
  `config.bypass_mouse_reporting_modifiers` now parse into the same native
  override path.
- Native `enable_scroll_bar` defaults to false. When true, the scrollback
  scrollbar renders and accepts click/drag input; when false, scrollback remains
  wheel/command driven without the scrollbar affordance. The thumb minimum
  defaults to WezTerm's `min_scroll_bar_height = "0.5cell"` behavior, and
  native px, DPI-scaled pt, cell, and percent unit overrides are applied to
  scrollbar rendering and hit-testing. Static Lua `enable_scroll_bar` and
  `min_scroll_bar_height` assignments parse into the same override path.
- Native `alternate_buffer_wheel_scroll_speed` defaults to WezTerm's `3`.
  In alternate screen with mouse reporting disabled, vertical wheel input
  writes repeated Up/Down arrow-key sequences to the active PTY instead of
  moving scrollback.
- Native `scrollback_lines` defaults to WezTerm's `3500` retained lines.
  Overrides are applied to active and inactive pane runtimes, including newly
  spawned panes/windows, and reducing the limit immediately prunes retained
  history while rebasing semantic markers and inline image metadata.
- Split panes support WezTerm-style `AdjustPaneSize` directional resize actions
  from `Ctrl+Shift+Alt+Arrow` and command-palette Adjust Pane Size entries.
  The command-palette queries `adjust pane size <direction> <amount>` and
  `adjustpanesize <direction> <amount>` cover arbitrary Left/Right/Up/Down
  resize amounts, and field forms such as
  `adjustpanesize direction=<direction> amount=<cells>` dispatch the same
  payload. WezTerm-style
  `wezterm.action.AdjustPaneSize { '<direction>', <cells> }` and
  `wezterm.action.AdjustPaneSize({ '<direction>', <cells> })` Lua table action
  queries plus `wezterm.action { AdjustPaneSize = { '<direction>', <cells> } }`
  table-wrapper queries dispatch the same payload. Native
  `WindowCommand::AdjustPaneSize { direction, amount }` payloads dispatch the
  same active-pane resize path with arbitrary cell amounts.
- Split panes support WezTerm-style `ActivatePaneDirection` from
  `Ctrl+Shift+Arrow` and command-palette Activate Pane Direction Left/Right/
  Up/Down/Next/Previous entries, with ambiguous candidates resolved by most
  recent pane activation. The command-palette query
  `activate pane direction <direction>` plus the action-name spelling
  `activatepanedirection <direction>` plus WezTerm-style
  `wezterm.action.ActivatePaneDirection '<direction>'` bare-string and
  `wezterm.action.ActivatePaneDirection("<direction>")` function-call queries
  plus `wezterm.action { ActivatePaneDirection = '<direction>' }` table-wrapper
  queries map Left/Right/Up/Down/Next/Prev to native direction payloads, while
  action-name `activatepaneleft`, `activatepaneright`, `activatepaneup`,
  `activatepanedown`, `nextpane`, and `previouspane` queries dispatch the
  corresponding no-argument entries. Native
  `WindowCommand::ActivatePaneDirection(direction)` payloads dispatch through
  the same Up/Down/Left/Right/Next/Previous path.
- App-shell state now exposes WezTerm-style `ActivatePaneByIndex`, and the
  command palette includes Activate Pane By Index 1..8 entries plus
  `activate pane <index>`, `activate pane by index <index>`, and
  `activatepanebyindex <index>` plus WezTerm-style
  `wezterm.action.ActivatePaneByIndex(<index>)` function-call queries for
  arbitrary zero-based current-tab pane indices, plus action-name
  `activatepane1` through `activatepane8`. Native
  `WindowCommand::ActivatePaneByIndex(index)` payloads dispatch through the
  same zero-based current-tab pane index path for arbitrary indices.
- App-shell state now exposes WezTerm-style `RotatePanes`; the command palette
  includes clockwise and counter-clockwise rotate entries, and pane identity
  rotation preserves split positions and size deltas. The command-palette query
  `rotate panes <direction>` plus the action-name spelling
  `rotatepanes <direction>` accepts quoted or unquoted
  Clockwise/CounterClockwise spellings plus WezTerm-style
  `wezterm.action.RotatePanes("<direction>")` function-call queries and maps
  them to the native payload.
  Native `WindowCommand::RotatePanes(direction)` payloads dispatch through the
  same clockwise/counter-clockwise path.
- Split separators can now be dragged with the mouse to update split sizes via
  the same app-shell resize path used by keyboard/palette resize actions.
- Split panes support WezTerm-style `TogglePaneZoomState` from `Ctrl+Shift+Z`
  and command-palette Toggle Pane Zoom State, plus explicit `SetPaneZoomState`
  command-palette zoom/unzoom actions and native
  `WindowCommand::SetPaneZoomState(bool)` payloads. The command-palette query
  `set pane zoom state true|false` / `set pane zoom state=true|false` plus the
  action-name spelling `setpanezoomstate true|false` /
  `setpanezoomstate=true|false` dispatches explicit native zoom-state payloads,
  and WezTerm-style `wezterm.action.SetPaneZoomState(true|false)`
  function-call queries dispatch the same path,
  rendering the zoomed pane across the full tab region. Action-name
  `togglepanezoomstate`, `togglepanezoom`, `zoompane`, and `unzoompane`
  queries dispatch the corresponding no-argument zoom commands. The default
  `Ctrl+Shift+Z` key-assignment entry exposes `TogglePaneZoomState`, while
  `TogglePaneZoom` remains a native compatibility alias. Directional pane
  switching honors `unzoom_on_switch_pane`: the default true unzooms before
  switching, while false leaves the pane zoomed and blocks `ActivatePaneDirection`.
- Split panes support WezTerm-style `PaneSelect` default Activate mode from the
  command palette entry `Pane Select`: pane labels use the WezTerm default
  selection alphabet and honor the native effective `quick_select_alphabet`
  value when configured; the quote-aware command-palette query
  `pane select alphabet <chars>` and its explicit-mode spelling
  `pane select activate alphabet <chars>` plus action-name `paneselect ...`
  aliases cover the native Activate plus per-action alphabet subset, and
  action-name `enterpaneselect` queries dispatch the default Activate entry.
  Selecting a label focuses that pane, and `Esc`/`Ctrl+g` exits without changing
  focus.
- Pane-select show-pane-ids mode is exposed through command-palette `Pane Select
  Show Pane IDs`, rendering labels as `label:pane_id` while preserving the
  default Activate action. The quote-aware command-palette query
  `pane select show pane ids alphabet <chars>`/`show-pane-ids alphabet <chars>`
  plus the explicit-mode spelling `pane select activate show pane ids alphabet
  <chars>`/`show-pane-ids alphabet <chars>` cover the native combined Activate,
  `show_pane_ids=true`, and per-action alphabet subset, with
  `alphabet=<chars>` assignment forms accepted for the same alphabet field. The action-name
  `enterpaneselectshowpaneids` dispatches the default show-pane-ids entry. The
  native action payload also carries `PaneSelect { mode, show_pane_ids,
  alphabet }` directly for command-palette augmentation and later config wiring,
  and structured `pane select mode <mode>` / `pane select mode=<mode>` queries with
  `[show_pane_ids true|false] [show_pane_ids=true|false] [alphabet <chars>|alphabet=<chars>]` fields plus
  action-name `paneselect ...` aliases map WezTerm-style option names to that payload while
  rejecting duplicate structured fields. WezTerm-style
  `wezterm.action.PaneSelect { mode = ..., show_pane_ids = ..., alphabet = ... }`
  and parenthesized table calls parse the same native option subset, with
  omitted `mode` defaulting to Activate, including bracketed string table keys
  with long-bracket values.
- Pane-select swap modes now expose WezTerm-style mode entries `Pane Select Swap
  With Active` and `Pane Select Swap With Active Keep Focus`: selected panes
  exchange layout positions with the active pane, with focus either moving to the
  selected pane or staying on the original active pane. The command-palette
  queries `pane select swap show pane ids`/`show_pane_ids`/`show-pane-ids` and
  `pane select swap keep focus show pane ids`/`show_pane_ids`/`show-pane-ids`
  cover those modes plus `show_pane_ids=true`; the matching `paneselect ...`
  aliases and adding quote-aware `alphabet <chars>` to either query also cover the combined
  per-action alphabet subset. Action-name `enterpaneswap` and
  `enterpaneswapkeepfocus` queries dispatch the default mode entries.
- Pane-select move mode entries now expose `Pane Select Move To New Tab` and
  `Pane Select Move To New Window` in the command palette for `MoveToNewTab` and
  `MoveToNewWindow`: the selected pane is removed from the current split layout
  and moved into a new tab or window while preserving pane runtime state. The
  command-palette queries `pane select move to new tab show pane ids`/
  `show_pane_ids`/`show-pane-ids` and `pane select move to new window show pane
  ids`/`show_pane_ids`/`show-pane-ids` cover those modes plus
  `show_pane_ids=true`; matching `paneselect ...` aliases and adding quote-aware
  `alphabet <chars>` also cover
  the combined per-action alphabet subset. Action-name
  `enterpanemovetonewtab` and `enterpanemovetonewwindow` queries dispatch the
  default move mode entries.
- Pending `MoveToNewWindow` requests can now be consumed into detached
  app-shell/native-window app state while transferring the selected pane runtime
  snapshot.
- `rssh-app window` now runs through a multi-window manager that materializes
  detached MoveToNewWindow app states as additional native OS windows.
- PTY reader events now carry app-shell `WindowId` plus `PaneId`, avoiding
  pane-id-only routing once independent windows create their own panes. PTY EOF
  handling waits for process status and honors native `exit_behavior` overrides
  for `Close`, `Hold`, and `CloseOnCleanExit`, including configured
  `clean_exit_codes` for non-zero statuses that should count as clean.
  Native `exit_behavior_messaging` controls held-pane status text verbosity,
  with `None` suppressing the message and verbose/brief messages using
  WezTerm's documented success/failure prefixes. Lua config parsing remains
  open.
- Command palette close actions now follow WezTerm's pane/tab/window lifecycle:
  closing the final pane in a tab closes the tab when possible, and closing the
  final tab/pane requests native-window shutdown.

The next layer is full App Shell v2 integration (multi-window focus/lifecycle
polish, remaining pane focus visuals, pane-local scrollbars/selection polish, drag
resize affordance polish, Lua/custom tab formatting, and external CLI/mux
tab-title control) before mux/domain and protocol extensions are scaled.
