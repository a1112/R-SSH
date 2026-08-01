# Performance Baseline and Gates

This document is the source of truth for the terminal benchmark budgets enforced
by CI and release workflows.

## Verification status (2026-08-02)

R-SSH's production native-window path presents through direct `wgpu`. The fixed
`bench --json` render sample documented here still uses the CPU/offscreen
`PixelRenderer`, however, so its render p95 is a regression proxy rather than
evidence for GPU present latency or input-to-present latency.

The following fixed-command observations were collected from commit
`83ade73a9d11e165dc66e82e8f6ca1b910c2946c` and are **verified locally on
Windows x64**:

| Workload | Elapsed | Throughput | Chunk p95 | `PixelRenderer` p95 | RSS | Additional deterministic evidence |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| `plain-scroll` | 282 ms | 3,705,635 B/s | 3,096 us | 527 us | 51,994,624 bytes | survivor clones `0`; history relocations `0` |
| `ansi-scroll-query` | 257 ms | 4,065,596 B/s | 2,996 us | 361 us | 52,727,808 bytes | inspected bytes `1,499,241`; survivor clones `0`; history relocations `0` |

Both invocations returned `ok: true` and an empty `threshold_violations` array
for the checks evaluated by those commands. That result is not a performance
certification: the local plain workload was below the approved 5 MiB/s design
budget, and the query workload's chunk p95 was above the 2 ms stabilization
target. The CPU/offscreen render values also do not measure the displayed frame.

Current evidence status is:

- **verified locally on Windows x64**: the two observations above and their
  deterministic clone, relocation, and scanner counters;
- **defined in hosted workflow but not run in this local session**: the
  pull-request `deterministic-performance` job in
  [`.github/workflows/ci.yml`](../.github/workflows/ci.yml);
- **requires protected/self-hosted environment**: the fixed-machine release gate
  in [`.github/workflows/release.yml`](../.github/workflows/release.yml), including
  its protected baseline variables and reviewer-controlled environment;
- **not yet evidenced**: a hosted or protected performance result for exact
  commit `83ade73a9d11e165dc66e82e8f6ca1b910c2946c`, plus GPU-present and
  input-to-present latency budgets.

The cross-requirement evidence ledger is maintained in
[`production-parity-verification.md`](production-parity-verification.md).

## Approved Budgets

| Metric | Budget |
| --- | ---: |
| Query scanner inspected bytes | at most `4 * input bytes` |
| Query throughput ratio, 16 KiB chunks / 512 B chunks | at least `0.70` |
| Surviving cells cloned by full-screen scrolling | `0` |
| Surviving history rows relocated by eviction | `0` |
| Metadata rebases for one batched prune | at most `1` |
| Query-heavy parser throughput | at least `1,048,576` bytes/s |
| Plain scrolling throughput | at least `5,242,880` bytes/s |
| 8 KiB parser chunk p95 | at most `5,000` us |
| Offscreen `PixelRenderer` frame p95 | at most `16,000` us |
| Idle CPU | at most `3.0` percent |
| Resident memory | at most `268,435,456` bytes |

The 16 ms render budget is currently an offscreen `PixelRenderer` proxy. It is
not a GPU present or input-to-present measurement. Direct `wgpu` presentation is
implemented in the production native-window path, but the corresponding
GPU-present and input-to-present performance gates are **not yet evidenced**.

`metadata_rebase_batches` is cumulative across a benchmark run, so it must not
be compared with `1` as a run-wide threshold. Hosted CI runs
`batched_scroll_prune_matches_incremental_prune`, which proves that one batched
prune performs one rebase, and verifies that benchmark JSON keeps the cumulative
field observable.

## Hosted Pull-Request Gate

The `deterministic-performance` job is **defined in hosted workflow but not run
in this local session**. When run on `windows-2025`, it enforces only
deterministic work and the relative 16 KiB/512 B throughput ratio. It does not
enforce absolute elapsed time, CPU, or memory budgets on shared hosted runners.
After one discarded warmup pair, it measures five 512 B/16 KiB pairs, alternates
which chunk size runs first, and gates the median of the five per-pair ratios.
The workflow emits the individual ratio samples and median as JSON.

