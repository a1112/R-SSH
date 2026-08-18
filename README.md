# R-SSH

The versioned cross-platform functional-test runner, observer isolation model,
scenario authoring rules, and CI evidence gate are documented in
[`docs/functional-testing.md`](docs/functional-testing.md).

R-SSH is a native Rust route for a high-performance SSH terminal client. The
target product shape is closer to XShell than to a web terminal: native windows,
direct GPU text rendering, native and system SSH transports, session management,
SFTP, tunnels, logging, and planned secure storage.

## Project Stage

R-SSH is at the **production-parity foundation** stage. The repository has moved
beyond its MVP 1-6 foundations: the native GUI presents through direct `wgpu`,
text is shaped and resolved through the `rssh-fonts`/`cosmic-text` stack, and
both the in-process `russh` transport and system OpenSSH integration are
implemented. This stage is a bounded, evidence-driven production foundation;
it is not a claim of production release certification or 100% WezTerm parity.

Verification status at source commit `83ade73a` on 2026-08-02:

- **verified locally on Windows x64**: Rust 1.89 workspace gates plus focused
  native GUI, real PTY, SSH/OpenSSH interoperability, and fresh unpacked package
  checks.
- **verified locally on macOS ARM64 for the current adaptation**: Rust 1.89
  all-target compilation, Unix PTY and self-test probes, a release `.app`,
  unpacked package checks, real OpenSSH loopback, and Metal ten-frame native
  presentation.
- **defined in hosted workflow but not run for the remaining targets in this
  local session**: Windows ARM64, Linux x64/ARM64, and macOS x64 native build and
  package jobs.
- **requires protected/self-hosted environment**: fixed-runner performance
  certification, release signing, macOS notarization/stapling, provenance
  attestation, and publication.
- **not yet evidenced**: successful runtime and package results for the other
  four native targets, hardware/driver/IME/RDP/DPI certification, or a complete
  WezTerm compatibility result. A protected hardware certification environment
  and result workflow still need to be established.

Commit `74f78bab` subsequently corrected the Linux native jobs to install
`openssh-server` behind a restored `policy-rc.d` guard. Its YAML, shell,
job-contract, cleanup-semantics, formatting, and Clippy checks passed locally;
the hosted Linux jobs themselves were not run in this local session.
Commit `f02acff6` applies the same canonical guard to unsigned and protected
release package-smoke jobs; its local contract checks passed, but those hosted
release jobs were likewise not run in this session.

See the [production-parity verification record](docs/production-parity-verification.md),
[release contract](docs/release-console.md),
[performance baseline](docs/performance-baseline.md),
[macOS cross-platform adaptation](docs/plans/2026-08-04-macos-cross-platform-adaptation.md),
[approved design](docs/plans/2026-07-28-production-parity-design.md),
[implementation plan](docs/plans/2026-07-28-production-parity-implementation.md),
and [bounded WezTerm gap tracker](docs/research/wezterm-parity-gap.md) for the
evidence and remaining risks.

MVP 6 keeps startup compatible: `rssh-app` still opens a single local PTY pane
by default, but that runtime is now surfaced through typed app-shell state:
workspace, tabs, and panes. The native window can render a basic tab bar and
right/down split panes with explicit tab titles, active-pane terminal title
fallback, pane-local click focus, wheel routing, and keyboard/palette resize and
zoom actions including explicit zoom/unzoom, directional pane activation,
palette-driven Rename Tab queries, Activate Last Tab, indexed tab activation,
wrapping and no-wrap relative tab activation, indexed pane activation, pane
rotation, relative and absolute tab-order movement, and pane-select Activate,
swap, and move-to-new-tab/window overlays, plus command-palette selection
clearing, CopyTo/PasteFrom actions for the system clipboard plus
PrimarySelection command routing, and paste-from-clipboard into the active pane.
On macOS the default native window uses a transparent full-size AppKit titlebar:
the tab strip extends into the titlebar while retaining native traffic-light
controls, resizing, rounded corners, and shadow. Empty tab-strip space drags the
window. Overflowing tab strips keep the active tab visible, mark hidden tabs at
the leading and trailing edges, and support browser-style middle-click close in
addition to drag reordering, wheel navigation, and Command-number shortcuts.
The command palette also exposes Clear Scrollback (`ScrollbackOnly`) and Clear
Scrollback And Viewport (`ScrollbackAndViewport`), Reset Terminal, and
scrollback navigation actions for top/bottom, page, line, and OSC 133
previous/next prompt movement. The terminal core also records OSC 133
Prompt/Input/Output semantic zones for retained rows and can extract text from
those zones while unwrapping soft-wrapped physical rows into logical lines; copy
mode can move between semantic zones across retained scrollback with `z` and
`Shift+Z`, plus typed Prompt/Input/Output zone movement via `Alt+P`, `Alt+I`,
and `Alt+O`/`Alt+Z`, while copy-mode selection anchors let `y` copy text
spanning the live viewport and retained history to ClipboardAndPrimarySelection
before scrolling back to bottom and closing, including WezTerm-style cell
selection through Space/`v`, line selection through uppercase no-modifier or
shifted `V`, and rectangular block selection with `Ctrl+V`; `j`/`k`, Enter/CR, and
PageUp/PageDown also move through retained history, with Enter/CR moving to the
next line start, and `g`/`Shift+G` jump to
scrollback top/bottom. WezTerm-style viewport movement through `H`/`M`/`L`
handles both shifted and uppercase no-modifier key events. Copy-mode
`^`/`Alt+m` and `$`/End now use WezTerm-style content-aware
line start/end movement, landing on the first/last non-space cell, and
WezTerm-style word movement is available through `w`, `b`, `e`, Tab,
Shift+Tab, Alt+Left/Right, and Alt+F/B. Copy-mode jump-to-char also covers
WezTerm-style `f`/`t`/`F`/`T` plus `;` repeat and `,` reverse repeat. The
`o`/`O` bindings move to the other selection end or horizontal selection end.
Ordinary copy-mode close scrolls back to the bottom before exiting, and
copy/search close handles both Escape key events and character ESC (`\u{1b}`)
events while clearing copy-mode search status from the window title. Copy mode
and copy-mode search also allow global command-palette and app-shell shortcuts
such as `Ctrl+Shift+P` and `Ctrl+Shift+T` to fall through from the overlay,
matching WezTerm key-table fallback behavior.
The copy-mode search path keeps copy mode active with `/`/`?` search input,
WezTerm-style next/prior match navigation via Down/`Ctrl+N` and
Up/Enter/CR/`Ctrl+P`, page-wise match navigation via PageDown/PageUp, and
`Ctrl+R` match-type cycling across case-sensitive, case-insensitive, and regex
search. Default `Ctrl+Shift+F`/`Super+F` search uses the same WezTerm-style
search navigation bindings for Down/Up, `Ctrl+N`/`Ctrl+P`, PageDown/PageUp,
`Ctrl+R` match-type cycling, `Ctrl+U` clear-pattern, character ESC close, and
initial query prefill from the current selection's first line, while plain
`Ctrl+F` remains available to the active PTY.
The default startup maps to workspace `1`, tab `1`, and pane `1`, with
`rssh-app` window title exposing the current state.

