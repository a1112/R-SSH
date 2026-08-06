# Web Terminal Design

## Status

Implemented MVP direction: add a separate browser client backed by the existing
local PTY implementation, plus a Tauri shell that hosts the same Web client.
The native `winit`/`wgpu` application remains independent and is not replaced by
a WebView.

## Goal

Provide an R-SSH Web surface that can open an interactive local shell in a
modern browser. The Rust process owns the command, PTY, process lifecycle, and
authorization. The browser owns VT parsing, terminal state, scrollback, text
selection, and presentation through xterm.js.

The first milestone is deliberately narrow: one browser terminal per
WebSocket, one server-configured local profile, and no reconnect/reattach. It
must correctly run ordinary shells and full-screen terminal applications,
including UTF-8 input, IME input, mouse reporting, paste, and resize.

## Non-goals for the first milestone

- Replacing the native window or native GPU renderer.
- Rendering `TerminalRenderSnapshot` in HTML, Canvas, or WebGL.
- Running `rssh-pty` or an operating-system PTY inside WebAssembly.
- Sharing one live PTY between multiple browsers.
- Reattaching to a shell after refresh or network loss.
- Accepting an arbitrary executable, cwd, or environment from a browser.
- Exposing the server to a non-loopback interface without an explicit secure
  deployment mode.
- Browser-side SFTP, tunnels, file upload/download, OSC 52 clipboard access,
  or terminal image protocols.

## Decision

Use a new `rssh-web` Rust binary and a separately built TypeScript frontend.
The production Rust binary serves the compiled frontend and a same-origin
WebSocket endpoint. Development uses the frontend development server with an
explicit proxy to the Rust backend.

Use xterm.js directly instead of `@xterm/addon-attach`. R-SSH needs a versioned
control protocol for open, resize, exit, and errors in addition to the raw PTY
stream. A small owned adapter keeps that protocol visible and testable.

The frontend starts with framework-free TypeScript and Vite. xterm.js owns the
terminal DOM node; a larger UI framework would add little to the single-pane
milestone and would expand the JavaScript code with access to shell input. The
frontend is structured so a later application shell can mount the terminal
controller inside React or another UI framework without changing the wire
protocol.

Use the stable scoped packages:

- `@xterm/xterm` for terminal emulation and presentation.
- `@xterm/addon-fit` for deriving rows and columns from the container.
- `@xterm/addon-webgl` as progressive enhancement, with the default renderer as
  the fallback after initialization or context loss.

Search, links, images, clipboard helpers, ligatures, and experimental Unicode
addons are not enabled in the first milestone. Each changes behavior or the
security boundary and should be added independently.

## Component boundaries

```text
Browser
  WebTerminalController
    xterm.js + FitAddon (+ optional WebglAddon)
    WebSocket protocol v1
             │
             │ binary: unmodified PTY bytes
             │ text: JSON control messages
             ▼
rssh-web
  HTTP static assets and bootstrap authentication
  WebSocket handshake, validation, and bounded queues
  WebPtySession lifecycle supervisor
             │
             ▼
rssh-pty
  PtyCommand -> PtySession
  reader / writer / resize / exit / termination
             │
             ▼
  Unix PTY or Windows ConPTY child process
```

`rssh-terminal`, `TerminalRuntime`, and `rssh-renderer` are intentionally not in
this data path. Running two terminal parsers would produce conflicting query
responses and state. The browser receives the original PTY byte stream and
xterm.js is the only terminal emulator for the Web session.

## Proposed repository layout

```text
crates/rssh-web/
  Cargo.toml
  src/main.rs          CLI and server startup
  src/server.rs        routes, limits, graceful shutdown
  src/auth.rs          bootstrap cookie and handshake checks
  src/protocol.rs      versioned control-message types
  src/session.rs       PTY workers and lifecycle supervisor
  tests/web_pty.rs     real PTY/WebSocket integration tests

web/
  package.json
  vite.config.ts
  src/main.ts
  src/terminal.ts      xterm lifecycle and byte conversion
  src/protocol.ts      protocol-v1 types and validation
  src/styles.css
  tests/

tauri/
  package.json
  app-icon.svg
  src-tauri/           Tauri desktop shell and in-process Web bridge host
```

