# Idle CPU Near-Zero Regression Gate Design

## Context

Release run `32443623019` measured an idle CPU median of `0.0034488498%`
against a same-machine baseline of `0.0006851674%`. The value is far below
the existing `3.0%` absolute limit, but the relative `110%` comparison turns a
sub-hundredth-percent measurement delta into a five-fold regression.

A second same-machine probe produced values from approximately `0.0024%` to
`0.0039%`, confirming that rerunning the unchanged gate would reproduce the
failure.

## Decision

Keep both layers of protection:

1. The existing `3.0%` absolute idle CPU limit remains unchanged.
2. The same-machine relative check fails only when idle CPU is more than 10%
   above the baseline and the absolute increase is greater than `0.01`
   percentage points.

The noise floor applies only to idle CPU. Throughput, latency, rendering, and
memory retain their existing relative comparisons.

## Alternatives Rejected

- Updating the baseline to the latest sample remains brittle near timer
  resolution and turns noise into policy.
- Removing the idle CPU relative check would lose useful same-machine
  regression detection below the absolute ceiling.

## Verification

The release workflow contract test must require the noise-floor constant, the
idle-only invocation, and self-check cases proving that a near-zero delta is
ignored while a material delta still fails. The full release-contract test,
formatting, Clippy, and the final fixed-runner Release must pass.
