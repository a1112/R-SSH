# MVP 5: SSH Session Boundary

MVP 5 starts the remote session path that will sit beside the local PTY path.
The app can start the system OpenSSH client through the existing PTY console
runtime, while `rssh-ssh` defines the validated configuration and channel
contract that a future in-process `russh` adapter must satisfy.

## Completed Scope

- `rssh-ssh::SshSessionConfig::try_new` trims user-facing host and username
  fields before storing them.
- SSH config validation rejects:
  - empty hosts
  - empty usernames
  - zero ports
  - terminal sizes with zero columns or rows
- `SshConfigError` is a typed error with `Display` and `Error`
  implementations for user-facing startup failures.
- `SshAuthMethod` models the SSH authentication inputs the adapter will need:
  - password prompt request
  - password value from a future secure prompt or secret store
  - private key path with an optional passphrase
  - SSH agent
- `SshAuthError` rejects empty password and empty private-key path inputs before
  a network connection starts.
- `SshConnectRequest` combines a validated session config, one authentication
  method, and a startup mode so the future adapter has one stable request
  object.
- `SshSessionStartup` carries the requested remote startup mode through the
  native SSH boundary: interactive shell, remote command, or no-shell.
- `SshShellConnector` defines the adapter entry point that turns an
  `SshConnectRequest` into an active shell, command, or no-shell session.
- `SshShellSession` defines the SSH channel operations needed by the terminal
  runtime:
  - read SSH channel bytes
  - write user input bytes
  - resize the remote PTY
  - send keepalives
  - close the session
- SSH shell sessions are `Send`, which keeps them compatible with future
  background connection/read tasks.
- `SshSessionError` gives adapters a crate-local error type before the network
  backend is introduced.
- `SshChannel` models the lower-level operations that a native SSH backend must
  expose after it has opened a channel: read, write, PTY resize,
  keepalive, and close.
- `SshChannelOpenPlan` derives the backend channel-opening steps from an
  `SshConnectRequest`: interactive shell and remote command requests use the
  configured initial PTY size, while no-shell requests skip PTY allocation.
- `SshChannelSession` adapts any `SshChannel` implementation into the existing
  `SshShellSession` trait, giving the future `russh` adapter a small tested
  integration point before real network wiring starts.
- `SshChannelOpener` models the backend step that connects, authenticates,
  requests the remote PTY when needed, and opens the requested channel mode.
- `SshChannelConnector` implements `SshShellConnector` for any
  `SshChannelOpener`, wrapping the opened channel in `SshChannelSession` so the
  app-facing shell-session contract stays stable while the native backend is
  introduced.
- `run_shell_with_io` provides the reusable native SSH session pump: it connects
  through an `SshShellConnector`, copies local input into the remote session,
  streams remote output to a caller-provided writer, and closes the session
  after EOF.
- `RusshChannelOpener` starts the in-process native SSH adapter surface with a
  real `russh::client::Config`. The dependency is configured with the `ring`
  crypto backend so Windows builds do not require NASM for `aws-lc-rs`.
- `RusshChannelOpener::connect_async` is the first real native transport entry
  point. It passes the planned socket address, shared russh client config, and
  host-key handler into `russh::client::connect`, returning a connected russh
  handle for the next authentication step.
- `RusshHostKeyPolicy` carries the native adapter's host-key decision rule.
  `RusshChannelOpener` defaults to `RejectUnknown`; callers can explicitly opt
  into `TrustOnFirstUse` for first-use known-host learning or `AcceptUnknown`
  for insecure local fixtures and test-only connections.
- `RusshKnownHosts` records OpenSSH-compatible known-host lines and checks host,
  port, and public-key matches through russh's known-host parser.
- `RusshClientHandler` implements `russh::client::Handler::check_server_key`
  from that policy, giving the `russh` connection path a tested host-key gate
  before authentication and channel opening run. With `TrustOnFirstUse`, an
  unknown host key is written to the configured known-hosts file and accepted;
  later connections must match the recorded key.
- `RusshConnectPlan` derives the stable inputs for
  `russh::client::connect` and the following channel-open step from an
  `SshConnectRequest`: socket host, socket port, username, and
  `SshChannelOpenPlan`.
- `RusshAuthPlan` derives the native authentication branch from an
  `SshConnectRequest`: password value, password prompt request, private-key
  path with optional passphrase, or SSH agent.
- `RusshAuthOutcome` normalizes `russh::client::AuthResult` into the
  crate-local session contract, so failed authentication becomes an
  `SshSessionError` before channel opening.
- `RusshPrivateKeyAuth` loads OpenSSH-compatible private-key files, including
  encrypted keys when a passphrase is present, and can detect whether a key
  needs a passphrase before authentication starts.
