# Reference Source Cache

This directory is a local research cache for terminal and SSH implementations.
The cloned source trees are intentionally ignored by Git so this repository does
not vendor large third-party projects.

Current local references:

| Name | Purpose | Commit | Source |
| --- | --- | --- | --- |
| alacritty | Rust terminal grid, parser boundary, OpenGL renderer reference | `aaf3bd7` | https://github.com/alacritty/alacritty.git |
| ghostty | Native terminal architecture, VT core, platform integration | `bfe633a` | https://github.com/ghostty-org/ghostty.git |
| libssh2 | C SSH2 library compatibility reference | `44a66e8` | https://github.com/libssh2/libssh2.git |
| russh | Pure Rust SSH protocol reference | `f1a0f18` | https://github.com/Eugeny/russh.git |
| wezterm | Rust terminal, PTY, SSH, mux, GPU rendering reference | `577474d` | https://github.com/wezterm/wezterm.git |
| windows-terminal | Windows ConPTY, renderer, terminal state reference | `93bdbfa` | https://github.com/microsoft/terminal.git |

To refresh a reference manually:

```powershell
git -C refs/<name> pull --ff-only
git -C refs/<name> rev-parse --short HEAD
```

Do not commit the cloned reference directories. Commit only this file and
`sources.json`.
