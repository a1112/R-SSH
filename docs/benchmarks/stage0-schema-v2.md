# Stage 0 GUI diagnostics schema v2

Stage 0 provides a cross-platform, process-owned measurement boundary for an
empty native window and one native SSH GUI pane. The launcher starts a fresh
`rssh-app` child for every run, validates flushed lifecycle markers, waits for
visibly presented readiness, stabilizes for 5 seconds, takes 10 samples at
100 ms intervals, requests graceful shutdown, and force-reaps the child if the
shutdown deadline expires.

## Local invocation

Build and collect the default protocol on Windows:

```powershell
cargo build --locked --release -p rssh-app
cargo build --locked --release -p rssh-diagnostics --bin rssh-bench-launcher
pwsh -File scripts/ci/run-stage0-diagnostics.ps1 -Profile release -Warmups 5 -Samples 30 -SkipBuild
```

On Linux or macOS:

```bash
cargo build --locked --release -p rssh-app
cargo build --locked --release -p rssh-diagnostics --bin rssh-bench-launcher
bash scripts/ci/run-stage0-diagnostics.sh --profile release --warmups 5 --samples 30 --skip-build
```

The scripts run both `empty-window` and `ssh1` at 80×24 and benchmark-only
scale factor 1.0. Warmups are discarded. Every measured run creates a new
launcher and a new app child; the SSH launcher also owns a fresh loopback
fixture and transports its password through an isolated environment channel,
never through the command line or result JSON.

## Wire contract

Every retained run is one JSON object with schema discriminator
`rssh.diagnostics/v2`. Its stable top-level sections are `run`,
`configuration`, `milestones`, `readiness`, `renderer`, `connection`, `memory`,
`process`, and `failures`. Runtime and sampler failures still produce the same
object with a non-empty `failures` array and a failed readiness state.

Marker identity is the tuple `(run_id, pid, scenario)`. Prefixed marker JSON is
rejected if that identity changes, elapsed time decreases, a singleton marker
is duplicated, or the schema is unknown. CPU first-present and later GPU-ready
are both valid; GPU readiness is not required for a CPU result.

Readiness is presentation based:

- `empty-window` becomes ready only after a real non-empty frame is presented;
- `ssh1` presents the masked password prompt before the isolated secret is
  supplied, then emits readiness only after the connected overlay is presented.

The process summary names the sampled child PID and whether teardown was
natural, requested, or forced. The launcher drains stdout and stderr
concurrently into bounded diagnostic tails and samples only that identity-bound
child PID.

## Platform memory semantics

All values use bytes; no platform silently substitutes RSS or virtual memory.

| Platform | `memory.metric` | Exact meaning |
| --- | --- | --- |
| Windows | `windows_private_working_set_bytes` | `PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize` for the identity-bound child process |
| Linux | `linux_pss_bytes` | `Pss:` from `/proc/<pid>/smaps_rollup`, converted from KiB to bytes |
| macOS | `macos_phys_footprint_bytes` | `rusage_info_v4.ri_phys_footprint` for the identity-bound child process |

If the exact native API, counter version, permissions, or process identity is
unsupported, the run fails explicitly. There is no RSS fallback.

Statistics use integer bytes. Mean uses a wide sum, median uses a checked
midpoint, and p50/p95 use nearest rank. A successful run must contain exactly
the configured sample count.

## Artifacts and gates

The default output is:

```text
artifacts/stage0-diagnostics/
├── raw/
│   ├── empty-window-01.json
│   └── ssh1-01.json
└── aggregate.json
```

`raw/` retains every measured v2 record. `aggregate.json` combines the exact
metric within each scenario/platform and records the protocol settings.

The established Windows first-present p95 ≤ 500 ms gate remains blocking in
the protected fixed-runner workflow. Stage 0 steady-memory targets are
report-only observations: 45 MiB for `empty-window` and 60 MiB for `ssh1`.
Crossing either target emits a warning and remains visible in the aggregate and
artifact; it does not fail a build. A missing sample, unsupported sampler,
invalid marker, schema violation, child-lifecycle failure, or missing artifact
does fail the job. Memory targets can graduate to blocking only after the
project records stable same-machine baselines and updates this contract.

Shared pull-request CI runs deterministic schema, parser, state-machine,
sampler-seam, and fixture-process tests. It deliberately does not run absolute
GUI time or memory gates on shared hardware.
