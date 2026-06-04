# R-SSH Native Rust Design

## Context

R-SSH is starting as a native high-performance desktop SSH terminal client. The
product target is an XShell-like tool: multi-session SSH, terminal rendering,
SFTP, tunnels, logs, key handling, and operational ergonomics. The current
decision is to use the Rust native route instead of an Electron or WebView route.

## Options Considered

### Recommended: Native Rust Terminal Stack

Build the application with Rust, native windows, a custom terminal core, and GPU
text rendering. SSH and local PTY are adapter layers that feed byte streams into
the same terminal core.

Trade-off: more initial engineering work, but the best long-term control over
performance, input correctness, memory safety, and product differentiation.

### Faster: WebView or Electron with xterm.js

Use `xterm.js` for the terminal and a Rust/Node backend for SSH. This is faster
to ship and proven for many tools.

Trade-off: easier MVP, but the core terminal experience is bounded by web
rendering and browser integration. This does not match the native high-performance
goal.

### Simplest: Wrap OpenSSH

Run `ssh.exe` or OpenSSH as a child process and render its output through a local
PTY-like bridge.

Trade-off: quick for login, but weak control over authentication, SFTP, host key
UX, tunnels, reconnects, and session lifecycle.

## Decision

Use the native Rust route:

- Terminal core is in-process and independent from transport.
- SSH is integrated through a Rust-facing library boundary.
- Rendering is custom and GPU-backed.
- Product UI stays native and can evolve without changing the terminal core.

## Architecture

The architecture has six boundaries:

1. `rssh-app`: desktop shell, tabs, settings, session tree, and command surface.
2. `rssh-core`: shared IDs, sizes, capability flags, and common types.
3. `rssh-terminal`: VT parser, grid, scrollback, input encoder, selection.
4. `rssh-renderer`: damage tracking, text shaping, glyph atlas, GPU draw batches.
5. `rssh-ssh`: SSH authentication, channel I/O, resize, keepalive, SFTP, tunnels.
6. `rssh-pty`: local shell integration through ConPTY or Unix PTY.

## Data Flow

Remote output:

```text
SSH channel bytes
  -> parser
  -> terminal state mutation
  -> dirty region
  -> renderer
  -> window frame
```

User input:

```text
keyboard/mouse/paste/resize
  -> terminal input encoder
  -> SSH channel or PTY writer
  -> remote shell
```

## Error Handling

- Network disconnects keep the tab open and display recoverable session state.
- Host key changes are blocking security errors.
- Unknown escape sequences are ignored or logged, not fatal.
- Renderer device loss rebuilds renderer resources from terminal state.
- Secret persistence failures never fall back to plaintext.

## Testing

The first implementation should be test-first:

- Unit tests for shared domain types.
- Parser and grid tests from small VT byte streams.
- SSH loopback tests before real server tests.
- Platform-gated PTY tests.
- Renderer tests after the first `wgpu` prototype.
- Fuzz tests for parser and input decoder before broad protocol support.

## Milestones

1. Workspace and planning baseline.
2. Terminal grid and parser MVP.
3. Local PTY MVP on Windows.
4. SSH shell MVP with resize and keepalive.
5. GPU text renderer prototype.
6. Session manager, tabs, and settings.
7. SFTP, tunnels, logging, and secure storage.
8. Packaging and release pipeline.
