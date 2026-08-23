# Stage 7 GPU backend memory evidence

**Date:** 2026-08-23

**Decision:** **NO-GO for the Stage 7 physical repository split**

## Scope and provenance

- Approved design baseline:
  `fde4649d1d1ee09af8b09326e5564945abcd5c10`.
- Source commit used for the release build, and worktree HEAD immediately before
  this evidence document: `53dec0ed22db168d235670a982652d8f71206ac0`.
- Artifact directory:
  `L:\rssh-evidence\gpu-backend-fixed-20260823`.
- Aggregate artifact:
  `L:\rssh-evidence\gpu-backend-fixed-20260823\aggregate.json`.
- Aggregate SHA-256:
  `40781B82D6F13DDCCD24CAE891A58BF7350B2C09D70A27B7EAF2FCE9739A3829`.
- Aggregate last-write time: `2026-08-23T01:01:48.6089196Z`.
- Measured `rssh-app.exe` SHA-256:
  `9DE1E2AEBF9F80CB9191F31DC2566A00853D78D9A0B7338734488C73F6EB07D1`.
- Measured `rssh-bench-launcher.exe` SHA-256:
  `5AD33D506EBB07C28637E10355EECADAECD12FE08E4C93A6EFE89BBF548F3759`.

The release executables were built from source commit
`53dec0ed22db168d235670a982652d8f71206ac0` with the exact pre-matrix build
command:

```powershell
$env:CARGO_TARGET_DIR='L:\rssh-targets\stage7-release-certification'
cargo build --locked --release -p rssh-app -p rssh-diagnostics --bin rssh-app --bin rssh-bench-launcher
```

This produced the measured executables at
`L:\rssh-targets\stage7-release-certification\release\rssh-app.exe` and
`L:\rssh-targets\stage7-release-certification\release\rssh-bench-launcher.exe`
with the hashes recorded above. The formal matrix then reused those exact
release artifacts with `-SkipBuild`:

```powershell
$env:CARGO_TARGET_DIR='L:\rssh-targets\stage7-release-certification'
pwsh -File scripts/ci/run-gpu-backend-memory-matrix.ps1 `
  -Profile release -Warmups 5 -Samples 30 `
  -OutputDirectory L:\rssh-evidence\gpu-backend-fixed-20260823 -SkipBuild
```

The aggregate identifies schema
`rssh.diagnostics/gpu-backend-memory-matrix-v1`, profile `release`, binary source
`cargo-target`, and `certification_eligible: true`. The run used 80 columns by
24 rows at scale factor 1.0. Each probe discarded five warmups and retained 30
measured runs. Every measured run stabilized for 5,000 ms and then collected
ten Windows Private Working Set samples at 100 ms intervals. Thus each probe
contains 300 measured sample points and the complete matrix contains 1,200.

## Independent artifact validation

The evidence was validated read-only after the run, independently of the
matrix runner. An inline PowerShell assertion block loaded `aggregate.json` and
all paths listed by `evidence.raw_files`, enumerated `raw/*.json`, and performed
these checks:

- exact aggregate schema, release profile, `cargo-target` binary source,
  certification eligibility, warmup/measured counts, 80x24 geometry, scale
  factor 1.0, sampling configuration, byte unit, and 47,185,920-byte target;
- exactly four ordered, successful probes named `cpu`, `dx12`, `vulkan`, and
  `gl`;
- exactly 120 listed raw paths, equality with the 120 JSON files on disk, and
  exactly 30 files for every probe;
- schema `rssh.diagnostics/v2`, Windows x86_64 `empty_window`, ready status,
  zero exit code, zero failures, and exactly ten positive sequential memory
  samples in every raw record;
- exact requested/final renderer and requested/actual backend identity, with
  no GPU fields on CPU records and stable adapter identity across every GPU
  record; and
- nearest-rank p50 and p95 plus maximum recomputed from the 300 raw values per
  probe, including a fresh comparison of p95 against the report-only target.

The validation command exited `0` and reported:

```text
VALIDATION PASSED: aggregate schema/eligibility/configuration, four exact
successful probes, identities, 120 exact raw files, 30 files and 300 samples
per probe, no failures, recomputed nearest-rank statistics, and target flags
all match.
```

No matrix execution was repeated during this validation. A further spot-check
of `raw/cpu-01.json` and `raw/dx12-01.json` confirmed the same v2 schema,
configuration, ten-sample memory payload, readiness, process exit, and expected
renderer identity visible in the full-set assertions.

## Measurement host

The following context was queried locally with read-only
`Get-CimInstance`/registry commands; no network lookup was used.

- OS: `Microsoft Windows 11 专业工作站版 Insider Preview`, version
  `10.0.26300`, build `26300.9032`, display version `26H2`, 64-bit.
