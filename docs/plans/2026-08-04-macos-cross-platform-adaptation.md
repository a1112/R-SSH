# macOS Cross-Platform Adaptation

## Goal

Make the existing Windows/Unix terminal architecture behave as a native macOS
application without forking the terminal, renderer, SSH, or PTY implementations.
The supported macOS baseline is macOS 11 or newer on Intel and Apple Silicon.

## Design

The adaptation keeps shared behavior in the existing crates and limits platform
policy to narrow boundaries:

| Boundary | Windows | macOS | Other Unix |
| --- | --- | --- | --- |
| PTY backend | ConPTY | Unix PTY | Unix PTY |
| GPU/window backend | winit + wgpu | winit + Metal through wgpu | winit + wgpu |
| Default shell fallback | `cmd.exe` | `/bin/zsh` | `/bin/sh` |
| Mutable app state | `%LOCALAPPDATA%\\R-SSH` | `~/Library/Application Support/R-SSH` | `$XDG_STATE_HOME/rssh` or `~/.local/state/rssh` |
| Clipboard | Native arboard provider | Native arboard provider | Native arboard provider |
| Primary selection | Available only where implemented | Deliberately unsupported | Deliberately unsupported |
| Open hyperlinks | `rundll32` | `/usr/bin/open` | `xdg-open` |

`XDG_STATE_HOME` remains an explicit override on macOS. This preserves hermetic
CLI/test workflows while Finder-launched applications use the native Library
location by default.

macOS keyboard behavior stays in the shared input reducer. Command-key defaults
cover tabs, windows, copy/paste, search, font sizing, reload, minimize, and hide;
terminal Control-key input continues to reach the PTY when no application
binding consumes it. IME forwarding and simple fullscreen remain isolated
behind `target_os = "macos"` window extensions.

## Packaging Contract

The macOS artifact contains both:

- `R-SSH.app/Contents/MacOS/rssh-app` for Finder/LaunchServices startup.
- `rssh-app` and `rssh-console.sh` launchers for shell workflows.

`Info.plist` declares the bundle identity, concrete architecture, Retina support,
automatic GPU switching, and a macOS 11 minimum. Release signing, hardened
runtime, notarization, and stapling remain in the protected release workflow.

## Verification

Run on each native architecture:

```sh
cargo check --workspace --all-targets
cargo test --workspace
cargo run -p rssh-app -- doctor --json
cargo run -p rssh-app -- self-test --json
cargo build --locked --release -p rssh-app
bash scripts/ci/package-native.sh \
  --binary target/release/rssh-app \
  --package-root target/package/rssh-macos \
  --artifact-name rssh-macos-aarch64-unsigned.tar.gz \
  --runtime-target macos-aarch64 \
  --pty-backend unix-pty \
  --version 0.1.0 \
  --unsigned
```

For Intel, substitute `--target x86_64-apple-darwin`, the corresponding binary
path, and `macos-x86_64`. Package smoke additionally verifies bundle metadata,
CLI startup, PTY operation, OpenSSH tools, and native ten-frame presentation.

## Remaining Release Evidence

Local validation does not replace protected code signing/notarization or
hardware coverage. Before declaring a release, record results for Intel and
Apple Silicon, input methods, Retina/external displays, sleep/wake, fullscreen,
clipboard, and signed package launch under Gatekeeper.

## Local ARM64 Evidence (2026-08-04)

The adaptation was exercised on Apple Silicon with Rust 1.89:

- `cargo check --workspace --all-targets` passed.
- All 45 `rssh-pty` tests and the new platform/raw-mode/frame-limit tests passed.
- `doctor --json` and `self-test --json` passed with `/bin/zsh`, Unix PTY, and
  `/usr/bin/ssh`, `/usr/bin/sftp`, and `/usr/bin/scp` detected.
- The release ARM64 `.app` passed the complete `package-smoke.sh` contract,
  including manifest/plist verification, benchmark, non-TTY console launcher,
  real OpenSSH loopback, and ten presented Metal frames on an Apple M2.
- `cargo clippy --workspace --all-targets` completed with existing repository
  warnings.

The aggregate `cargo test --workspace` result is not yet a green macOS gate:
4,084 tests passed and 162 monolithic app tests failed. Representative failures
reproduce in isolation and cover existing platform-assumptive fixtures such as
non-UTF-8 Darwin filenames, primary selection, Windows input encoding, and
large static configuration/mouse suites. They are outside this adaptation's
runtime/package boundaries, but remain explicit debt before claiming complete
workspace certification on macOS.