`rssh-web` remains a separate binary instead of becoming an `rssh-app web`
command. The browser deployment therefore keeps its HTTP runtime and static
assets out of the native application. The Tauri shell reuses the same crate as
an in-process loopback backend, without adding WebView responsibilities to the
already large `rssh-app` module. It depends on `rssh-pty`, not on `rssh-app`.
The Tauri window is undecorated and uses a custom Web titlebar with a drag
region and window controls; those controls are enabled only when the page runs
inside Tauri.

## Server command

The initial command contract is:

```text
cargo run -p rssh-web -- --listen 127.0.0.1:7788
```

Defaults and limits:

- Listen on `127.0.0.1` only.
- Open the platform default shell through `PtyCommand::default_shell()`.
- Preserve the existing `TERM=xterm-256color` and `COLORTERM=truecolor`
  defaults.
- Allow at most 8 live sessions by default.
- Accept terminal sizes from 2-500 columns and 1-300 rows.
- Accept at most 64 KiB in one client input frame and 8 KiB in one control
  frame.
- Bound queued output to 4 MiB per session. Never drop bytes from the middle of
  a terminal stream; terminate a persistently slow connection instead.

All limits are server configuration, not client authority. A later profile
selector sends only an opaque profile id. The server resolves that id to a
configured `PtyCommand`; it never treats client-provided strings as a program,
argument vector, cwd, or environment.

## WebSocket protocol v1

Endpoint:

```text
GET /api/v1/terminal
Upgrade: websocket
```

Text WebSocket frames contain UTF-8 JSON control messages. Binary WebSocket
frames contain terminal bytes. Direction makes binary frames unambiguous:

- Server to browser binary: bytes read from the PTY.
- Browser to server binary: bytes to write to the PTY.

The first client message must be `open`:

```json
{
  "type": "open",
  "protocol": 1,
  "cols": 120,
  "rows": 32,
  "profile": "local-default"
}
```

The server validates the complete message before spawning a process and then
responds:

```json
{
  "type": "opened",
  "protocol": 1,
  "sessionId": "opaque-random-id",
  "cols": 120,
  "rows": 32
}
```

Client control messages after `opened`:

```json
{ "type": "resize", "cols": 132, "rows": 40 }
{ "type": "close" }
```

Server terminal messages:

```json
{ "type": "exit", "code": 0, "signal": null }
{
  "type": "error",
  "code": "PTY_WRITE_FAILED",
  "message": "terminal input could not be written",
  "fatal": true
}
```

Unknown message types, repeated `open`, invalid dimensions, terminal input sent
as a text frame, oversized frames, and binary input before `opened` are protocol
errors. The peer receives a bounded error message and the socket closes with an
appropriate WebSocket close code. Error messages sent to the browser must not
include filesystem paths, environment values, command arguments, or backend
debug strings.

The protocol uses WebSocket ping/pong for liveness rather than JSON heartbeat
messages. Protocol additions are optional fields or new message types. A
breaking semantic change increments the integer protocol version.

## Byte and encoding rules

PTY output stays as bytes from `PtySession::take_reader()` through the
WebSocket. The browser sets `binaryType = "arraybuffer"` and passes a
`Uint8Array` to `Terminal.write`. This preserves split UTF-8 code points and
split escape sequences; xterm.js keeps streaming decoder state across writes.

`Terminal.onData` contains Unicode text and control input. Encode it with
`TextEncoder` to UTF-8 before sending a binary frame. `Terminal.onBinary`
contains a binary string used by legacy reports; convert each code unit to its
low eight bits and send those bytes without UTF-8 expansion. Both event paths
write to the same FIFO client send queue.

Do not convert PTY output to a Rust `String`, JSON string, base64 value, or one
JavaScript string per WebSocket frame.

## Open and resize flow

1. Load the page and authenticate the browser bootstrap request.
2. Create the xterm.js instance and mount it into a visible, sized container.
3. Load `FitAddon`, call `fit()`, and read the resulting `cols` and `rows`.
4. Open the WebSocket and send `open` with that initial size.
5. Spawn `PtySession` only after authentication and message validation.
6. Send `opened`; only then enable terminal input and focus the terminal.
7. Feed subsequent PTY binary frames directly to `Terminal.write`.
8. Use `ResizeObserver` to schedule `fit()` on the next animation frame. Send a
   debounced `resize` only when rows or columns actually changed.
