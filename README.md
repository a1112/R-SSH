# R-SSH

R-SSH is a native Rust route for a high-performance SSH terminal client. The
target product shape is closer to XShell than to a web terminal: native window,
GPU text rendering, direct SSH protocol integration, session management, SFTP,
tunnels, logging, and secure key storage.

The repository now includes MVP 1 for the terminal core: a styled cell model,
grid read/write access, basic text/newline parsing, basic SGR color/style
handling, wide CJK glyph placement, and terminal damage tracking.

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
```

## MVP Status

- MVP 1: Terminal core baseline is complete. See `docs/mvp-1-terminal-core.md`.
- MVP 2: Local PTY and SSH shell wiring is next.

## Reference Sources

Reference projects are cloned under `refs/` for local study and are intentionally
not committed. See `refs/README.md` and `docs/research/native-terminal-references.md`
for the current list and what each project is used for.
