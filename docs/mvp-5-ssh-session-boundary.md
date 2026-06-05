# MVP 5: SSH Session Boundary

MVP 5 starts the remote session path that will sit beside the local PTY path.
It does not connect to a server yet; it defines the validated configuration and
channel contract that a future `russh` adapter must satisfy.

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
  - password
  - private key path with an optional passphrase
  - SSH agent
- `SshAuthError` rejects empty password and empty private-key path inputs before
  a network connection starts.
- `SshConnectRequest` combines a validated session config with one
  authentication method so the future adapter has one stable request object.
- `SshShellConnector` defines the adapter entry point that turns an
  `SshConnectRequest` into an active shell session.
- `SshShellSession` defines the shell channel operations needed by the terminal
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
- `rssh-app ssh` parses user-facing connection options into `SshConnectRequest`,
  including host, user, port, initial terminal size, password auth, private-key
  auth with optional passphrase, and agent auth.
- `rssh-app` has an injectable SSH runner path that passes the parsed request to
  a connector, writes local input bytes into the shell session, streams shell
  output to the local console, and closes the shell session after EOF.
- `rssh-app ssh` can start the system OpenSSH client inside the existing PTY
  console runtime as an interim remote-session backend. It maps host, user, port,
  private-key path, and password-preferred authentication into OpenSSH arguments
  without placing password or passphrase secrets on the command line.

## Run

Start an SSH request with agent authentication through the system OpenSSH
client:

```powershell
cargo run -p rssh-app -- ssh --host example.com --user ops --agent
```

Start with a private key:

```powershell
cargo run -p rssh-app -- ssh --host example.com --user ops --key C:\Users\ops\.ssh\id_ed25519
```

For password authentication, R-SSH asks OpenSSH to prefer password and
keyboard-interactive authentication, then OpenSSH prompts inside the terminal.
R-SSH does not pass the password value on the process command line.

## Verification

```powershell
cargo test -p rssh-ssh
cargo test -p rssh-app ssh_
cargo test -p rssh-app ssh_runner
```

SSH-boundary tests cover:

- successful validated config creation
- host, username, port, and terminal-size validation
- password, private-key, and agent connection request construction
- empty password and empty private-key path rejection
- shell connector trait shape with a mock connector
- shell-session trait shape with a mock channel
- app-level SSH command parsing for agent, password, and private-key requests
- missing host/user and conflicting authentication rejection
- app-level SSH runner behavior with a mock connector and shell session
- app-level SSH runner input forwarding into a mock shell session
- app-level OpenSSH command mapping for target, port, private-key path, password
  prompt policy, secret non-leakage, PTY size, and mouse support

## Explicit Non-Scope

- In-process native `russh` network connections.
- Executing password, key, agent, and host-key authentication through `russh`.
- `russh` adapter wiring.
- SFTP, tunnels, reconnects, and known-host storage.

## Next Milestone

The next SSH step is to keep the OpenSSH PTY backend as a usable compatibility
path while adding a `russh` shell adapter behind `SshShellConnector` and
`SshShellSession`:

1. Add a loopback SSH fixture or mocked channel test.
2. Connect and authenticate through `russh`.
3. Request a remote PTY using `SshSessionConfig::initial_size`.
4. Start the shell and expose read/write/resize/keepalive/close.
5. Feed SSH channel bytes into the existing terminal runtime used by the local
   PTY window.
