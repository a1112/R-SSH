# R-SSH Native Architecture

R-SSH will be a native Rust SSH terminal client. The design separates terminal
state, connection I/O, rendering, and product UI so each layer can be tested and
replaced independently.

## Goals

- Provide an XShell-like desktop SSH client with native performance.
- Keep the terminal core independent from SSH, local PTY, and rendering.
- Use GPU rendering for dense terminal output and large scrollback.
- Support Windows first, without blocking later Linux and macOS support.
- Store session metadata locally and secrets through OS-backed secure storage.

## Non-Goals

- Do not build on Electron, WebView, or `xterm.js` for the main terminal view.
- Do not shell out to `ssh.exe` as the primary connection engine.
- Do not vendor third-party terminal projects into this repository.
- Do not implement every XShell feature before the terminal and SSH loop is
  stable.

## Layers

```text
Application shell
  Tabs, panes, session tree, command palette, settings, logs

Session runtime
  Owns terminal instances, connection tasks, resize events, lifecycle, replay

Connection layer
  SSH channel, local PTY, future serial/Telnet, SFTP, tunnels

Terminal core
  VT parser, terminal grid, scrollback, selection, hyperlinks, mouse modes

Renderer
  Font shaping, glyph atlas, damage tracking, GPU draw batches, presentation

Storage and security
  SQLite metadata, known hosts, DPAPI/Keychain/Secret Service, audit logs
```

## Primary Data Flow

```text
SSH or PTY byte stream
  -> terminal parser
  -> terminal grid and scrollback mutation
  -> damage regions
  -> renderer batches
  -> GPU presentation

keyboard, mouse, paste, resize
  -> input encoder
  -> active connection channel
  -> remote shell or local PTY
```

## Crate Boundaries

- `rssh-core`: shared domain types such as session IDs, terminal size, session
  lifecycle state, and common error categories.
- `rssh-terminal`: VT parser boundary, grid, scrollback, selection, and input
  encoding.
- `rssh-renderer`: renderer state, damage tracking, font atlas, and future
  `wgpu` integration.
- `rssh-ssh`: SSH session abstraction. Start with `russh`; keep `libssh2`
  compatibility isolated behind this crate if needed.
- `rssh-pty`: local shell support through Windows ConPTY and Unix PTY.
- `rssh-app`: desktop entry point and user-facing application shell.

## Technology Choices

Recommended initial stack:

- `winit` for cross-platform native windows and input.
- `wgpu` for GPU rendering across DirectX, Metal, Vulkan, and OpenGL backends.
- `cosmic-text` first for shaping and font fallback; evaluate HarfBuzz bindings
  if terminal compatibility requires lower-level control.
- `russh` first for pure Rust SSH; keep `libssh2` as an optional fallback.
- `portable-pty` as the first local PTY implementation reference, with direct
  platform adapters later if the abstraction becomes limiting.
- `rusqlite` or `sqlx` for local session metadata. Prefer `rusqlite` for a small
  desktop app unless async database access becomes useful.
- `keyring` or platform-specific APIs for secrets. Never store passwords or
  private key passphrases in SQLite.

## Error Handling

- Connection failures should keep the session tab visible with a clear terminal
  status line and retry action.
- Host key mismatch is a blocking security error and must never be auto-accepted.
- Parser errors should be counted and logged, but unknown escape sequences should
  not crash the session.
- Renderer device loss should rebuild renderer state from terminal grid and
  scrollback.
- Secret storage failures should degrade to "not saved" rather than plaintext
  persistence.

## Testing Strategy

- Unit-test terminal grid behavior and VT parser conformance with recorded
  byte streams.
- Use snapshot tests for terminal state after escape sequences.
- Use loopback SSH fixtures before touching real servers.
- Use local PTY integration tests gated by platform.
- Use renderer pixel/screenshot tests once `wgpu` is introduced.
- Use fuzzing for the VT parser before handling untrusted network streams at
  scale.
