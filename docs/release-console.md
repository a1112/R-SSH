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
- `dist/R-SSH-windows-x64/rssh-app.exe version --json`
- `dist/R-SSH-windows-x64/rssh-app.exe doctor --json`
- `dist/R-SSH-windows-x64/rssh-app.exe self-test --json`

The packaged `version --json` smoke test proves that the downloaded executable
can identify its app version, target, console mode, PTY backend, and native SSH
backend. The packaged `doctor --json` smoke test proves that it can start as a
console app and report terminal size, child terminal environment, default shell,
and OpenSSH tool availability. The packaged `self-test --json` smoke test proves
that the downloaded executable can open a real local PTY, spawn a child command,
capture its output, and launch the OpenSSH `ssh`, `sftp`, and `scp` console
tools without opening a network connection.

## Console Startup After Download

After extracting the zip:

```powershell
.\rssh-app.exe doctor
.\rssh-app.exe version --json
.\rssh-app.exe self-test --json
.\rssh-app.exe local
.\rssh-app.exe ssh ops@example.com
.\rssh-app.exe ssh -p 2222 -i C:\Users\ops\.ssh\id_ed25519 -l ops example.com
.\rssh-app.exe ssh ops@example.com uptime -p
.\rssh-app.exe ssh --target prod --preflight
.\rssh-app.exe sftp ops@example.com
.\rssh-app.exe sftp -P 2222 -i C:\Users\ops\.ssh\id_ed25519 ops@example.com
.\rssh-app.exe scp local.txt ops@example.com:/tmp/remote.txt
.\rssh-app.exe scp -P 2222 -i C:\Users\ops\.ssh\id_ed25519 -r logs ops@example.com:/tmp/logs
.\rssh-app.exe scp ops@example.com:/tmp/remote.txt local.txt
.\rssh-app.exe scp ops@example.com --upload local.txt /tmp/remote.txt
.\rssh-app.exe ssh -F C:\Users\ops\.ssh\prod_config -o ProxyJump=bastion prod
.\rssh-app.exe ssh -L 127.0.0.1:15432:db.internal:5432 -D 127.0.0.1:1080 -N prod
.\rssh-app.exe ssh --native --trust-on-first-use --host example.com --user ops --agent
```

## Release Indicators

Use these indicators to decide whether a build is ready for a wider console
pilot:

- Package build success rate: release workflow passes on the target tag.
- Package identity: packaged `version --json` reports the expected version and
  backends.
- Startup health: packaged `doctor --json` exits successfully.
- Local PTY and tool health: packaged `self-test --json` captures the expected
  PTY marker from a child process and verifies that `ssh -V`, `sftp -h`, and
  `scp -h` can launch.
- Console coverage: local shell, positional `[USER@]HOST` SSH/SFTP/SCP
  launches, SSH trailing remote commands, SCP `HOST:PATH` upload/download
  operands, common OpenSSH short options (`ssh -p/-l/-i/-F/-o/-L/-R/-D/-N`,
  `sftp -P/-i`, and `scp -P/-i/-r`), OpenSSH config targets, profiles,
  metrics, and native russh startup remain covered by workspace tests.
- Runtime observability: console sessions can emit `--metrics` or
  `--metrics-json`.
- Safe startup: `--preflight` is available for local, SSH, SFTP, and SCP
  profile or direct launches.