9. The backend validates the new dimensions and calls `PtySession::resize`.

The initial PTY is therefore born at the correct cell size. It does not start
at 80x24 and immediately reflow, which avoids incorrect startup layouts in
shell prompts, `vim`, `tmux`, and other full-screen applications.

## Backend concurrency and backpressure

All `rssh-pty` calls are blocking and must stay off async HTTP executor threads.
Each session owns bounded workers/channels:

- A PTY reader worker owns the value from `take_reader()` and produces ordered
  output chunks.
- A PTY writer worker owns the value from `take_writer()` and performs ordered
  `write_all` plus `flush` operations.
- A lifecycle supervisor owns `PtySession`, applies resize, observes exit, and
  performs bounded termination and master close.
- One outbound queue carries `Output`, `Exit`, and `Error` events to the
  WebSocket sender so final output is delivered before the exit message.

Browser input is paused or rejected before an unbounded queue can form.
Server output is never silently discarded because losing a byte can corrupt VT
parser state. If the 4 MiB output budget remains exhausted, the server reports
`SLOW_CONSUMER`, closes the socket, and terminates the child.

The frontend also watches `WebSocket.bufferedAmount`. It temporarily disables
input when the client-side high-water mark is exceeded and closes the session
if the socket does not recover. Exact high/low-water values should be constants
covered by tests, not scattered magic numbers.

## Session lifecycle

- Closing the terminal UI sends `close`, disables further input, and waits for
  the server result.
- Normal child exit drains PTY output to EOF, enqueues `exit` after the final
  output, and closes the WebSocket normally.
- Browser refresh, network loss, authentication expiry, slow-consumer failure,
  and server shutdown all cancel the session.
- Cancellation closes the input queue, asks the child to terminate within a
  fixed deadline, closes the master, and joins or transfers workers through
  the existing `PtySession` cleanup behavior.
- Dropping a WebSocket task must never detach a child or a reader/writer
  thread.

MVP sessions are socket-owned. There is no grace period and no reattachment:
disconnecting terminates the shell. This invariant keeps lifecycle and access
control auditable. Reconnect later requires a server-owned session registry,
fresh authorization, a bounded replay buffer or headless terminal state, and a
separate protocol revision.

## Security model

A browser terminal is equivalent to interactive shell access. The Web server
must run with the user's normal privileges and must never be installed as a
root/Administrator terminal service.

Loopback mode uses defense in depth:

- Bind only to the configured literal loopback address and reject unexpected
  `Host` headers to reduce DNS-rebinding exposure.
- Generate a high-entropy, process-lifetime bootstrap token. Print a URL
  containing the token once, exchange it for an `HttpOnly`, `SameSite=Strict`
  session cookie, and redirect to a URL without the token.
- Validate the cookie, exact `Origin`, and `Host` on every WebSocket upgrade.
- Never put credentials in the WebSocket URL, browser local storage, logs, or
  terminal output.
- Serve a restrictive Content Security Policy and only bundled, integrity-
  pinned JavaScript. Do not load scripts, fonts, analytics, or addons from a
  CDN.
- Treat title changes, links, and all buffer data as untrusted. Never insert
  terminal data with `innerHTML`. OSC 8 links are not clickable in MVP.
- Do not implement OSC 52 clipboard reads or writes in MVP. Browser copy and
  paste must originate from an explicit user gesture.
- Redact PTY error detail returned to the browser while retaining structured,
  payload-free server logs.

Non-loopback listen addresses are rejected unless a future `--remote` mode is
explicitly enabled. That mode requires TLS (`wss`), real user authentication,
authorization per terminal profile, request/session rate limits, audit events,
idle and absolute lifetimes, and deployment documentation. A reverse proxy by
itself is not considered authentication.

## Frontend behavior

The first page is a single full-viewport terminal with a compact connection
status indicator. Status values are `connecting`, `open`, `closing`, `exited`,
and `failed`. After exit, the page shows the exit status and an explicit
“Start new terminal” action; it does not silently spawn another shell.