## Technical Direction

- Language: Rust.
- Window and event loop: `winit`.
- GPU renderer: direct `wgpu` presentation with `glyphon` text batches, plus a
  CPU/offscreen renderer for deterministic tests and benchmark proxies.
- Text shaping and font fallback: the `rterm-fonts` package backed by
  `cosmic-text`, including the configured fallback stack.
- SSH: an in-process `russh` backend for native sessions and forwarding, plus a
  system OpenSSH backend for SSH/SFTP/SCP and compatibility-oriented options.
- Local shell: Windows ConPTY and Unix PTY through a small internal abstraction.
- Planned storage: SQLite for sessions and host metadata; persistent product
  storage is not part of the currently evidenced foundation.
- Planned secret storage: Windows DPAPI, macOS Keychain, and Linux Secret
  Service; passwords and key passphrases must not be persisted as plaintext.

## Workspace

```text
crates/rssh-app       Desktop application entry point
crates/rterm-types    Dependency-free terminal/session value types
crates/rssh-domain    Window/workspace/tab/pane and launch domain state
crates/rssh-core      rssh-core compatibility facade for legacy public paths
crates/rssh-terminal  rterm-terminal package: terminal grid and VT parser
crates/rterm-render-core  Renderer-neutral snapshots, geometry, damage, and layer values
crates/rterm-render-cpu   PixelRenderer, text rasterization, image decode, and software frames
crates/rterm-render-wgpu  WGPU surfaces, render graph, glyphon text, textures, and recovery
crates/rssh-renderer      One-stage rssh-renderer compatibility facade
crates/rssh-ssh       SSH session boundary
crates/rssh-pty       Local PTY boundary
crates/rterm-fonts    Font discovery, shaping, fallback, and deterministic fonts
crates/rssh-test-support  Hermetic SSH fixtures and bounded process-test support
crates/rssh-web       Loopback WebSocket PTY bridge for the browser client
web/                  TypeScript/xterm.js browser terminal client
tauri/                Tauri desktop shell for the Web terminal client
docs/                 Architecture and planning documents
refs/                 Local reference source cache, ignored by Git
```

Stage 1 establishes one-way package ownership: terminal primitives come from
`rterm-types`, application identifiers and app-shell state come from
`rssh-domain`, and foundational crates import those owners directly. The
`rssh-core compatibility facade` preserves existing source paths while callers
are migrated incrementally. The `rterm-terminal` Cargo package intentionally
retains the `crates/rssh-terminal` directory during this stage because the
immutable Task 10 provenance evidence records that physical source path.

Stage 2 makes `rterm-runtime` the transport-neutral owner of pane workers,
bounded mailboxes, terminal progression, and the `SessionTransport` ownership
contract. Concrete `runtime-adapter` implementations now belong to `rssh-pty`
and `rssh-ssh`, which depend inward on the runtime abstraction; the runtime no
longer links PTY or Russh implementations. Its Cargo package intentionally keeps
the `crates/rssh-runtime` directory because Task 10 provenance also freezes
those physical source paths.

Stage 3 makes renderer ownership explicit without changing pixels or presentation:
`rterm-render-core` owns the shared terminal snapshot, geometry, damage, paint/layer
value, and digest contracts; `rterm-render-cpu` owns deterministic software
composition and decoding; and `rterm-render-wgpu` owns GPU planning, surfaces,
glyphon text, textures, recovery, and presentation. The application directly
composes the CPU bootstrap/fallback path with the WGPU path. The
`rssh-renderer compatibility facade` remains implementation-free for one migration
stage and re-exports the legacy public surface.

Stage 4 reduces snapshot and cache retention without changing rendered output.
Rows, graphemes, styles, hyperlinks, and inline-image payloads use shared immutable
identities; damage updates replace only touched rows. Snapshot and image retention
have independent byte budgets, while compatibility callers can still request a
lazy flat cell view. The fixed Windows runner enforces full/damage equivalence,
80×24 and 200×60 evidence, parser throughput of at least 98% of the checked-in
baseline, and a downward Stage 0 memory trend. See the
[Stage 4 snapshot/cache contract](docs/benchmarks/stage4-snapshot-cache.md).

## Local Commands

### Web terminal

Build the browser assets, then start the loopback WebSocket PTY bridge. The
server prints a one-time authenticated URL; open that URL in a modern browser.

```sh
cd web
npm install
npm run build
cd ..
cargo run -p rssh-web -- --listen 127.0.0.1:7788 --web-root web/dist
```

The initial Web version starts the platform default local shell through
`rssh-pty`, uses xterm.js for VT parsing/rendering, and terminates the shell when
the browser connection closes. It is intentionally loopback-only; remote
deployment and detached/reconnect sessions are not enabled. The printed URL
contains a 60-second, single-use ticket; redemption issues a separate session
cookie and replaying the URL is rejected.

