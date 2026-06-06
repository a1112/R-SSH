# Console Release Package

This document defines the first downloadable console package for R-SSH.

## Package

The release workflow builds `rssh-app` on `windows-latest` and publishes
`R-SSH-windows-x64.zip`.

The zip contains:

- `rssh-app.exe`
- `README.md`
- `LICENSE`
- `examples/rssh-profiles.toml`

## Release Triggers

- Manual run: GitHub Actions `Release` workflow through `workflow_dispatch`.
- Versioned release: push a tag that starts with `v`, for example `v0.1.0`.

Tag releases create a GitHub Release and attach `R-SSH-windows-x64.zip`.
Manual runs upload the same zip as a workflow artifact.

## Verification Gates

The release package is not uploaded until all gates pass:

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo build --release -p rssh-app`
- `dist/R-SSH-windows-x64/rssh-app.exe doctor --json`

The packaged `doctor --json` smoke test proves that the downloaded executable
can start as a console app and report the selected PTY backend, native SSH
backend, terminal size, child terminal environment, default shell, and OpenSSH
tool availability.

## Console Startup After Download

After extracting the zip:

```powershell
.\rssh-app.exe doctor
.\rssh-app.exe local
.\rssh-app.exe ssh --target prod --preflight
.\rssh-app.exe ssh --native --trust-on-first-use --host example.com --user ops --agent
```

## Release Indicators

Use these indicators to decide whether a build is ready for a wider console
pilot:

- Package build success rate: release workflow passes on the target tag.
- Startup health: packaged `doctor --json` exits successfully.
- Console coverage: local shell, OpenSSH SSH, SFTP, SCP, profiles, metrics, and
  native russh startup remain covered by workspace tests.
- Runtime observability: console sessions can emit `--metrics` or
  `--metrics-json`.
- Safe startup: `--preflight` is available for local, SSH, SFTP, and SCP
  profile or direct launches.