- `RusshChannelOpener::authenticate_async` starts the real native
  authentication path. Password-value authentication is wired through
  `russh::client::Handle::authenticate_password`; private-key authentication is
  wired through `russh::client::Handle::authenticate_publickey`; password-prompt
  is resolved by the app before calling the native opener. Agent authentication
  is wired through the system SSH agent and russh's external-signer
  public-key/certificate authentication entry points.
- `RusshChannelOpener::open_session_channel_async` opens a real
  `russh::Channel` with `channel_open_session` after authentication.
- `RusshChannelOpener::start_channel_async` sends the planned PTY, shell, and
  exec requests to the opened `russh::Channel`, matching the startup mode
  derived from `SshConnectRequest`.
- `RusshSshChannel` wraps a live `russh::Channel` in the synchronous
  `SshChannel` trait by bridging read, write, PTY resize, keepalive, and close
  calls through the runtime that owns the native russh session.
- `RusshChannelOpener` implements `SshChannelOpener`, so the existing
  `SshChannelConnector` can create a native russh-backed shell session for
  request shapes currently supported by the native adapter.
- `RusshChannelStartupPlan` converts the channel-open plan into the ordered
  `russh::Channel` requests that must run after authentication: PTY then shell
  for interactive sessions, PTY then exec for remote commands, and no channel
  startup request for no-shell tunnel sessions.
- `rssh-app ssh` parses user-facing connection options into `SshConnectRequest`,
  including host, user, port, initial terminal size, password-prompt auth,
  private-key auth, and agent auth.
- `rssh-app ssh --native --host ...` selects the in-process russh path for
  direct targets. The native app path uses `RusshChannelOpener` through
  `SshChannelConnector`, prompts for a password when `--password` is selected,
  supports `--key PATH` for private-key authentication, prompts for encrypted
  private-key passphrases before connecting through russh, supports agent
  authentication through `--agent`, supports `--trust-on-first-use` for user
  `.ssh/known_hosts` persistence, and keeps `--accept-unknown-host-key` for
  insecure/test-only unknown-host-key acceptance.
- `rssh-app ssh --target NAME` can reuse an existing OpenSSH `Host NAME`
  configuration entry, with optional user, port, key, password-prompt, and size
  overrides. The native path resolves `--target` through `ssh -G` before
  connecting through russh.
- `rssh-app ssh ... -- <command> [args...]` appends a remote command after the
  OpenSSH target. The injectable native connector path maps the same direct
  SSH request to `SshSessionStartup::Command`, while omitting `--` keeps the
  default interactive shell.
- `rssh-app ssh` can pass OpenSSH local, remote, and dynamic forwarding specs
  through `--local-forward`, `--remote-forward`, and `--dynamic-forward`.
  `--no-shell` maps to OpenSSH `-N` for tunnel-only sessions. The injectable
  native connector path maps the same direct SSH request to
  `SshSessionStartup::NoShell`.
- The native app path parses OpenSSH-style local-forward, dynamic-forward, and
  remote-forward specs into a structured native tunnel plan. Local and dynamic
  forwarding start local TCP/SOCKS5 listeners and map accepted connections to
  russh `direct-tcpip` channels. Remote forwarding requests a server-side TCP
  listener and maps incoming `forwarded-tcpip` channels back to the configured
  local target.
- `rssh-app` has an injectable SSH runner path that passes the parsed request to
  a connector, writes local input bytes into the shell session, streams shell
  output to the local console, and closes the shell session after EOF.
- `rssh-app ssh` can start the system OpenSSH client inside the existing PTY
  console runtime as an interim remote-session backend. It maps host, user, port,
  private-key path, and password-preferred authentication into OpenSSH arguments
  without placing password or passphrase secrets on the command line.
- `rssh-app ssh --log PATH` reuses the local PTY console logger to write visible
  SSH/OpenSSH output to a session log file.
- `rssh-app sftp` starts the system OpenSSH SFTP client inside the same PTY
  console runtime, reusing the SSH-style `--host`/`--target`, user, port, auth,
  key, and log arguments for interactive file transfer.
- `rssh-app scp` starts the system OpenSSH SCP client inside the same PTY
  console runtime, reusing the SSH-style target and auth arguments for one-shot
  upload and download transfers.
- `rssh-app profile NAME --file PATH` loads a TOML session profile and maps it
  back through the existing local, native-window, SSH, SFTP, or SCP CLI parser, so
  profile startup keeps the same validation and secret-handling rules as direct
  command-line startup.
- `rssh-app profile --list --file PATH` prints configured profile names and
  kinds so console users can discover saved sessions before starting one.
- `rssh-app profile --check --file PATH` validates every configured profile
  through the same parser used for startup without opening a local process or
  network connection.
- `rssh-app profile --init --file PATH` writes a starter profile file from the
  bundled example and refuses to overwrite existing files unless `--force` is
  supplied.