Use browser-native focus and IME behavior. Preserve `Ctrl+C`, `Ctrl+D`, and
other terminal keys for xterm.js. Browser-reserved shortcuts remain browser
owned. Paste uses xterm.js input behavior and must respect bracketed-paste mode.
The default renderer is always available; WebGL initialization failure or
context loss falls back without ending the session.

The visual theme may mirror R-SSH colors, but the Web client does not promise
pixel parity with glyphon/wgpu. Cell width, font fallback, scrollback, cursor
shape, selection, and escape-sequence coverage follow the pinned xterm.js
version and are verified as a separate compatibility surface.

## Observability

Record per-session counters without recording terminal contents:

- Open success/failure and startup latency.
- PTY input/output bytes and chunk counts.
- Resize count and last validated dimensions.
- Queue high-water marks and slow-consumer closures.
- Session duration and exit/termination category.
- Active sessions and rejected session-limit attempts.

Session ids in logs are random correlation ids. Do not log keystrokes, PTY
output, bootstrap tokens, cookies, environment variables, or command lines.

## Verification

### Rust tests

- Protocol parsing accepts every valid message and rejects malformed, unknown,
  repeated, out-of-order, and oversized messages.
- Authentication rejects missing/wrong cookies, `Origin`, and `Host`.
- Size and session limits are enforced before PTY spawn.
- A real PTY round trip renders an output marker, accepts input, resizes, exits,
  and drains final output before `exit`.
- Disconnect and server shutdown reap the child and leave no owned worker.
- A blocked writer does not block the async server executor.
- A slow browser exhausts a bounded queue and closes without dropped-byte
  continuation or leaked process state.

### Frontend tests

- `onData` becomes UTF-8 bytes and `onBinary` preserves all 256 byte values.
- Incoming `ArrayBuffer` data reaches `Terminal.write` as `Uint8Array`.
- ResizeObserver bursts produce one changed-size message.
- Input is disabled before `opened`, after exit, and during unrecovered
  backpressure.
- WebGL failure falls back to the default renderer.
- Terminal-derived title/status text never reaches an HTML injection sink.

### Browser end-to-end tests

Run against current stable Chrome/Edge, Firefox, and Safari where available:

- Shell prompt, command echo, exit code, and terminal restart.
- `vim` or an equivalent alternate-screen application.
- `tmux` where installed.
- CJK, emoji, combining characters, IME composition, and split UTF-8 chunks.
- Rapid resize, large output, selection, copy, paste, mouse reporting, and
  bracketed paste.
- Refresh/disconnect cleanup and a rejected unauthorized cross-origin socket.

## Delivery phases

1. Add the protocol types, authenticated loopback WebSocket server, and
   lifecycle integration tests around `rssh-pty`.
2. Add the TypeScript/xterm.js single-terminal client and development proxy.
3. Embed production frontend assets, package `rssh-web`, and run browser E2E
   smoke tests in CI.
4. Add server-owned profile ids and map them to existing local/OpenSSH startup
   configuration without allowing browser-provided commands.
5. Design detached sessions/reconnect, tabs, native `russh` channels, SFTP, and
   remote deployment as separate reviewed features.

## Acceptance criteria

- Opening the authenticated loopback page starts exactly one correctly sized
  PTY only after a valid WebSocket `open` message.
- Shells and full-screen TUI programs are interactive through xterm.js with
  byte-preserving output and ordered input.
- Resize reaches the operating-system PTY and updates a running TUI.
- Normal exit delivers all final output and the exit status.
- Closing or losing the browser connection reaps the child within the bounded
  cleanup path.
- Memory remains bounded when either peer stops consuming data.
- An unauthorized origin cannot start or attach to a terminal.
- Native `rssh-app window` behavior and dependencies remain unchanged.

## References

- [xterm.js project and supported addons](https://github.com/xtermjs/xterm.js)
- [xterm.js encoding guide](https://xtermjs.org/docs/guides/encoding/)
- [xterm.js security guide](https://xtermjs.org/docs/guides/security/)
- [xterm.js terminal API](https://xtermjs.org/docs/api/terminal/classes/terminal/)
