# R-SSH

This workspace owns SSH authentication, host-key verification, channels, remote
shell/commands and forwarding primitives. It has no window, renderer, font,
terminal emulator, Web or Tauri dependency, including optional features.

`cargo build --locked -p rssh-cli` builds `rssh`. Run `rssh --help` for the initial
command-line interface. This is an experimental native client, not an OpenSSH
compatible replacement: OpenSSH configuration, scp/sftp commands and forwarding
CLI options are not yet implemented. Forwarding remains available in the library.

Unknown and changed host keys are rejected. Supply a previously verified
known_hosts file. Passwords are requested through a hidden terminal prompt, never
through command-line arguments.

`cargo test --locked --workspace --all-targets` runs the SSH and fixture tests.
The graphical product and terminal adapter live in the [R-Term repository](https://github.com/a1112/R-Term).

## Repository boundary

R-SSH owns SSH transport and its `rssh` command. All graphical UI, terminal
emulation, local terminals and rendering belong to R-Term. `cargo build --locked`
builds the SSH command by default; there are no GUI dependencies.

The original Git history remains available. The split is a repository ownership
change, not an OpenSSH-compatibility or cross-platform certification claim.
See [split record](docs/formal-split.md) for provenance and rollback.
