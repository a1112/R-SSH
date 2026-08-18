# Stage 4 snapshot/cache contract

Stage 4 makes terminal render snapshots structurally shared and puts explicit,
independent byte limits around retained snapshot rows and inline-image payloads.
The renderer output contract is unchanged: a full snapshot and the equivalent
damage-updated snapshot must compare equal for ASCII, CJK, emoji, hyperlink, and
inline-image fixtures.

## Local invocation

Build the release application and run the deterministic evidence collector on
Windows:

```powershell
cargo build --locked --release -p rssh-app
pwsh -File scripts/ci/run-stage4-snapshot-cache.ps1 `
  -Profile release -Warmups 2 -Samples 7 -SkipBuild
```

The script runs the harnessless `snapshot_memory` Rust benchmark for 80×24 and
200×60 terminals, then executes the existing `ansi-scroll-query` parser workload.
When `artifacts/stage0-diagnostics/aggregate.json` exists, it also evaluates the
same-machine Stage 0 memory trend. Set `CARGO_TARGET_DIR` before both commands when
using an external Cargo target directory.

## Snapshot evidence

Each snapshot benchmark emits one JSON object with these fields:

- `columns`, `rows`, `full_iterations`, and `damage_iterations` identify the
  deterministic workload;
- `full_mean_ns` and `damage_mean_ns` report average construction cost;
- `active_snapshot_bytes` reports the current snapshot estimate independently of
  cache retention;
- `retained_snapshot_bytes` and `retained_image_bytes` report the two bounded
  cache classes;
- `row_reuse_permille` reports unchanged row identity reuse after damage.

The benchmark includes CJK, emoji, hyperlink, and inline-image content and fails
before emitting a record if full and damage snapshots differ.

One local Windows x64 release run on 2026-08-18 observed:

| Grid | Full mean | Damage mean | Active bytes | Retained snapshot/image | Row reuse |
| --- | ---: | ---: | ---: | ---: | ---: |
| 80×24 | 113,593 ns | 9,568 ns | 44,072 | 44,068 / 4 | 96.4% |
| 200×60 | 294,675 ns | 14,724 ns | 109,484 | 109,480 / 4 | 98.5% |

These values are diagnostic observations, not portable absolute latency limits.
The fixed runner artifact is authoritative for release evidence.

## Blocking gates

The protected Windows x64 fixed-performance job requires:

1. exactly one valid snapshot record for 80×24 and one for 200×60;
2. successful full/damage equivalence inside each benchmark workload;
3. median `ansi-scroll-query` throughput of at least 98% of the checked-in
   same-machine baseline;
4. Stage 0 `empty-window` and `ssh1` p95 memory values strictly below the recorded
   pre-Stage-4 values of 312,672,256 and 57,421,824 bytes respectively.

The memory comparison consumes the Stage 0 aggregate produced earlier in the same
job, so metric meaning, dimensions, DPI, and runner identity remain aligned. Shared
pull-request runners execute deterministic contract tests only; they do not enforce
absolute timing or memory limits.

## Output artifact

The fixed job uploads `artifacts/stage4-snapshot-cache/report.json` with schema
`rssh.stage4-snapshot-cache/v1`. It contains the two snapshot records, parser
baseline/minimum/median/ratio, and the measured Stage 0 memory comparison. Missing
or malformed benchmark records, parser regression, missing release binary, or a
non-downward memory trend fails the job.
