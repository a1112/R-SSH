# Console Release Package

This document defines the first downloadable console package for R-SSH.

## Package

The release workflow builds `rssh-app` on `windows-latest` and publishes
`R-SSH-windows-x64.zip`.

The zip contains:

- `rssh-app.exe`
- `rssh-console.cmd`
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
- `dist/R-SSH-windows-x64/rssh-app.exe bench --json --render-frames 30 --idle-ms 200`
- `dist/R-SSH-windows-x64/rssh-app.exe profile --check --file examples/rssh-profiles.toml`
- `dist/R-SSH-windows-x64/rssh-app.exe profile --show window-smoke --file examples/rssh-profiles.toml`
- `dist/R-SSH-windows-x64/rssh-app.exe console --preflight -- cmd.exe /C echo packaged-console-alias-smoke`
- `dist/R-SSH-windows-x64/rssh-console.cmd --preflight -- cmd.exe /C echo packaged-console-launcher-smoke`

The packaged `version --json` smoke test proves that the downloaded executable
can identify its app version, target, console mode, PTY backend, and native SSH
backend. The packaged `doctor --json` smoke test proves that it can start as a
console app and report terminal size, child terminal environment, default shell,
and OpenSSH tool availability. The packaged `self-test --json` smoke test proves
that the downloaded executable can open a real local PTY, spawn a child command,
capture its output, and launch the OpenSSH `ssh`, `sftp`, and `scp` console
tools without opening a network connection. The packaged `bench --json` smoke
test proves that the downloaded executable can run the deterministic
terminal-runtime benchmark and emit parser throughput, p95 chunk latency,
response count, visible output bytes, bell count, scrollback lines, and final
cursor position as machine-readable metrics, plus offscreen renderer p95 frame
time, rendered pixels, rendered pixel throughput, idle CPU usage, process
resident memory, virtual memory, and accumulated CPU time. The packaged profile
checks prove that bundled examples validate and that `window-smoke` resolves
`--metrics-json` for native-window automation. The packaged `console` smoke tests
prove that both the explicit CLI alias and the Windows launcher enter the same
console-hosted PTY path.

## Console Startup After Download

After extracting the zip:

```powershell
.\rssh-app.exe doctor
.\rssh-app.exe version --json
.\rssh-app.exe self-test --json
.\rssh-app.exe bench --json --render-frames 30 --idle-ms 200
.\rssh-app.exe console
.\rssh-console.cmd
.\rssh-app.exe local
.\rssh-app.exe ssh ops@example.com
.\rssh-app.exe ssh -p 2222 -i C:\Users\ops\.ssh\id_ed25519 -l ops example.com
.\rssh-app.exe ssh ops@example.com uptime -p
.\rssh-app.exe ssh --target prod --preflight
.\rssh-app.exe sftp ops@example.com
.\rssh-app.exe sftp -P 2222 -i C:\Users\ops\.ssh\id_ed25519 ops@example.com
.\rssh-app.exe sftp -F C:\Users\ops\.ssh\prod_config -o ProxyJump=bastion prod
.\rssh-app.exe sftp -J bastion -C -vv prod
.\rssh-app.exe sftp -l 4096 prod
.\rssh-app.exe sftp -b batch.txt -B 32768 -R 64 prod
.\rssh-app.exe scp local.txt ops@example.com:/tmp/remote.txt
.\rssh-app.exe scp app.log audit.log ops@example.com:/tmp/logs/
.\rssh-app.exe scp -l 4096 local.txt prod:/tmp/remote.txt
.\rssh-app.exe scp -p local.txt prod:/tmp/remote.txt
.\rssh-app.exe scp -R -s local.txt prod:/tmp/remote.txt
.\rssh-app.exe scp -P 2222 -i C:\Users\ops\.ssh\id_ed25519 -r logs ops@example.com:/tmp/logs
.\rssh-app.exe scp -F C:\Users\ops\.ssh\prod_config -o ProxyJump=bastion local.txt prod:/tmp/remote.txt
.\rssh-app.exe scp -J bastion -C -vv local.txt prod:/tmp/remote.txt
.\rssh-app.exe scp -O -T -B local.txt prod:/tmp/remote.txt
.\rssh-app.exe scp ops@example.com:/tmp/remote.txt local.txt
.\rssh-app.exe scp ops@example.com:/var/log/app.log ops@example.com:/var/log/audit.log logs
.\rssh-app.exe scp ops@example.com --upload local.txt /tmp/remote.txt
.\rssh-app.exe ssh -F C:\Users\ops\.ssh\prod_config -o ProxyJump=bastion prod
.\rssh-app.exe ssh -J bastion -C -vv prod
.\rssh-app.exe ssh -T -W db.internal:5432 prod
.\rssh-app.exe ssh -E ssh-debug.log -Q cipher prod
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
- Benchmark baseline: packaged `bench --json` reports terminal-runtime
  throughput, p95 chunk processing latency, visible output bytes, response
  count, bell count, scrollback lines, and final cursor position from a
  deterministic ANSI/CSI/OSC workload, plus offscreen render frame p95 and
  rendered pixel throughput from `PixelRenderer`, plus idle CPU usage,
  process resident memory, virtual memory, and accumulated CPU time from the
  current process resource sampler.
- Profile readiness: packaged profile checks validate bundled profiles and
  verify that the native-window smoke profile resolves `--metrics-json`.
- Console coverage: explicit `console` launcher, local shell, positional
  `[USER@]HOST` SSH/SFTP/SCP launches, SSH trailing remote commands, SCP
  `HOST:PATH` single-source and multi-source upload/download operands, common
  OpenSSH short options (`ssh -p/-l/-i/-J/-F/-o/-4/-6/-A/-a/-C/-q/-v/-L/-R/-D/-N`,
  SSH control options `-B/-b/-c/-E/-e/-I/-m/-O/-P/-Q/-S/-W/-w/-f/-G/-g/-K/-k/-M/-n/-s/-T/-t/-X/-x/-Y/-y`,
  `sftp -P/-i/-J/-F/-o/-4/-6/-A/-a/-C/-q/-v/-l/-b/-B/-R/-D/-S/-s/-X/-c`, and
  `scp -P/-i/-J/-F/-o/-4/-6/-A/-a/-C/-q/-v/-l/-3/-O/-T/-B/-p/-R/-s/-D/-S/-X/-c/-r`),
  OpenSSH config targets, profiles, metrics, and native russh startup remain
  covered by workspace tests.
- Runtime observability: console sessions can emit `--metrics` or
  `--metrics-json`; non-interactive parser runs can emit `bench --json`.
- Safe startup: `--preflight` is available for console/local, SSH, SFTP, and
  SCP profile or direct launches.
