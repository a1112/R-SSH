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
- `SshShellSession` defines the shell channel operations needed by the terminal
  runtime:
  - read SSH channel bytes
  - write user input bytes
  - resize the remote PTY
  - send keepalives
  - close the session
- `SshSessionError` gives adapters a crate-local error type before the network
  backend is introduced.

## Verification

```powershell
cargo test -p rssh-ssh
```

SSH-boundary tests cover:

- successful validated config creation
- host, username, port, and terminal-size validation
- shell-session trait shape with a mock channel

## Explicit Non-Scope

- Real SSH network connections.
- Password, key, agent, and host-key authentication.
- `russh` adapter wiring.
- SFTP, tunnels, reconnects, and known-host storage.

## Next Milestone

The next SSH step is a `russh` shell adapter behind `SshShellSession`:

1. Add a loopback SSH fixture or mocked channel test.
2. Connect and authenticate through `russh`.
3. Request a remote PTY using `SshSessionConfig::initial_size`.
4. Start the shell and expose read/write/resize/keepalive/close.
5. Feed SSH channel bytes into the existing terminal runtime used by the local
   PTY window.
