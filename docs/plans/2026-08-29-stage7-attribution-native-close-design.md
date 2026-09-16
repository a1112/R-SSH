# Stage 7 Attribution Native-Close Design

## Context

The fixed Windows Gate 0 attribution matrix failed once at `auto/full-frame`
after a successful CPU first present. The launcher received no
`attribution_stage_ready` marker before its 20-second readiness deadline. The
same source and binaries completed 30 consecutive targeted `auto/full-frame`
runs after the failure, so the failure is intermittent rather than a stable
rendering or protocol error.

The runner is Windows on an NVIDIA adapter, and every observed automatic run
selects Vulkan. Production `WindowGpu` already preserves GPU resources until
process exit for the Windows + NVIDIA + Vulkan native-close combination because
normal teardown is unsafe on that driver stack. The private Stage 7 attribution
owner creates and drops equivalent windowed GPU resources without applying that
known workaround. A long matrix repeatedly exercises this unprotected teardown
path before the eventual readiness timeout.

## Decision

Apply the existing native-close eligibility policy to the private Stage 7
attribution owner. At the end of an attribution run, the owner will:

1. release the renderer before its context on ordinary backends;
2. retain the renderer and context until process exit on Windows + NVIDIA +
   Vulkan; and
3. perform this teardown before returning from the native event-loop callback,
   including controller and hold failures once GPU identity is available.

The change remains private to the diagnostic owner. It does not change backend
selection, readiness timing, memory sampling, the Stage 7 evidence contract, or
production rendering behavior.

## Rejected Alternatives

- Increasing the 20-second readiness timeout would only wait longer after a
  poisoned driver teardown and would not address the known unsafe close path.
- Retrying a failed matrix cell would weaken the fail-closed evidence model and
  could hide a product or driver lifecycle defect.
- Forcing DX12 for `auto` would alter production backend selection and make the
  attribution matrix stop measuring the existing automatic path.

## Error Handling

Teardown must run after the controller returns regardless of whether the
controller succeeded. The original controller or hold error remains the
reported result. Cleanup does not turn a failed attribution run into a success.
For unmatched adapters, resources are dropped deterministically in renderer-
before-context order.

## Testing

Test-first coverage will prove:

- the private Stage 7 native path invokes its native-close teardown before
  returning;
- only Windows + NVIDIA + Vulkan selects resource retention;
- unmatched combinations release renderer before context; and
- matched resources are retained rather than dropped.

Verification will include the focused unit/compatibility tests, formatting and
clippy, 30 process-cold targeted `auto/full-frame` runs, and then a new complete
Gate 0 evidence collection. The Gate remains blocked unless the validator emits
exactly `attribution-ready`.