For frontend development, start the Rust bridge with the Vite origin allowed,
open its printed bootstrap URL once to establish the local cookie, then run the
Vite server in a second terminal:

```sh
cargo run -p rssh-web -- --listen 127.0.0.1:7788 \
  --web-root web/dist --allowed-origin http://127.0.0.1:5173
cd web
npm run dev
```

Open `http://127.0.0.1:5173` after the bootstrap step. The Vite proxy forwards
`/api` and its WebSocket upgrade to the Rust bridge.

### Tauri terminal

The Tauri desktop client reuses the same `web/` xterm.js UI. Its Rust host
starts `rssh-web` in-process on a random loopback port, navigates the window to
the authenticated local page, and shuts the bridge down with the app. The
desktop window is frameless: the Web header provides the drag region and
minimize/maximize/close controls, while the standalone browser view keeps
those desktop-only controls hidden. On macOS the controls use traffic-light
styling with a transparent, shadowed window surface.

```sh
cd web
npm install
npm run build
cd ../tauri
npm install
npm run dev
```

Create a release bundle with `npm run build` from `tauri/`. The Tauri bundle
includes the compiled Web assets and the generated application icons.

macOS/Linux quick start:

```sh
cargo fmt --all
cargo test --workspace
cargo build --release -p rssh-app
./target/release/rssh-app version --json
./target/release/rssh-app doctor
./target/release/rssh-app self-test --json
cargo run -p rssh-app
cargo run -p rssh-app -- local -- /bin/sh -lc 'printf "console-smoke\\n"'
```

On macOS, the default shell uses `$SHELL` and falls back to `/bin/zsh` for
Finder/LaunchServices launches. Mutable UI state defaults to
`~/Library/Application Support/R-SSH`; setting `XDG_STATE_HOME` overrides that
location for CLI and hermetic workflows.

Windows and full compatibility command inventory:

```powershell
cargo fmt --all
cargo test --workspace
cargo build --release -p rssh-app
.\target\release\rssh-app.exe version --json
.\target\release\rssh-app.exe doctor
.\target\release\rssh-app.exe self-test --json
cargo run -p rssh-app
cargo run -p rssh-app -- window --frames 3
cargo run -p rssh-app -- doctor
cargo run -p rssh-app -- doctor --json
cargo run -p rssh-app -- version
cargo run -p rssh-app -- version --json
cargo run -p rssh-app -- self-test
cargo run -p rssh-app -- self-test --json
cargo run -p rssh-app -- bench --json
cargo run -p rssh-app -- bench --bytes 4194304 --chunk-size 8192 --render-frames 120 --idle-ms 500 --cols 120 --rows 30
cargo run --locked --release -p rssh-app -- bench --json --workload ansi-scroll-query --bytes 1048576 --chunk-size 8192 --render-frames 30 --idle-ms 1000 --min-throughput-bytes-per-sec 1048576 --max-chunk-p95-us 5000 --max-render-frame-p95-us 16000 --max-idle-cpu-percent 3 --max-process-memory-bytes 268435456
cargo run --locked --release -p rssh-app -- bench --json --workload plain-scroll --bytes 1048576 --chunk-size 8192 --render-frames 30 --idle-ms 1000 --min-throughput-bytes-per-sec 5242880 --max-chunk-p95-us 5000 --max-render-frame-p95-us 16000 --max-idle-cpu-percent 3 --max-process-memory-bytes 268435456
cargo run -p rssh-app -- local --preflight -- cmd.exe /C echo console-preflight-smoke
cargo run -p rssh-app -- console --preflight -- cmd.exe /C echo console-alias-smoke
cargo run -p rssh-app -- local --metrics -- cmd.exe /C echo console-metrics-smoke
cargo run -p rssh-app -- local --metrics-json -- cmd.exe /C echo console-metrics-json-smoke
cargo run -p rssh-app -- window --frames 30 --metrics
cargo run -p rssh-app -- window --frames 30 --metrics-json
cargo run -p rssh-app -- window --frames 120 --metrics -- cmd.exe /K echo window-smoke
cargo run -p rssh-app -- window --frames 120 --metrics --log window.log -- cmd.exe /K echo window-log-smoke
cargo run -p rssh-app -- local
cargo run -p rssh-app -- local --cols 120 --rows 30
cargo run -p rssh-app -- local --mouse
cargo run -p rssh-app -- local --osc52 write
cargo run -p rssh-app -- local -- cmd.exe /C echo console-smoke
cargo run -p rssh-app -- local --log session.log -- powershell -NoProfile -Command "Write-Output logged-smoke"
cargo run -p rssh-app -- ssh ops@example.com
cargo run -p rssh-app -- ssh -p 2222 -i C:\Users\ops\.ssh\id_ed25519 -l ops example.com
cargo run -p rssh-app -- ssh ops@example.com uptime -p
cargo run -p rssh-app -- ssh --target prod --preflight
cargo run -p rssh-app -- ssh --target prod --metrics
cargo run -p rssh-app -- ssh --target prod --metrics-json
cargo run -p rssh-app -- ssh --host example.com --user ops --agent
cargo run -p rssh-app -- ssh --native --trust-on-first-use --host example.com --user ops --password
cargo run -p rssh-app -- ssh --native --trust-on-first-use --host example.com --user ops --key C:\Users\ops\.ssh\id_ed25519
cargo run -p rssh-app -- ssh --native --accept-unknown-host-key --host example.com --user ops --password
cargo run -p rssh-app -- ssh --target prod
cargo run -p rssh-app -- ssh --target prod -- uname -a
cargo run -p rssh-app -- ssh --host example.com --user ops --password
cargo run -p rssh-app -- ssh --host example.com --user ops --key C:\Users\ops\.ssh\id_ed25519
cargo run -p rssh-app -- ssh -F C:\Users\ops\.ssh\prod_config -o ProxyJump=bastion prod
cargo run -p rssh-app -- ssh -J bastion -C -vv prod
cargo run -p rssh-app -- ssh -T -W db.internal:5432 prod
cargo run -p rssh-app -- ssh -E ssh-debug.log -Q cipher prod
cargo run -p rssh-app -- ssh -L 127.0.0.1:15432:db.internal:5432 -D 127.0.0.1:1080 -N prod
cargo run -p rssh-app -- ssh --target prod --local-forward 127.0.0.1:15432:db.internal:5432 --no-shell
cargo run -p rssh-app -- ssh --target prod --dynamic-forward 127.0.0.1:1080 --no-shell
cargo run -p rssh-app -- ssh --target prod --osc52 write
cargo run -p rssh-app -- ssh --target prod --log prod.log
cargo run -p rssh-app -- sftp ops@example.com
cargo run -p rssh-app -- sftp -P 2222 -i C:\Users\ops\.ssh\id_ed25519 ops@example.com
cargo run -p rssh-app -- sftp -F C:\Users\ops\.ssh\prod_config -o ProxyJump=bastion prod
cargo run -p rssh-app -- sftp -J bastion -C -vv prod
cargo run -p rssh-app -- sftp -l 4096 prod
cargo run -p rssh-app -- sftp -b batch.txt -B 32768 -R 64 prod
cargo run -p rssh-app -- sftp --target prod
cargo run -p rssh-app -- sftp --host example.com --user ops --key C:\Users\ops\.ssh\id_ed25519
cargo run -p rssh-app -- sftp --target prod --log sftp.log
cargo run -p rssh-app -- scp local.txt ops@example.com:/tmp/remote.txt
cargo run -p rssh-app -- scp app.log audit.log ops@example.com:/tmp/logs/
cargo run -p rssh-app -- scp -l 4096 local.txt prod:/tmp/remote.txt
cargo run -p rssh-app -- scp -p local.txt prod:/tmp/remote.txt
cargo run -p rssh-app -- scp -R -s local.txt prod:/tmp/remote.txt
cargo run -p rssh-app -- scp -P 2222 -i C:\Users\ops\.ssh\id_ed25519 -r logs ops@example.com:/tmp/logs
cargo run -p rssh-app -- scp -F C:\Users\ops\.ssh\prod_config -o ProxyJump=bastion local.txt prod:/tmp/remote.txt
cargo run -p rssh-app -- scp -J bastion -C -vv local.txt prod:/tmp/remote.txt
cargo run -p rssh-app -- scp -O -T -B local.txt prod:/tmp/remote.txt
cargo run -p rssh-app -- scp ops@example.com:/tmp/remote.txt local.txt
cargo run -p rssh-app -- scp ops@example.com:/var/log/app.log ops@example.com:/var/log/audit.log logs
cargo run -p rssh-app -- scp ops@example.com --upload local.txt /tmp/remote.txt
cargo run -p rssh-app -- scp --target prod --upload local.txt /tmp/remote.txt
cargo run -p rssh-app -- scp --target prod --download /tmp/remote.txt local.txt
cargo run -p rssh-app -- profile --init --file rssh-profiles.toml
cargo run -p rssh-app -- profile --check --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile --check --json --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile --list --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile --list --verbose --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile --list --json --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile --show prod-shell --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile --show prod-shell --json --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile local-smoke --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile window-smoke --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile prod-shell --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile prod-files --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile prod-upload --file examples/rssh-profiles.toml
cargo test -p rssh-ssh
```

