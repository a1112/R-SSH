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
cargo run -p rssh-app -- local --preflight -- cmd.exe /C echo console-preflight-smoke
cargo run -p rssh-app -- local --metrics -- cmd.exe /C echo console-metrics-smoke
cargo run -p rssh-app -- local --metrics-json -- cmd.exe /C echo console-metrics-json-smoke
cargo run -p rssh-app -- window --frames 30 --metrics
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
cargo run -p rssh-app -- ssh -L 127.0.0.1:15432:db.internal:5432 -D 127.0.0.1:1080 -N prod
cargo run -p rssh-app -- ssh --target prod --local-forward 127.0.0.1:15432:db.internal:5432 --no-shell
cargo run -p rssh-app -- ssh --target prod --dynamic-forward 127.0.0.1:1080 --no-shell
cargo run -p rssh-app -- ssh --target prod --osc52 write
cargo run -p rssh-app -- ssh --target prod --log prod.log
cargo run -p rssh-app -- sftp ops@example.com
cargo run -p rssh-app -- sftp -P 2222 -i C:\Users\ops\.ssh\id_ed25519 ops@example.com
cargo run -p rssh-app -- sftp --target prod
cargo run -p rssh-app -- sftp --host example.com --user ops --key C:\Users\ops\.ssh\id_ed25519
cargo run -p rssh-app -- sftp --target prod --log sftp.log
cargo run -p rssh-app -- scp local.txt ops@example.com:/tmp/remote.txt
cargo run -p rssh-app -- scp -P 2222 -i C:\Users\ops\.ssh\id_ed25519 -r logs ops@example.com:/tmp/logs
cargo run -p rssh-app -- scp ops@example.com:/tmp/remote.txt local.txt
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
integration. Use `--osc52 off|write|read-write` on `local`, `ssh`, or
`window` to control whether PTY-side OSC 52 clipboard writes and read queries
are allowed.
PTY-backed local, window, and OpenSSH child processes receive
`TERM=xterm-256color` and `COLORTERM=truecolor` by default.
Use `--log PATH` on `local`, `ssh`, `sftp`, or `scp` to tee visible terminal
output to a session log file.
Use `doctor` before launching console sessions to report the selected PTY
backend, native SSH backend, terminal size, and child terminal environment, and
to verify that the local default shell plus `ssh`, `sftp`, and `scp` are
available; add `--json` for a machine-readable report.
Use `self-test` or `self-test --json` after download to run a local PTY smoke
and verify that `ssh -V`, `sftp -h`, and `scp -h` can launch without opening a
network connection.
Add `--preflight` to `local`, `ssh`, `sftp`, or `scp` when startup should run
the same console dependency check before spawning the PTY child process.
Add `--metrics` to `local`, `ssh`, `sftp`, or `scp` to print human-readable
console runtime metrics after the PTY child process exits, including command,
PTY backend, startup columns/rows, final session state, elapsed time, exit code,
signal, and success state, plus PTY input/output bytes, terminal output bytes,
and resize events.
Use `--metrics-json` on the same commands when a launcher, script, or desktop
UI should consume the metrics as JSON.
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
SPEC -D SPEC -N HOST`.
`sftp` starts the system OpenSSH SFTP client inside the same PTY console
runtime, using the same `--host`/`--target`, `--user`, `--port`, `--agent`,
`--password`, `--key`, and `--log` shape for interactive file transfer; it also
accepts the same positional `[USER@]HOST` target. Common short options
`sftp -P PORT -i KEY HOST` are supported.
`scp` starts the system OpenSSH SCP client inside the same PTY console runtime
for one-shot upload and download transfers. Use `scp local.txt
ops@example.com:/tmp/remote.txt` to upload and `scp
ops@example.com:/tmp/remote.txt local.txt` to download. The longer `--upload
LOCAL REMOTE` and `--download REMOTE LOCAL` forms remain available when the
target is supplied separately; add `-r` or `--recursive` for directories.
Common short options `scp -P PORT -i KEY` are supported.
Add `--native` to use the experimental in-process `russh` path instead of
spawning an interactive OpenSSH session. The native path supports `--host`
direct targets and `--target NAME` entries resolved through `ssh -G`, with
agent, password-prompt, or private-key authentication, including encrypted
private-key passphrase prompts. Use
`--trust-on-first-use` to record a first-time host key in the user's
`.ssh/known_hosts` file and verify it on later connections.
`--accept-unknown-host-key` remains available for insecure test servers only.
Native `--local-forward` and `--dynamic-forward` start in-process listeners and
open russh `direct-tcpip` channels for accepted local TCP or SOCKS5 CONNECT
requests. Native `--remote-forward` requests a server-side TCP listener and
maps incoming forwarded connections back to the configured local target.
Native SSH also honors `--metrics` and `--metrics-json`, reporting the
`NativeRussh` backend, resolved host/user/port, startup size, final session
state, SSH input/output bytes, elapsed time, and exit result without logging
password or key material.
Use `--password` as a flag when you want OpenSSH to prompt in the terminal; do
not pass password or key-passphrase values as command arguments.
Use `--target NAME` to reuse an OpenSSH `Host NAME` entry from your existing
SSH config; `--user`, `--port`, `--password`, and `--key` can still override
the generated OpenSSH command when needed.
Add `-- <command> [args...]` after the SSH options to run a remote command
instead of opening the default interactive shell.
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
Set `metrics = true` in local, SSH, SFTP, or SCP profiles when saved sessions
should print the same console runtime metrics on exit. Use `metrics = "json"`
when saved console sessions should emit machine-readable JSON metrics.
For saved native SSH sessions, set `native = true` and choose
`host_key_policy = "trust-on-first-use"`, `"accept-unknown"`, or
`"reject-unknown"`.
Use `profile --check --file PATH` to validate every configured profile without
starting a local process or network connection; add `--json` for a structured
per-profile report that still exits non-zero when any profile is invalid.

## Downloadable Console Build

The `Release` GitHub Actions workflow builds the Windows console package
`R-SSH-windows-x64.zip`. Manual workflow runs upload it as an artifact; tags
starting with `v` also publish it as a GitHub Release asset. The workflow runs
formatting, tests, clippy, release compilation, and packaged
`rssh-app.exe version --json`, `rssh-app.exe doctor --json`, and
`rssh-app.exe self-test --json` smoke tests before upload. See
`docs/release-console.md`.

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
  runner path for local input and remote output. `rssh-core` also includes a
  shared session lifecycle model for created, connecting, connected,
  disconnected, and closed states. The app-level `ssh` command can also run the
  system OpenSSH client through the PTY console runtime as an interim backend.
  See
  `docs/mvp-5-ssh-session-boundary.md`.

## Reference Sources

Reference projects are cloned under `refs/` for local study and are intentionally
not committed. See `refs/README.md` and `docs/research/native-terminal-references.md`
for the current list and what each project is used for.