- `rssh-app profile --show NAME --file PATH` prints the resolved `rssh-app`
  command line for a profile without starting a local process or network
  connection.

## Run

Start an SSH request with agent authentication through the system OpenSSH
client:

```powershell
cargo run -p rssh-app -- ssh --host example.com --user ops --agent
```

Start a native russh SSH request with system-agent authentication:

```powershell
cargo run -p rssh-app -- ssh --native --trust-on-first-use --host example.com --user ops --agent
```

Show the SSH startup options:

```powershell
cargo run -p rssh-app -- ssh --help
```

Reuse an OpenSSH config host alias:

```powershell
cargo run -p rssh-app -- ssh --target prod
```

Override fields for a config host alias:

```powershell
cargo run -p rssh-app -- ssh --target prod --user ops --port 2222 --key C:\Users\ops\.ssh\id_ed25519
```

Run a remote command instead of the default interactive shell:

```powershell
cargo run -p rssh-app -- ssh --target prod -- uname -a
```

Open a local tunnel and keep it alive without starting a remote shell:

```powershell
cargo run -p rssh-app -- ssh --target prod --local-forward 127.0.0.1:15432:db.internal:5432 --no-shell
```

Open a dynamic SOCKS tunnel:

```powershell
cargo run -p rssh-app -- ssh --target prod --dynamic-forward 127.0.0.1:1080 --no-shell
```

Start with a private key:

```powershell
cargo run -p rssh-app -- ssh --host example.com --user ops --key C:\Users\ops\.ssh\id_ed25519
```

Prefer an interactive password prompt:

```powershell
cargo run -p rssh-app -- ssh --host example.com --user ops --password
```

Start the native russh path and record a first-time host key:

```powershell
cargo run -p rssh-app -- ssh --native --trust-on-first-use --host example.com --user ops --password
```

Start the native russh path with a private key:

```powershell
cargo run -p rssh-app -- ssh --native --trust-on-first-use --host example.com --user ops --key C:\Users\ops\.ssh\id_ed25519
```

Write an SSH session log:

```powershell
cargo run -p rssh-app -- ssh --target prod --log prod.log
```

Start an interactive SFTP session through the same console runtime:

```powershell
cargo run -p rssh-app -- sftp --target prod
```

Start SFTP with a private key and session log:

```powershell
cargo run -p rssh-app -- sftp --host example.com --user ops --key C:\Users\ops\.ssh\id_ed25519 --log sftp.log
```

Upload one file with SCP through the same console runtime:

```powershell
cargo run -p rssh-app -- scp --target prod --upload local.txt /tmp/remote.txt
```

Download one file with SCP:

```powershell
cargo run -p rssh-app -- scp --target prod --download /tmp/remote.txt local.txt
```

For password authentication, R-SSH asks OpenSSH to prefer password and
keyboard-interactive authentication, then OpenSSH prompts inside the terminal.
For native encrypted private-key authentication, R-SSH prompts for the key
passphrase before starting the russh authentication step. R-SSH does not accept
password or key-passphrase values on the process command line.

Start from a reusable profile file:

```powershell
cargo run -p rssh-app -- profile --init --file rssh-profiles.toml
cargo run -p rssh-app -- profile --check --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile --list --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile --show prod-shell --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile local-smoke --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile window-smoke --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile prod-shell --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile prod-files --file examples/rssh-profiles.toml
cargo run -p rssh-app -- profile prod-upload --file examples/rssh-profiles.toml
```

The current profile file format is TOML:

```toml
[profiles.prod-shell]
kind = "ssh"
target = "prod"
user = "ops"
auth = "agent"
cols = 120
rows = 32
log = "prod.log"

[profiles.prod-files]
kind = "sftp"
target = "prod"
auth = "agent"
log = "sftp.log"

[profiles.prod-upload]
kind = "scp"
target = "prod"
auth = "agent"
upload = ["local.txt", "/tmp/remote.txt"]
log = "prod-upload.log"

[profiles.local-smoke]
kind = "local"
command = ["powershell", "-NoProfile", "-Command", "Write-Output rssh-profile-smoke"]

[profiles.window-smoke]
kind = "window"
frames = 120
metrics = true
osc52 = "write"
command = ["cmd.exe", "/K", "echo", "rssh-window-profile-smoke"]
```

## Verification

```powershell
cargo test -p rssh-ssh
cargo test -p rssh-app ssh_
cargo test -p rssh-app sftp
cargo test -p rssh-app scp
cargo test -p rssh-app ssh_runner
cargo test -p rssh-app profile
cargo test -p rssh-app log
```

SSH-boundary tests cover:

- successful validated config creation
- host, username, port, and terminal-size validation
- password-prompt, password-value, private-key, and agent connection request construction
- empty password and empty private-key path rejection
- shell connector trait shape with a mock connector
- shell-session trait shape with a mock channel
- `SshChannelSession` delegation from a mock lower-level channel into the
  `SshShellSession` read/write/resize/keepalive/close contract