`start` is a WezTerm-style alias for the native-window startup path exposed as
`window`. `window <program> [args...]`, `window -- <program> [args...]`,
`window -e <program> [args...]`, `start <program> [args...]`, or
`start -e <program> [args...]` starts a custom command inside the native window;
without an explicit program, the native window starts the platform default shell.
On Windows, extensionless commands may resolve to `.cmd`, `.bat`, or `.ps1`
wrappers. R-SSH rejects `.cmd`/`.bat` paths and arguments containing `cmd.exe`
metacharacters because Windows cannot preserve untrusted batch arguments
reliably; use a native executable or a PowerShell script when those characters
are data. Explicit `.ps1` launches are non-interactive and honor the machine's
configured PowerShell execution policy.
Use `window --log PATH` to write visible native-window terminal output to a
session log file.
`console` is the explicit console-hosted startup path; `local` remains a
backward-compatible alias for the same local PTY runtime. Add `--mouse` when
you want terminal applications to negotiate xterm mouse/focus reporting through
PTY output modes, including legacy X10, UTF-8 `1005`, SGR `1006`, and urxvt
`1015` mouse encodings. SGR-pixels `1016` is not declared because the current
input model tracks terminal cells rather than pixel coordinates.
Bracketed paste mode is negotiated from PTY output automatically.
Synchronized output mode (`ESC[?2026h/l`) is handled from PTY output: the native
runtime delays render damage until reset, and the console path buffers visible
host-console writes while continuing to answer terminal queries.
The console path also answers basic terminal status, device-attribute, and
DECRQM status queries, including private input, cursor visibility, auto-wrap,
origin, alternate-screen, private cursor save modes, cursor blinking (`?12`),
Meta-key mode (`?1034`), and ANSI insert/replace mode (`CSI 4 $ p`). `RIS`
(`ESC c`) resets tracked mode state and releases synchronized-output buffers.
`DECSTR` (`CSI ! p`, including the C1 CSI form) soft-resets tracked origin,
cursor visibility, and insert/replace modes without treating the stream as a
full terminal reset.
Kitty keyboard progressive-enhancement negotiation is tracked from PTY output:
`CSI = flags ; mode u` applies replace/set/reset flag updates, `CSI > flags u`
/ `CSI < n u` update the active flags stack, `CSI ? u` is answered with the
current flags, and Ctrl/Alt ASCII character keys are encoded as kitty `CSI-u`
events when the disambiguate flag is active. When the kitty report-all flag is
active, plain text keys plus Enter/Tab/Backspace are also encoded as canonical
`CSI-u` events, and `Ctrl+Shift+Tab` uses canonical `CSI 9;6u` under
disambiguate mode rather than treating Shift+Tab as a separate key.
Navigation/editing keys, F1-F12, and F13-F35 use kitty canonical
functional-key forms under disambiguate/report-all modes, and keypad keys use
kitty KP_* private-use codepoints when kitty keyboard flags request
CSI-u reporting. The default console and native-window input paths encode
Menu/ContextMenu using the legacy `CSI 29~` functional sequence. Kitty
private-use functional codes also cover CapsLock, ScrollLock, NumLock,
PrintScreen, Pause, and Menu/ContextMenu in console and native-window paths,
plus media transport, track, record, and volume keys where the input backend
exposes them. Kitty
event-type reporting is supported for repeat/release events using
`modifier:event` subfields, including text-key repeat/release when only flag 2
is active, and report-all input includes kitty associated-text
third fields when flag 16 is active, including modified Enter sequences such as
`Ctrl+Enter` -> `CSI 13;5;13u` and `Ctrl+Shift+Enter` -> `CSI 13;6;13u`.
Console and native-window text-key input
also reports kitty alternate shifted key subfields when flag 4 is active, with
console kitty modifier encoding including crossterm-provided Super/Hyper/Meta
and CapsLock/NumLock state bits plus modifier-key private-use codepoints for
left/right Shift/Ctrl/Alt/Super/Hyper/Meta and ISO level shifts, and
native-window input additionally reporting printable PC-101 physical
base-layout subfields and kitty Super/Cmd/Windows modifier bits.
Xterm `modifyOtherKeys` negotiation is also tracked from `CSI > 4 ; N m`,
answered through `CSI ? 4 m`, and used for modified other-key input such as
Ctrl+Enter and Ctrl+Shift+I.
XTGETTCAP capability replies include terminal name, 256-color/true-color
markers, official WezTerm booleans, Meta-key boolean/templates
(`km`/`smm`/`rmm`), OSC 52 clipboard support, italic/style
underline/underline-color, overline, strikethrough, default-color, and
palette-reset templates, tmux/xterm cursor style and cursor color templates,
the WezTerm `Sync` synchronized-output template, foundational
cursor/screen/style/color capabilities including WezTerm cursor
visibility/blink, SGR, flash, and select-color templates, common line/display
editing controls, WezTerm control/save-restore sequence capabilities,
cursor-position/device-attribute query templates, title/status-line,
title-stack, palette-initialization, printer, memory-lock, and reset/init
templates, tab-stop/erase/repeat/scroll-region templates, WezTerm SGR mouse
templates (`kmous`/`XM`/`xm`), application cursor, base and modified function-key
capabilities, WezTerm keypad transmit templates,
Backspace/BackTab/keypad-center/keypad-enter and shifted navigation/editing key
capabilities, WezTerm ACS enter/exit metadata, current `co`/`li` plus official
`cols`/`lines` column/row counts, `it=8` tab interval, and `pairs=32767`.
OSC color handling tracks and answers default foreground/background, cursor
color, and indexed palette queries, including multi-index `OSC 4` query
sequences, WezTerm-style RGBA dynamic color specs for `OSC 10`/`11`/`12`,
`OSC 110`/`OSC 111` foreground/background reset, `OSC 112` cursor-color reset,
and `OSC 104` indexed-palette reset.
OSC 52 clipboard writes are handled in the console path so local and
OpenSSH-backed terminal programs can use terminal clipboard integration. Local
terminal and window commands retain the WezTerm-style write-only default, while
remote `ssh` sessions default to `off`. PTY-side read queries are ignored unless
`--osc52 read-write` is selected explicitly, and decoded clipboard writes are
limited to 1 MiB. Use
`--osc52 off|write|read-write` on `console`/`local`, `ssh`, or `window` to
control whether PTY-side OSC 52 clipboard writes and read queries are allowed.
PTY-backed local, window, and OpenSSH child processes receive
`TERM=xterm-256color` and `COLORTERM=truecolor` by default.
Use `--cwd PATH` on `console`/`local`, `window`, or `start` to set the initial
child process working directory.
Use `--workspace NAME` on `window`/`start` to name the initial app-shell
workspace instead of the default `default` workspace.
Use `--class CLASS` on `window`/`start` to request the native window class name
on Windows. X11/Wayland class/app-id application remains future parity work.
Use `--position X,Y`, `--position screen:X,Y`, `--position main:X,Y`,
`--position active:X,Y`, or `--position <monitor>:X,Y` on `window`/`start` to
request an initial native window screen position. `main:` is relative to the
primary monitor origin, `active:` is relative to the active monitor when the
platform exposes one and otherwise falls back to the primary monitor origin,
and named monitor forms such as `HDMI-1:10,20` are relative to the matching
monitor origin.
`window`/`start` also accepts WezTerm startup compatibility flags
`--no-auto-connect`, `--always-new-process`, and `--new-tab`; they are no-ops
until R-SSH grows a GUI daemon and auto-connected mux domains. `window`/`start`
accepts `--domain local` for the current local PTY domain and accepts
`--attach` as a no-op until mux domain attachment exists; remote or named mux
domains remain future parity work.
Use `--log PATH` on `console`/`local`, `ssh`, `sftp`, or `scp` to tee visible
terminal output to a session log file.
Use `doctor` before launching console sessions to report the selected PTY
backend, native SSH backend, terminal size, and child terminal environment, and
to verify that the local default shell plus `ssh`, `sftp`, and `scp` are
available; add `--json` for a machine-readable report.
Use `self-test` or `self-test --json` after download to run a local PTY smoke
and verify that `ssh -V`, `sftp -h`, and `scp -h` can launch without opening a
network connection.
Use `bench` or `bench --json` to run a deterministic terminal-runtime benchmark
without opening a network connection. It feeds ANSI/CSI/OSC workload bytes
through the same terminal parser and query-response path used by the console and
native window, reporting throughput, p95 chunk processing time, visible output
bytes, response count, bell count, scrollback lines, final cursor position,
plus offscreen `PixelRenderer` frame count, p95 frame time, rendered pixels, and
rendered pixel throughput. It also samples the current `rssh-app` process during
an idle window and reports idle CPU usage, resident memory, virtual memory, and
accumulated CPU time. Use `--render-frames N` to tune the offscreen render
sample count and `--idle-ms N` to tune the resource sampling window. Add
`--min-throughput-bytes-per-sec`, `--max-chunk-p95-us`,
`--max-render-frame-p95-us`, `--max-idle-cpu-percent`, or
`--max-process-memory-bytes` to turn the benchmark into a non-zero-exit quality
gate; JSON output includes `threshold_violations` when a gate fails.
Each violation identifies `metric`, `observed`, and `expected` values while
retaining compatibility aliases `actual` and `limit`.
Add `--preflight` to `console`/`local`, `ssh`, `sftp`, or `scp` when startup
should run the same console dependency check before spawning the PTY child
process. Add `--metrics` to `console`/`local`, `ssh`, `sftp`, or `scp` to print
human-readable console runtime metrics after the PTY child process exits,
including command, PTY backend, startup columns/rows, final session state,
elapsed time, exit code, signal, and success state, plus PTY input/output bytes,
terminal output bytes, and resize events.
Use `--metrics-json` on the same commands when a launcher, script, or desktop
UI should consume the metrics as JSON.
Native window runs also support `--metrics-json`, including automated
`window --frames N` smoke runs, so render and PTY processing metrics can feed
external benchmark dashboards. The window metrics include terminal damage
region, damaged-cell, snapshot damage-update, and full snapshot-rebuild totals;
live bottom PTY output now uses damage regions to update the existing render
snapshot and dirty-render the affected framebuffer cells before the later GPU
renderer work.
`ssh` currently starts the system OpenSSH client inside the same PTY console
runtime, so remote login can use the existing host OpenSSH configuration,
known-host handling, agent, key prompts, and password prompts without exposing
secrets in the R-SSH command line. Use `ssh ops@example.com` for the shortest
console launch path, `ssh --target prod` for a saved OpenSSH config host, or
`ssh --host example.com --user ops` when you want explicit host/user flags.
For OpenSSH-style positional targets and `--target NAME`, a trailing command is
passed through as the remote command, for example `ssh ops@example.com uptime
-p` or `ssh --target prod uname -a`. Direct `--host/--user` launches keep using
`--` before remote commands so password and key prompts cannot be mistaken for
command-line secrets. Common OpenSSH short options are accepted on the console
path: `ssh -p PORT -l USER -i KEY -F CONFIG -o OPTION=VALUE -L SPEC -R
SPEC -D SPEC -N HOST`. `-J JUMP`, `-4`, `-6`, `-A`, `-a`, `-C`, `-q`, and
`-v`/`-vv`/`-vvv` are also passed through to OpenSSH. SSH-specific control,
debugging, stdio forwarding, tunnel, TTY, and X11 options `-B`, `-b`, `-c`,
`-E`, `-e`, `-I`, `-m`, `-O`, `-P`, `-Q`, `-S`, `-W`, `-w`, `-f`, `-G`, `-g`,
`-K`, `-k`, `-M`, `-n`, `-s`, `-T`, `-t`, `-tt`, `-X`, `-x`, `-Y`, and `-y`
are passed through on the OpenSSH console backend.
`sftp` starts the system OpenSSH SFTP client inside the same PTY console
runtime, using the same `--host`/`--target`, `--user`, `--port`, `--agent`,
`--password`, `--key`, and `--log` shape for interactive file transfer; it also
accepts the same positional `[USER@]HOST` target. Common short options
`sftp -P PORT -i KEY -J JUMP -F CONFIG -o OPTION=VALUE -l LIMIT -C -v HOST`
are supported. Use `--user USER` when a username override is needed outside
the `[USER@]HOST` target form. SFTP-specific OpenSSH options `-b`, `-B`, `-R`,
`-D`, `-S`, `-s`, `-X`, and `-c` are passed through for batch mode and transfer
tuning.
`scp` starts the system OpenSSH SCP client inside the same PTY console runtime
for one-shot upload and download transfers. Use `scp local.txt
ops@example.com:/tmp/remote.txt` to upload and `scp
ops@example.com:/tmp/remote.txt local.txt` to download. OpenSSH-style
positional transfers also accept multiple local sources before one remote
destination, or multiple remote sources from the same target before one local
destination. The longer `--upload LOCAL REMOTE` and `--download REMOTE LOCAL`
forms remain available when the target is supplied separately; add `-r` or
`--recursive` for directories.
Common short options
`scp -P PORT -i KEY -J JUMP -F CONFIG -o OPTION=VALUE -l LIMIT -C -v` are
supported. Use `--user USER` when a username override is needed outside the
`[USER@]HOST` target form. SCP-specific OpenSSH options `-3`, `-O`, `-T`, `-B`,
`-p`, `-R`, `-s`, `-D`, `-S`, `-X`, and `-c` are passed through for protocol
and transfer tuning.
Add `--native` to use the in-process `russh` path instead of spawning an
interactive system OpenSSH session. The native path supports `--host`
direct targets and `--target NAME` entries resolved through `ssh -G`, with
agent, password-prompt, or private-key authentication, including encrypted
private-key passphrase prompts. Use
`--trust-on-first-use` to record a first-time host key in the user's
`.ssh/known_hosts` file and verify it on later connections.
`--accept-unknown-host-key` remains available for insecure test servers only.
OpenSSH passthrough flags such as `-J`, `-F`, `-o`, `-W`, and `-T` require the
OpenSSH console backend and are rejected with `--native`.

