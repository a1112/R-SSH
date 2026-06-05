# R-SSH

R-SSH is a native Rust route for a high-performance SSH terminal client. The
target product shape is closer to XShell than to a web terminal: native window,
GPU text rendering, direct SSH protocol integration, session management, SFTP,
tunnels, logging, and secure key storage.

The repository now includes MVP 1 for the terminal core, MVP 2 for a
console-hosted local terminal path, MVP 3 for a native `winit` renderer demo,
and MVP 4 for a live PTY session inside the native window. The current app can
start a native window, spawn the platform shell through the local PTY layer,
feed PTY output into the terminal grid, render live terminal cells, and write
keyboard input back to the PTY.

## Technical Direction

- Language: Rust.
- Window and event loop: `winit`.
- GPU renderer: `wgpu`.
- Text shaping and font fallback: `cosmic-text` or HarfBuzz-backed equivalent.
- SSH: start with pure Rust `russh`; keep `libssh2` as a fallback option for
  algorithm and server compatibility.
- Local shell: Windows ConPTY and Unix PTY through a small internal abstraction.
- Storage: SQLite for sessions and host metadata.
- Secret storage: Windows DPAPI, macOS Keychain, and Linux Secret Service.

## Workspace

```text
crates/rssh-app       Desktop application entry point
crates/rssh-core      Shared domain types
crates/rssh-terminal  Terminal grid and VT parser boundary
crates/rssh-renderer  Renderer boundary and damage tracking
crates/rssh-ssh       SSH session boundary
crates/rssh-pty       Local PTY boundary
docs/                 Architecture and planning documents
refs/                 Local reference source cache, ignored by Git
```

## Local Commands

```powershell
cargo fmt --all
cargo test --workspace
cargo run -p rssh-app
cargo run -p rssh-app -- window --frames 3
cargo run -p rssh-app -- window --frames 30 --metrics
cargo run -p rssh-app -- window --frames 120 --metrics -- cmd.exe /K echo window-smoke
cargo run -p rssh-app -- window --frames 120 --metrics --log window.log -- cmd.exe /K echo window-log-smoke
cargo run -p rssh-app -- local
cargo run -p rssh-app -- local --cols 120 --rows 30
cargo run -p rssh-app -- local --mouse
cargo run -p rssh-app -- local -- cmd.exe /C echo console-smoke
cargo run -p rssh-app -- local --log session.log -- powershell -NoProfile -Command "Write-Output logged-smoke"
cargo run -p rssh-app -- ssh --host example.com --user ops --agent
cargo run -p rssh-app -- ssh --target prod
cargo run -p rssh-app -- ssh --target prod -- uname -a
cargo run -p rssh-app -- ssh --host example.com --user ops --password
cargo run -p rssh-app -- ssh --host example.com --user ops --key C:\Users\ops\.ssh\id_ed25519
cargo run -p rssh-app -- ssh --target prod --local-forward 127.0.0.1:15432:db.internal:5432 --no-shell
cargo run -p rssh-app -- ssh --target prod --dynamic-forward 127.0.0.1:1080 --no-shell
cargo run -p rssh-app -- ssh --target prod --log prod.log
cargo run -p rssh-app -- profile local-smoke --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile window-smoke --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile prod-shell --file examples/rssh-profiles.toml
cargo test -p rssh-ssh
```

`window -- <program> [args...]` starts a custom command inside the native
window; without `--`, the native window starts the platform default shell.
Use `window --log PATH` to write visible native-window terminal output to a
session log file.
`local` is the console-hosted startup path. Add `--mouse` when you want terminal
applications to negotiate xterm mouse/focus reporting through PTY output modes.
Bracketed paste mode is negotiated from PTY output automatically.
The console path also answers basic terminal status and device-attribute queries.
XTGETTCAP capability replies include terminal name, 256-color/true-color
markers, OSC 52 clipboard support, and current column/row counts.
OSC 52 clipboard writes and read queries are handled in the console path so
local and OpenSSH-backed terminal programs can use terminal clipboard
integration.
PTY-backed local, window, and OpenSSH child processes receive
`TERM=xterm-256color` and `COLORTERM=truecolor` by default.
Use `--log PATH` on `local` or `ssh` to tee visible terminal output to a session
log file.
`ssh` currently starts the system OpenSSH client inside the same PTY console
runtime, so remote login can use the existing host OpenSSH configuration,
known-host handling, agent, key prompts, and password prompts without exposing
secrets in the R-SSH command line.
Use `--password` as a flag when you want OpenSSH to prompt in the terminal; do
not pass password or key-passphrase values as command arguments.
Use `--target NAME` to reuse an OpenSSH `Host NAME` entry from your existing
SSH config; `--user`, `--port`, `--password`, and `--key` can still override
the generated OpenSSH command when needed.
Add `-- <command> [args...]` after the SSH options to run a remote command
instead of opening the default interactive shell.
Use `--local-forward`, `--remote-forward`, or `--dynamic-forward` with OpenSSH
forward specs for tunnels. Add `--no-shell` when the session should only keep
the tunnel open.
`profile NAME --file PATH` loads a TOML session profile and then starts the
same local, native-window, or SSH runtime. See `examples/rssh-profiles.toml`
for the current file format, including `kind = "local"`, `kind = "window"`,
`kind = "ssh"`, and the optional `log = "path"` field.

## MVP Status

- MVP 1: Terminal core baseline is complete. See `docs/mvp-1-terminal-core.md`.
- MVP 2: Local terminal path is complete as a console-hosted prototype. See
  `docs/mvp-2-local-terminal.md`.
- MVP 3: Native window renderer demo is complete. See
  `docs/mvp-3-native-renderer.md`.
- MVP 4: Live PTY session inside the native renderer is complete. See
  `docs/mvp-4-live-pty-window.md`.
- MVP 5 groundwork: Window smoke runs can print startup, PTY processing,
  rendering, and input-write metrics with `window --metrics`; the SSH crate now
  has a validated config, authentication request model, connector entry point,
  shell-session boundary, `rssh-app ssh` request parsing, and an injectable SSH
  runner path for local input and remote output. The app-level `ssh` command can
  also run the system OpenSSH client through the PTY console runtime as an
  interim backend. See
  `docs/mvp-5-ssh-session-boundary.md`.

## Reference Sources

Reference projects are cloned under `refs/` for local study and are intentionally
not committed. See `refs/README.md` and `docs/research/native-terminal-references.md`
for the current list and what each project is used for.
