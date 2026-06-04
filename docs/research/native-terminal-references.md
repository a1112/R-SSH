# Native Terminal Reference Notes

The local `refs/` cache contains shallow reference clones for studying proven
terminal implementations. These projects are research inputs, not vendored
dependencies.

## Study Targets

### WezTerm

Use WezTerm as the main Rust architecture reference:

- `term/` and `wezterm-surface/` for terminal state and surface storage.
- `wezterm-escape-parser/` for escape parser organization.
- `wezterm-gui/src/` for renderer, window, tab, and input flow.
- `pty/` for Windows ConPTY and Unix PTY behavior.
- `wezterm-ssh/` for SSH, SFTP, and agent-forwarding boundaries.
- `wezterm-font/` for font discovery, shaping, and fallback.

### Alacritty

Use Alacritty for a smaller terminal model:

- Terminal grid and scrollback shape.
- Event loop separation.
- Renderer minimalism and performance discipline.

### Ghostty

Use Ghostty for modern native terminal decisions:

- VT core and C ABI direction.
- macOS terminal integration model.
- Examples around terminal stream handling.

### Windows Terminal

Use Windows Terminal for Windows-specific behavior:

- ConPTY expectations.
- Windows input translation.
- DirectWrite and renderer behavior.
- Compatibility behavior for Windows shells and console apps.

### russh and libssh2

Use `russh` as the first Rust implementation candidate and `libssh2` as a
compatibility fallback reference. The R-SSH SSH crate should hide either choice
behind its own session/channel interfaces.

## Reference Hygiene

- Keep reference clones out of Git.
- Record source URLs and commits in `refs/sources.json`.
- Prefer copying ideas and test cases, not source code.
- Check upstream licenses before porting any substantial implementation detail.