### Native SSH GUI

Use `ssh --gui --target prod` (or the existing positional/`--host` target and
authentication options) to open an SSH session in the native window. `--gui`
implies the in-process native SSH backend; passwords and private-key
passphrases are requested through masked window prompts rather than command-line
arguments.

The GUI renderer modes are:

- `--renderer auto` is the default. It presents a CPU bootstrap frame first,
  then adopts the GPU renderer after a complete GPU frame succeeds; GPU failure
  keeps the software path available.
- `--renderer cpu` forces software presentation and is the operational fallback
  when a GPU or driver is unreliable.
- `--renderer gpu` selects the synchronous GPU startup path for explicit GPU
  testing or rollback comparison.

`--benchmark-startup` is for the fixed Windows startup harness, not interactive
use. It presents one CPU bootstrap frame, emits the `first_present` marker, and
exits before configuration, GPU initialization, or SSH transport work begins.
It is valid only with `--gui`.

A saved GUI profile can use the same settings:

```toml
[profiles.gui-prod]
kind = "ssh"
target = "prod"
gui = true
renderer = "auto"
host_key_policy = "prompt"
auth = "agent"
```

With `host_key_policy = "prompt"`, an unknown host key can be accepted for the
current connection, accepted and written to `known_hosts`, or cancelled. A
changed host key is always blocked. GUI SSH does not support forwarding
(`-L`/`-R`/`-D`), and GUI SSH does not support `--no-shell`.
GUI SSH does not support OpenSSH passthrough options such as `-J`, `-F`, or
`-o`, and it rejects
`--preflight`. Interactive shells and explicit remote commands are supported;
SFTP and SCP remain separate console commands.