- CPU: Intel Core i9-14900K, 24 cores / 32 logical processors.
- Installed memory: 68,003,237,888 bytes.
- Selected physical adapter: NVIDIA GeForce RTX 5060 Ti, Windows driver
  `32.0.16.2002`.
- `Win32_VideoController` reported the adapter PNP vendor/device prefix as
  `PCI\VEN_10DE&DEV_2D04`; these hexadecimal identifiers equal aggregate
  vendor `4318` and device `11524`. The machine also exposed virtual display
  controllers, but no successful GPU raw record selected one of them.

The instance-specific suffix of the PNP device ID is intentionally omitted;
the vendor/device context is sufficient to correlate the hardware without
recording a machine-unique identifier.

## Renderer and adapter identity

| Probe | Requested renderer / backend | Final renderer / actual backend | Adapter identity |
| --- | --- | --- | --- |
| CPU | `cpu` / omitted | `cpu` / omitted | GPU identity fields omitted |
| DX12 | `auto` / `dx12` | `gpu` / `dx12` | `NVIDIA GeForce RTX 5060 Ti`; vendor 4318; device 11524; `discrete-gpu` |
| Vulkan | `auto` / `vulkan` | `gpu` / `vulkan` | `NVIDIA GeForce RTX 5060 Ti`; vendor 4318; device 11524; `discrete-gpu` |
| GL | `auto` / `gl` | `gpu` / `gl` | `NVIDIA GeForce RTX 5060 Ti/PCIe/SSE2`; vendor 4318; device 0; `other` |

All 30 measured raw records for each GPU probe reported exactly the requested
backend and the identity shown above. All four probes succeeded; the aggregate
and all 120 raw records contain no failures.

## Memory results

The memory metric is `windows_private_working_set_bytes`. The report-only Stage
7 target is **47,185,920 bytes (45 MiB)** and is evaluated against p95.

| Probe | Sample points | p50 | p95 | Maximum | 45 MiB target |
| --- | ---: | ---: | ---: | ---: | --- |
| CPU | 300 | 7,630,848 B (7.28 MiB) | 7,692,288 B (7.34 MiB) | 7,704,576 B (7.35 MiB) | met |
| DX12 | 300 | 256,843,776 B (244.95 MiB) | 258,281,472 B (246.32 MiB) | 259,018,752 B (247.02 MiB) | not met |
| Vulkan | 300 | 260,001,792 B (247.96 MiB) | 261,410,816 B (249.30 MiB) | 261,599,232 B (249.48 MiB) | not met |
| GL | 300 | 238,157,824 B (227.12 MiB) | 238,387,200 B (227.34 MiB) | 238,436,352 B (227.39 MiB) | not met |

GL is the lowest-memory successful GPU path, but its p95 is still 227.34 MiB:
about 5.05 times the target and 182.34 MiB over it. DX12 and Vulkan are higher.
The successful CPU-only control is below the target. This A/B result shows that
the large steady-memory delta is associated with GPU-active probes; it is not
temporal before/after proof of when any allocation occurs.

## Startup milestones

The following values were independently summarized from the 30 raw records per
probe. Each cell is nearest-rank p50 / p95 / maximum in milliseconds from
process start. CPU has no GPU-ready milestone by design.

| Probe | Window created | First present | Config ready | GPU ready | Scenario ready | Sampling started |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| CPU | 44 / 58 / 579 | 48 / 62 / 582 | 59 / 74 / 632 | n/a | 48 / 62 / 582 | 5,080 / 5,310 / 6,074 |
| DX12 | 40 / 53 / 54 | 43 / 56 / 57 | 73 / 90 / 91 | 821 / 917 / 941 | 43 / 56 / 57 | 5,072 / 5,088 / 5,090 |
| Vulkan | 45 / 83 / 83 | 48 / 87 / 87 | 187 / 258 / 259 | 742 / 916 / 1,072 | 48 / 87 / 87 | 5,075 / 5,124 / 5,135 |
| GL | 45 / 103 / 104 | 48 / 106 / 109 | 195 / 267 / 403 | 596 / 724 / 915 | 48 / 106 / 109 | 5,074 / 5,133 / 5,137 |

## Decision and next action

The decision is a strict **NO-GO for the Stage 7 physical split**. Every tested
GPU path exceeds the 45 MiB target. The CPU-only pass does not authorize
removing GPU rendering, changing the production renderer contract, or
physically extracting repositories. This evidence also does not authorize any
history rewrite.

Work therefore continues in the monorepo with GPU allocation attribution and
remediation, starting with the lowest-memory GL path while retaining DX12 and
Vulkan comparisons to distinguish backend-specific allocations from shared
WGPU/renderer allocations. If Windows later meets the target, protected native
Linux and macOS baselines are still required before a physical split can be
approved.