Each benchmark process exit code is checked before its JSON is parsed. A failed
gate emits `threshold_violations` entries with `metric`, `observed`, and
`expected` fields. CLI JSON also retains the legacy `actual` and `limit` fields;
their values are identical to `observed` and `expected`. The public Rust
`BenchThresholdViolation` fields remain `metric`, `actual`, and `limit` during
this compatibility window. If the process sampler cannot report both resident
and virtual memory, requested idle-CPU or RSS thresholds fail with
`observed = "unavailable"` instead of treating zero as a measurement.

## Fixed Release Gate

The fixed release gate **requires protected/self-hosted environment** and was
not run for exact commit `83ade73a9d11e165dc66e82e8f6ca1b910c2946c` in
this local session. The release workflow is configured to use a protected
runner labeled
`self-hosted`, `Windows`, `X64`, and `rssh-performance`. Tag and manually
dispatched releases cannot package or publish until this job succeeds. The job
is serialized per machine class.

The protected `performance` environment must require designated reviewers.
Manual dispatch is accepted only from the repository default branch. Tag runs
are accepted only for `v*` tags; the repository ruleset must restrict creation
of those tags to authorized release maintainers and require the tag commit to
be reachable from the protected default branch. The fixed job has read-only
contents permission and checks out without persisted credentials. Only the
separate publish job receives `contents: write`.

For both `ansi-scroll-query` and `plain-scroll`, the runner executes the
workloads in interleaved query/plain rounds to reduce thermal drift:

- two discarded warmups;
- seven measured samples;
- the sorted sample at index `3` as the median.

The fixed command fingerprint is:

```text
v1|bytes=1048576|chunk=8192|frames=30|idle=1000|query=ansi-scroll-query|plain=plain-scroll
```

Absolute budgets are applied to the medians. Throughput may not fall below 90%
of the protected same-machine baseline. Latency, idle CPU, and RSS may not rise
above 110% of that baseline. Missing, non-positive, non-finite, or mismatched
baseline metadata fails closed. Regression comparisons divide observed values
by the positive finite baseline, avoiding overflow near `Double.MaxValue`;
workflow boundary checks cover zero, negative values, NaN, both infinities,
`Double.MaxValue`, and exact 90%/110% boundaries.

The protected `performance` environment supplies:

- `RSSH_PERF_BASELINE_MACHINE_CLASS` (exact `RUNNER_NAME`);
- `RSSH_PERF_BASELINE_OS` (exact `RUNNER_OS`);
- `RSSH_PERF_BASELINE_ARCH` (exact `RUNNER_ARCH`);
- `RSSH_PERF_BASELINE_CPU` (processor name, core count, and logical count);
- `RSSH_PERF_BASELINE_TOOLCHAIN` (exact `rustc --version`);
- `RSSH_PERF_BASELINE_COMMAND_FINGERPRINT`;
- `RSSH_PERF_BASELINE_QUERY_BPS`;
- `RSSH_PERF_BASELINE_PLAIN_BPS`;
- `RSSH_PERF_BASELINE_CHUNK_P95_US`;
- `RSSH_PERF_BASELINE_RENDER_P95_US`;
- `RSSH_PERF_BASELINE_IDLE_CPU_PERCENT`;
- `RSSH_PERF_BASELINE_RSS_BYTES`.

Upper-bound baseline values use the worse of the query and plain medians.
Baseline variables must be captured with the exact runner and command above.
The first baseline is established by an authorized runner operator after two
warmups and seven samples; until all protected values are installed, release
certification intentionally fails.

When an intentional optimization changes a median, retain the previous baseline
for the validating release. After that release passes, replace the protected
value with the accepted median from the workflow JSON. Never refresh a baseline
to make an unexplained regression pass.