Native `--local-forward` and `--dynamic-forward` start in-process listeners and
open russh `direct-tcpip` channels for accepted local TCP or SOCKS5 CONNECT
requests. Dynamic forwarding rejects non-loopback bind addresses because its
SOCKS5 listener has no client authentication. Native `--remote-forward`
requests a server-side TCP listener and
maps incoming forwarded connections back to the configured local target.
Native SSH also honors `--metrics` and `--metrics-json`, reporting the
`NativeRussh` backend, resolved host/user/port, startup size, final session
state, SSH input/output bytes, elapsed time, and exit result without logging
password or key material.
Native SSH accepts Ed25519 and supported ECDSA private keys. Native RSA
private-key authentication is disabled because the Rust RSA implementation has
an unresolved timing-side-channel advisory; use the OpenSSH backend when a
legacy RSA identity is unavoidable.

Release packages build the native application with the minimal
`production-gui` feature set: native GUI, native SSH, local PTY, and PNG/JPEG
images. GIF and legacy DDS/Farbfeld/ICO/PNM/TGA/TIFF decoders are opt-in build
features. Developer builds retain the full command inventory by default;
`bench`, `doctor`, `self-test`, SFTP, and SCP are diagnostic/transfer entrypoints
and are intentionally not exposed by the reduced packaged GUI binary. The two
build profiles can be checked directly with:

