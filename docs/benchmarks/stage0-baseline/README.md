# Stage 0 Windows x64 baseline

This baseline was recorded on 2026-08-18 from commit
`369869cb8bc76d754f16a8aea76290dcb8bc1b97` using the Stage 0 schema-v2
protocol described in [stage0-schema-v2.md](../stage0-schema-v2.md).

## Measurement host

- OS: Microsoft Windows 11 Pro for Workstations Insider Preview 10.0.26300,
  x86_64
- CPU: Intel Core i9-14900K, 24 cores / 32 logical processors
- Memory: 68,003,237,888 bytes installed
- Rust: rustc 1.89.0 (`29483883e`, LLVM 20.1.7)
- Cargo: 1.89.0 (`c24e10642`)
- Build profile: locked release
- Window protocol: 80 columns x 24 rows at scale factor 1.0

The local Cargo target directory was placed on a separate local volume because
the repository volume did not have enough free space for the workspace-wide
all-targets build. The measured release executables were copied unchanged to
the runner's conventional `target/release` path before the Stage 0 run.

## Commands

```powershell
cargo build --locked --release -p rssh-app
cargo build --locked --release -p rssh-diagnostics --bin rssh-bench-launcher
pwsh -NoProfile -File scripts/ci/run-ssh-gui-startup.ps1 `
  -Profile release -Warmups 5 -Samples 30 -SkipBuild
pwsh -NoProfile -File scripts/ci/run-stage0-diagnostics.ps1 `
  -Profile release -Warmups 5 -Samples 30 `
  -OutputDirectory artifacts/stage0-diagnostics -SkipBuild
```

Stage 0 discarded five warmups per scenario. Each of the 30 measured runs
stabilized for five seconds and then collected ten samples at 100 ms intervals
from the exact child process identity. On Windows the metric is private working
set bytes; it is not RSS, commit size, or a process-tree aggregate.

## Results

| Contract | Result | Status |
| --- | ---: | --- |
| SSH GUI first-present p50 | 43.83 ms | pass (<=400 ms) |
| SSH GUI first-present p95 | 71.52 ms | pass (<=500 ms) |
| SSH GUI first-frame Private Bytes p95 | 9,465,856 bytes (9.03 MiB) | pass (<=55 MiB) |
| SSH GUI first-frame Private Bytes maximum | 9,531,392 bytes (9.09 MiB) | pass (<60 MiB) |
| `empty-window` steady private working set p95 | 312,672,256 bytes (298.19 MiB) | report-only target not met (45 MiB) |
| `ssh1` steady private working set p95 | 57,421,824 bytes (54.76 MiB) | report-only target met (60 MiB) |

The first-present contract is a blocking fixed-runner gate. The Stage 0 steady
memory observations are intentionally report-only: the high `empty-window`
measurement is recorded as a decomposition target for the next project-split
stage and does not weaken or replace the first-frame gate.

The Windows run produced 60 raw schema-v2 JSON records plus one aggregate JSON
artifact. Linux PSS and macOS `phys_footprint` sampler seams passed their
deterministic tests in this change, but native Linux and macOS baseline runs
still require their respective fixed runners and are not represented here.
