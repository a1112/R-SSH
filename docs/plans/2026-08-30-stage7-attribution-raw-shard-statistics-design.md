# Stage 7 Attribution Raw-Shard Statistics Design

## Context

The repaired Gate 0 run completed the font proof, all 960 attribution matrix
processes, the deterministic attribution suite, and the external-source proof.
Evidence assembly then rejected every supported attribution raw shard whose
local process statistics differed from the complete 30-process cohort.

The attribution runner emits one raw artifact per process. Each artifact
contains one process, its ten residence samples, and its nearest-rank p50
representative. It currently also writes `statistics` computed from that one
process. During validation, all shards for a backend/stage cell are combined,
and any shard-level `statistics` claim is required to equal the independently
recomputed complete-cohort p50, p95, and maximum. The existing test helper that
splits a complete metric payload into per-process shards deliberately removes
`statistics`, documenting the intended shard representation.

## Decision

Remove `statistics` from supported per-process attribution raw groups. Retain:

- the ten raw samples;
- the per-process representative;
- stage/backend identity and project-owned resource summary; and
- the complete 30-process statistics in the attribution aggregate artifact.

The validator remains unchanged. It will continue combining all 30 distinct
process shards, recomputing the cohort statistics, enforcing thresholds, and
matching the independently produced aggregate.

## Data Flow

For each supported backend/stage cell, the runner collects 30 processes. It
writes 30 atomic raw artifacts without local statistics, then writes one
aggregate containing the ordered representatives, raw maxima, and recomputed
group statistics. Assembly loads every raw shard, reconstructs the complete
cohort, and compares that result with the aggregate.

Unsupported suffix artifacts remain unchanged because they contain no metric
processes or statistics.

## Error Handling

The change does not suppress or reinterpret collection errors. Incomplete
cohorts, duplicate process IDs, protocol drift, unsupported `auto`, threshold
failures, and aggregate mismatches remain fail-closed. No retry or evidence
mutation is introduced.

## Rejected Alternatives

- Copying complete-cohort statistics into every process shard would duplicate
  the same derived claim 30 times and create avoidable drift risk.
- Accepting per-shard local statistics in the validator would weaken the rule
  that all reported reductions are independently reproducible from the full
  raw cohort.
- Editing the already collected evidence would break immutable producer/source
  provenance and cannot certify a new source commit.

## Testing and Evidence

A failing deterministic contract test will first prove that the production
runner still writes local `statistics` into per-process shards. The minimal
implementation removes that field while retaining aggregate `group_statistics`.
The existing attribution matrix validator suite, deterministic proof runner,
formatting, and whitespace checks must pass.

Because the producer source SHA changes, the current failed evidence will be
archived recoverably. A fresh full Gate 0 run must regenerate all artifacts from
one clean commit. Success is recognized only when the validator emits exactly
`attribution-ready`.