```sh
cargo build --locked --release -p rssh-app
cargo build --locked --release -p rssh-app --no-default-features --features production-gui
```

### Stage 0 GUI diagnostics

The cross-platform Stage 0 launcher measures a fresh empty native window and a
fresh one-pane native SSH GUI process with versioned lifecycle markers and an
identity-bound native memory metric. On Windows, run
`scripts/ci/run-stage0-diagnostics.ps1`; on Linux or macOS, run
`scripts/ci/run-stage0-diagnostics.sh`. Both runners retain individual v2 JSON
records plus an aggregate report. The existing fixed-runner startup limit stays
blocking, while the initial steady-memory targets remain report-only. See the
[Stage 0 schema and runner contract](docs/benchmarks/stage0-schema-v2.md) for
commands, exact platform metric semantics, artifact layout, failure behavior,
and threshold graduation rules.
Use `--password` as a flag when you want OpenSSH to prompt in the terminal; do
not pass password or key-passphrase values as command arguments.
Use `--target NAME` to reuse an OpenSSH `Host NAME` entry from your existing
SSH config; `--user`, `--port`, `--password`, and `--key` can still override
the generated OpenSSH command when needed.
Add `-- <command> [args...]` after the SSH options to run a remote command
instead of opening the default interactive shell.
The native `russh` backend treats this form as an argument vector and quotes
each token for the remote POSIX shell, so spaces and shell metacharacters remain
part of the original argument. To request pipelines, redirections, or other
shell syntax deliberately, invoke a shell explicitly, for example
`-- sh -lc 'printf "%s\n" "$HOME" | head -1'`.
Use `--local-forward`, `--remote-forward`, or `--dynamic-forward` with OpenSSH
forward specs for tunnels. Add `--no-shell` when the session should only keep
the tunnel open. The OpenSSH short aliases `-L`, `-R`, `-D`, and `-N` work on
the same console path. Use `-F CONFIG` or repeated `-o OPTION=VALUE` on the
OpenSSH console backend when a launch needs an alternate config file,
ProxyJump/ProxyCommand, or host-key options.
`profile NAME --file PATH` loads a TOML session profile and then starts the
same local, native-window, SSH, SFTP, or SCP runtime. See
`examples/rssh-profiles.toml` for the current file format, including
`kind = "local"`, `kind = "window"`, `kind = "ssh"`, `kind = "sftp"`,
`kind = "scp"`, and the optional `log = "path"` field.
Use `profile --init --file PATH` to create a starter profile file. Existing
files are not overwritten unless `--force` is added.
Use `profile --list --file PATH` to print available profile names and kinds
before launching one; add `--verbose` to include each profile's resolved
`rssh-app` command line, or `--json` for machine-readable output with the
resolved command string and argv array.
Use `profile --show NAME --file PATH` to preview the resolved `rssh-app`
command line without starting a local process or network connection; add
`--json` to return the single launch plan as name, kind, command, and argv.
Set `preflight = true` in local, SSH, SFTP, or SCP profiles when saved sessions
should run the console dependency check before spawning the PTY child process.
Set `metrics = true` in local, window, SSH, SFTP, or SCP profiles when saved
sessions should print runtime metrics on exit. Use `metrics = "json"` when
saved console or native-window sessions should emit machine-readable JSON
metrics.
For saved native SSH sessions, set `native = true` and choose
`host_key_policy = "trust-on-first-use"`, `"accept-unknown"`, or
`"reject-unknown"`.
Use `profile --check --file PATH` to validate every configured profile without
starting a local process or network connection; add `--json` for a structured
per-profile report that still exits non-zero when any profile is invalid.

## Native Release Packages

