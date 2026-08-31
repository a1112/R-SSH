# Stage 7 CI Gate Design

## Goal

Run the complete Stage 7 Gate 0 proof on the protected Windows performance
runner instead of a developer workstation that also hosts unrelated builds and
services. The authoritative terminal result remains exactly
`attribution-ready` for one immutable candidate commit.

## Context

The local Gate 0 run at `7ba25608` was contaminated by unrelated R-Switch,
R-File, TireDetection3D, and other workloads. Windows recorded a low virtual
memory event during the failed attribution cohort. A second local attempt was
stopped as soon as new foreign Cargo and CMake workloads appeared.

The repository already has a `stage7-attribution-matrix` job in
`.github/workflows/release.yml`. It targets the protected
`[self-hosted, Windows, X64, rssh-performance]` runner, but it currently has
three gaps:

- manual dispatch is restricted to the default branch;
- the current Stage 7 branch is not yet available to Actions;
- the job collects only the font and attribution fragments, not the complete
  four-fragment Gate 0 decision.

## Considered Approaches

### 1. Extend the existing release workflow (selected)

Add one explicit boolean manual-dispatch input for a Stage 7-only run. Permit
the protected Stage 7 job to run for the selected ref only when that input is
true. Skip the unrelated legacy fixed-performance and package jobs during a
Stage 7-only dispatch. Preserve their existing default-branch and tag release
behavior.

This keeps runner labels, environment protection, concurrency, toolchain, and
artifact handling in one established workflow.

### 2. Add a second Stage 7 workflow

A dedicated workflow would be shorter, but it would duplicate protected runner
configuration, checkout hardening, toolchain pinning, concurrency policy, and
artifact upload behavior. The copies could drift.

### 3. Continue retrying locally

This requires stopping unrelated projects for several hours and cannot enforce
host isolation. It is unsuitable for authoritative evidence.

## Workflow Contract

`workflow_dispatch` gains a `stage7_gate_only` Boolean input with a default of
`false`. A non-default ref can reach the protected Stage 7 proof job only when
this input is explicitly true. The legacy fixed-performance job is skipped for
Stage 7-only dispatches; ordinary default-branch and tag releases still require
it, so none of its thresholds are bypassed or weakened.

The Stage 7 job keeps:

- the `performance` protected environment;
- the `rssh-performance` self-hosted Windows x64 labels;
- read-only repository permissions and credential-free checkout;
- the fixed machine-class concurrency group;
- release/locked builds and runner-temporary target directories.

The timeout increases to six hours because the deterministic suite follows the
font and 900-process attribution collections in the same immutable job.

## Evidence Flow

The job runs these steps in order:

1. collect the font cohort and runner fingerprint;
2. collect the cumulative attribution matrix;
3. run deterministic attribution tests;
4. prove immutable external R-Term Git consumption;
5. assemble the four fragments into `stage7-evidence-manifest.json`;
6. validate the manifest for `attribution-ready`.

The artifact upload uses `if: always()` and uploads the complete Gate root, so
failed runs retain their partial raw evidence. Upload success does not override
the result of any proof or validator step.

## Failure and Security Behavior

Every proof and the final validator remains fail-closed. No retry replaces a
failed process cohort. The manual input expands only which Git ref may be
measured; it does not add write permissions, expose secrets, weaken environment
protection, or allow hosted runners to certify hardware evidence.

The branch is pushed only after deterministic workflow tests, formatting, and
diff checks pass. CI is dispatched with the exact branch ref and
`stage7_gate_only=true`. The run is not considered successful unless the Stage
7 validator emits `attribution-ready` and the complete evidence artifact is
available.

## Testing

Static contract tests parse the release workflow text and require:

- the typed opt-in input with a false default;
- guarded non-default-ref access to Stage 7 while the release prerequisite is
  skipped only for explicit Stage 7-only dispatches;
- the fixed runner, protected environment, concurrency, and extended timeout;
- all four proof commands, manifest assembly, final validation, and complete
  artifact upload;
- unrelated package jobs to be skipped for Stage 7-only dispatches.

Existing Stage 7 Python tests, YAML parsing, whitespace checks, and a clean
worktree check provide the local verification boundary before push.