- `SshChannelConnector` delegation from a mock channel opener into an
  app-facing `SshShellSession`
- `SshChannelOpenPlan` PTY/startup derivation for interactive shell, remote
  command, and no-shell requests
- `SshSessionStartup` defaults, remote command validation, and direct native
  SSH connector propagation for remote-command and no-shell requests
- `run_shell_with_io` behavior for streaming remote output, forwarding local
  input, and closing the shell session
- `RusshChannelOpener` construction against a real `russh::client::Config`
- `RusshChannelOpener::connect_async` API shape against the real
  `russh::client::connect` transport entry point
- `RusshHostKeyPolicy` defaults and explicit insecure/test-only accept-unknown
  host-key behavior through the `russh` client handler
- OpenSSH-compatible known-host writing and matching for the native russh path
- native trust-on-first-use host-key recording through the russh client handler
- `RusshConnectPlan` derivation for socket address, username, and channel-open
  plan from a validated SSH request
- `RusshAuthPlan` derivation for password, password-prompt, private-key, and
  agent authentication branches
- `RusshAuthOutcome` success/failure normalization from russh authentication
  results
- `RusshPrivateKeyAuth` loading of unencrypted and encrypted private-key files,
  plus encrypted-key passphrase detection
- `RusshChannelOpener::authenticate_async` API shape against the real russh
  password and public-key authentication entry points
- `RusshChannelOpener::open_session_channel_async` API shape against
  `russh::client::Handle::channel_open_session`
- `RusshChannelOpener::open_direct_tcpip_channel_async` and blocking
  direct-tcpip entrypoints for native local and dynamic forwarding
- `RusshChannelOpener::start_channel_async` API shape against russh channel
  PTY, shell, and exec startup requests
- `RusshSshChannel` implementation of the crate-local `SshChannel` trait
- `RusshSshChannel` split reader/writer entrypoint for bidirectional local
  forwarding pumps
- `RusshChannelOpener` implementation of the crate-local `SshChannelOpener`
  trait with `RusshSshChannel` as its channel type
- `RusshChannelStartupPlan` request ordering for shell, remote command, and
  no-shell startup modes
- app-level SSH command parsing for agent, password-prompt, and private-key requests
- app-level native SSH backend selection with `--native`
- app-level native password prompt resolution before connecting through russh
- app-level native encrypted private-key passphrase prompt resolution before
  connecting through russh
- native russh agent-auth dispatch through the authentication backend
- native local-forward spec parsing into bind and target endpoints
- native local-forward startup before shell startup, using an injectable
  forward starter
- native dynamic-forward bind parsing, SOCKS5 no-auth CONNECT parsing, and
  dynamic-forward startup before shell startup
- native OpenSSH config-target parsing through resolved `ssh -G` output before
  connecting through russh
- explicit `--accept-unknown-host-key` parsing and russh host-key policy mapping
- explicit `--trust-on-first-use` parsing, native host-key policy mapping, and
  default `.ssh/known_hosts` path selection
- rejection of native remote forwarding
- command-line rejection for password and key-passphrase secret values
- app-level OpenSSH config-target parsing with optional user, port, key,
  password-prompt, and size overrides
- remote command parsing after `--` for direct and OpenSSH config targets
- OpenSSH command generation that appends remote command arguments after the
  target
- OpenSSH local, remote, and dynamic forwarding argument generation before the
  target
- `--no-shell` parsing, OpenSSH `-N` generation, and rejection when combined
  with remote commands
- missing host/user and conflicting authentication rejection
- app-level SSH runner behavior with a mock connector and shell session
- app-level SSH runner input forwarding into a mock shell session
- app-level OpenSSH command mapping for target, port, private-key path, password
  prompt policy, secret non-leakage, PTY size, and mouse support
- app-level profile command parsing and TOML profile loading for local,
  native-window, and SSH startup paths
- app-level local and SSH log path parsing plus visible-output log tee behavior

## Explicit Non-Scope

- Switching the app-level `rssh-app ssh` command from the OpenSSH PTY
  compatibility path to the native russh adapter by default.
- SFTP, complete in-process native tunnels, and reconnects.

## Next Milestone

The next SSH step is to keep the OpenSSH PTY backend as a usable compatibility
path while adding a `russh` shell adapter behind `SshShellConnector`,
`SshChannel`, and `SshShellSession`:

1. Add a loopback SSH fixture.
2. Implement a `russh`-backed `SshChannelOpener`.
3. Connect and authenticate through `russh`.
4. Request a remote PTY using `SshChannelOpenPlan::pty_size`.
5. Feed SSH channel bytes into the existing terminal runtime used by the local
   PTY window.