The `Release` GitHub Actions workflow defines package contracts for six native
targets: Windows, Linux, and macOS on x64 and ARM64. Manual workflow artifacts
are explicitly unsigned and non-releasable. A `v*` tag follows a protected DAG
that requires fixed-runner performance certification, native package smokes,
platform signing, macOS notarization/stapling where applicable, SBOM and
provenance generation, and final publication gates.

At commit `83ade73a` on 2026-08-02, the fresh Windows x64 archive and unpacked
binary path were **verified locally on Windows x64**. The remaining five target
jobs are **defined in hosted workflow but not run in this local session**.
Protected performance, signing, notarization, and attestation
**require protected/self-hosted environment**. Hardware/IME/RDP/DPI
certification is **not yet evidenced** and still needs a protected hardware
environment and result workflow. See the [native package contract](docs/release-console.md),
[performance baseline](docs/performance-baseline.md), and
[verification record](docs/production-parity-verification.md).

## MVP Status

- MVP 1: Terminal core baseline is complete. See `docs/mvp-1-terminal-core.md`.
  The core now supports background color erase for SGR-colored blank cells.
- MVP 1/3 compatibility: SGR strikethrough now parses through the terminal core,
  reports through SGR state queries, survives renderer snapshots, and draws in
  the native pixel renderer.
- MVP 1/3 compatibility: SGR faint now parses through the terminal core,
  reports through SGR state queries, survives renderer snapshots, and renders as
  dimmed native foreground pixels.
- MVP 1/3 compatibility: SGR conceal now parses through the terminal core,
  reports through SGR state queries, survives renderer snapshots, and hides
  native foreground pixels while preserving the cell contents.
- MVP 1/3 compatibility: SGR overline now parses through the terminal core,
  reports through SGR state queries, survives renderer snapshots, and draws in
  the native pixel renderer.
- MVP 1/3 compatibility: SGR blink now parses through the terminal core,
  reports through SGR state queries, survives renderer snapshots, and can hide
  native foreground pixels during the renderer's hidden blink phase.
- MVP 1/3 compatibility: SGR double underline now parses through the terminal
  core, reports through SGR state queries, survives renderer snapshots, and
  draws as two native underline strokes.
- MVP 1/3 compatibility: SGR italic now parses through the terminal core,
  reports through SGR state queries, survives renderer snapshots, and renders
  with a slanted bitmap glyph pass in the native pixel renderer.
- MVP 1/3 compatibility: SGR underline color now parses through the terminal
  core, reports through SGR state queries, survives renderer snapshots, and
  renders underline strokes independently from glyph foreground color.
- MVP 1/3 compatibility: colon-separated SGR underline styles (`4:0` through
  `4:5`) now parse without leaking style parameters into bold/faint/italic,
  report through SGR state queries, survive renderer snapshots, and render
  single, double, curly, dotted, and dashed underline strokes.
- MVP 2: Local terminal path is complete as a console-hosted prototype. See
  `docs/mvp-2-local-terminal.md`.
- MVP 3: Native window renderer demo is complete. See
  `docs/mvp-3-native-renderer.md`.
- MVP 4: Live PTY session inside the native renderer is complete. See
  `docs/mvp-4-live-pty-window.md`.
- MVP 5 groundwork: Window smoke runs can print startup, PTY processing,
  terminal damage, snapshot update/rebuild, full/dirty rendering, and
  input-write metrics with `window --metrics` or `window --metrics-json`;
  native window literal search, `literal:<text>` forced-literal search, and
  `regex:<pattern>` search can match across visual row boundaries in scrollback
  and the live grid;
  OSC 8 hyperlinks now survive renderer snapshot conversion and can be opened
  from the native window with `Ctrl` + left click;
  the native window now draws a clickable and draggable right-edge scrollback
  scrollbar instead of putting scrollback position in the title;
  `bench --json` now provides a repeatable
  terminal-runtime throughput, p95 chunk latency, offscreen renderer p95 frame
  time, rendered-pixel throughput, idle CPU, process memory, virtual memory,
  accumulated CPU-time baseline, and optional threshold-gate failures for
  console/native-window parser, rendering, and resource work. The SSH crate now has a
  validated config, authentication request model, connector entry point,
  shell-session boundary, `rssh-app ssh` request parsing, and an injectable SSH
  runner path for local input and remote output. `rssh-core` also includes a
  shared session lifecycle model for created, connecting, connected,
  disconnected, and closed states. The app-level `ssh` command can also run the
  system OpenSSH client through the PTY console runtime as an interim backend.
  See
  `docs/mvp-5-ssh-session-boundary.md`.

- MVP 6 foundation: App Shell v1 introduces process-local workspace, tab, and
  pane state in `rssh-core` with typed IDs and actions; `rssh-app window`
  initializes with workspace `1`, tab `1`, pane `1` and appends that shell
  state in the native window title. Shortcuts for tab/pane actions are routed
  through the action dispatch boundary, with a basic explicit-title-aware tab
  bar that falls back to active-pane terminal titles, split-pane rendering,
  click-to-focus, pane-local wheel scrolling, and keyboard/palette pane
  resize/zoom actions including explicit zoom/unzoom plus `rename tab <title>`
  palette input, Activate Last Tab, indexed tab activation, wrapping/no-wrap
  relative tab activation, indexed pane activation, pane rotation, tab-order
  movement including Move Tab To entries, and pane-select
  Activate/swap/move-to-new-tab/window modes in the native window.
  See `docs/mvp-6-app-shell-v1.md`.

- Web terminal initial bridge: `rssh-web` serves the authenticated loopback
  WebSocket PTY endpoint and the `web/` xterm.js client supports interactive
  shell input, raw PTY output, resize, process exit, and bounded session
  cleanup. See `docs/plans/2026-08-05-web-terminal-design.md` for the protocol
  and security boundary.
- Tauri Web shell: `tauri/` embeds the same Web client, starts the loopback
  bridge in-process, and reuses the authenticated page without a sidecar.

## Reference Sources

Reference projects are cloned under `refs/` for local study and are intentionally
not committed. See `refs/README.md` and `docs/research/native-terminal-references.md`
for the current list and what each project is used for.
