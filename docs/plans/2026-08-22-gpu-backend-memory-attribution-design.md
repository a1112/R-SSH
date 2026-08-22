# GPU Backend Memory Attribution Design

**Date:** 2026-08-22

**Status:** Approved

**Baseline:** `fde4649d1d1ee09af8b09326e5564945abcd5c10`

## Problem

Stage 7 physical repository extraction is blocked by the protected Windows x64
empty-window memory contract. Release evidence and a fresh local reproduction
show that CPU-first startup is healthy, but the process settles at roughly
311 MiB of Private Working Set after deferred GPU activation. The same run
reports Vulkan on an NVIDIA GeForce RTX 5060 Ti. The 45 MiB empty-window target
is therefore not met, while first-present latency and the SSH1 CPU path remain
within their approved budgets.

The current renderer creates a Windows WGPU instance with DX12, Vulkan, and GL
enabled. WGPU selects Vulkan on the observed runner. The standard
`WGPU_BACKEND=dx12` environment variable does not affect this explicit backend
set, so the repository cannot currently produce a controlled backend A/B result
without changing production defaults.

## Chosen Approach

Add an explicit backend selector only to the private diagnostics path:

```text
rssh-bench-launcher --renderer auto --gpu-backend dx12 ...
    -> rssh-app diagnostic-gui --renderer auto --gpu-backend dx12 ...
    -> NativeWindowApp diagnostic override
    -> WindowGpu preparation
    -> GpuContextOptions restricted to DX12
```

The selector accepts `dx12`, `vulkan`, and `gl`. The launcher also accepts
`--renderer auto|cpu|gpu` so the same harness can produce a CPU-only control.
`--gpu-backend` is rejected with `--renderer cpu`, because that combination
would claim to test a GPU backend while never initializing one.

The ordinary `window`, `start`, and `ssh --gui` paths do not accept or inherit
this override. Their default backend set and fallback behavior remain unchanged
until the A/B evidence identifies a safe production change.

## Evidence Contract

Diagnostic JSON records the requested renderer and backend without changing
legacy output when both remain at their defaults. A successful `gpu_ready`
marker additionally carries the actual backend and adapter identity from
`GpuPresentationMetrics`. The launcher preserves those values in the renderer
summary, allowing each stored artifact to prove both what was requested and
what WGPU actually selected.

The Stage 0 PowerShell runner gains a diagnostic matrix mode that writes
separate CPU, DX12, Vulkan, and GL artifacts. Unsupported backend initialization
is recorded as a failed probe rather than silently falling back to another
backend. The release decision compares Windows Private Working Set only between
successful probes on the same fixed runner.

## Alternatives Rejected

1. **Change the Windows default to DX12 immediately.** This could reduce memory,
   but there is no controlled DX12 measurement yet and it would change every
   user window before compatibility, recovery, and presentation are verified.
2. **Honor `WGPU_BACKEND` globally.** A process-global environment variable is
   difficult to validate, can unexpectedly affect production launches, and does
   not provide self-describing evidence.
3. **Implement the full `webgpu_preferred_adapter` configuration now.** That is a
   useful product feature, but it expands this blocker investigation into public
   configuration, adapter matching, reload, and multi-window semantics.

## Failure and Safety Behavior

- Invalid renderer/backend names fail during CLI parsing.
- CPU plus a backend selector fails before creating a window.
- A requested backend that cannot create a compatible surface or device reports
  a normal diagnostic failure; it must not retry another backend.
- Auto mode retains CPU presentation if the selected GPU backend fails.
- No passwords, keys, paths, or environment values are added to markers.
- The selector never affects production commands.

## Verification

Implementation follows strict TDD:

- app and launcher CLI parsing, defaults, invalid combinations, and command
  forwarding;
- WGPU option restriction without changing default backend masks;
- marker collection and backwards-compatible schema serialization;
- CPU control plus DX12/Vulkan/GL release probes on Windows;
- same-window CPU-to-GPU and forced-failure CPU fallback E2E;
- Stage 6 release/consumer/vendor contracts, workspace tests, format, and Clippy.

## Exit Decision

If a hardware backend meets the empty-window budget and passes presentation and
recovery tests, a separate design will propose the production selection change.
If no backend meets the budget, Stage 7 remains NO-GO and optimization continues
inside the monorepo with the backend/adapter allocation evidence attached.
