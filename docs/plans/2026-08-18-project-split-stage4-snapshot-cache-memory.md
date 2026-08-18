# Project Split Stage 4 Snapshot and Cache Memory Implementation Plan

**Date:** 2026-08-18

**Status:** Approved for implementation

**Base:** `origin/main` at `2f826b738164952682a01cb3a765766f70481ef0`

## Goal

Make renderer snapshots immutable, incrementally reusable, and explicitly
bounded without changing CPU/WGPU visual output or the renderer facade. Stage 4
must reduce retained duplicate payloads while preserving parser throughput and
the Stage 0 diagnostic schema.

## Current facts

- `rterm-terminal` already stores graphemes as `SmolStr` and hyperlinks as
  `Arc<str>`.
- `rterm-render-core` currently expands every renderable cell into an owned
  `String`, repeats the complete style record per cell, and copies every inline
  image into a fresh `Vec<u8>`.
- `TerminalRenderSnapshot::update_from_terminal_damage` mutates one flat vector
  with retain/append/sort and reconstructs all image projections.
- Shape, raster, glyph-atlas, GPU instance, GPU texture, and readback paths have
  individual budgets, but there is no common snapshot/cache accounting contract
  and no row identity that CPU and WGPU consumers can reuse.

## Target design

### Shared identities

- Add renderer-core owned `RenderGrapheme`, `RenderStyle`, and
  `RenderImagePayload` identities.
- Graphemes and hyperlinks use shared immutable strings. Styles are immutable
  shared values keyed by all visual attributes. Image payloads are immutable
  shared bytes with stable content identity.
- `RenderCell` remains the compatibility cell value, but delegates visual style
  field access through its shared style identity. Existing consumers migrate to
  constructors/accessors in this stage; no renderer-specific type leaks into
  core.

### Immutable rows

- `RenderRowSnapshot` owns an immutable ordered cell slice and stable content
  identity.
- `TerminalRenderSnapshot` owns shared row snapshots instead of one mutable flat
  cell vector. Its compatibility cell view is iterator-based; a dedicated
  compatibility builder remains for callers that construct ad-hoc snapshots.
- `TerminalSnapshotBuilder` retains the previous snapshot and interns payloads.
  A damage update rebuilds only intersecting rows and reuses unchanged row
  `Arc`s. Cursor-only changes reuse every row.
- Overlay and viewport operations produce new rows only where their coordinates
  change. They never mutate rows shared with another snapshot.

### Budgets and metrics

- Introduce a common `ByteBudget`/metrics vocabulary in renderer core for
  snapshot pools and retained image payloads.
- Snapshot interning and retained-row caches have explicit byte ceilings,
  deterministic admission, and observable hits/misses/evictions/bypasses.
- Existing shape, raster, glyph-atlas, texture, instance, and readback budgets
  expose their configured and retained bytes through one aggregate renderer
  cache report. This stage does not raise any existing default budget.
- Active-frame bytes are reported separately from retained-cache bytes; an
  active frame is never silently truncated to satisfy a cache limit.

### Compatibility

- Keep `TerminalRenderSnapshot::from_grid`, `from_terminal`, and
  `from_terminal_viewport` as compatibility builders backed by the new snapshot
  builder.
- Keep CPU and WGPU render output byte-for-byte equivalent. Compatibility APIs
  may allocate a temporary flat vector only when a legacy caller explicitly
  requests one; production render paths consume rows directly.

## TDD batches

### Batch 1: contracts and shared payloads

1. Add failing core tests proving repeated graphemes/styles/hyperlinks/images
   share allocations and image data is no longer owned as `Vec<u8>`.
2. Add failing architecture tests proving snapshot value types remain in
   `rterm-render-core` and do not introduce WGPU/window/image-decoder dependencies.
3. Implement shared identities and migrate direct struct construction to narrow
   constructors.

### Batch 2: immutable rows and damage reuse

1. Add failing tests for 80x24 and 200x60 row counts, unchanged-row pointer
   identity, cursor-only reuse, changed-row replacement, and clone-on-write
   overlays.
2. Implement row snapshots, the stateful builder, compatibility iteration, and
   damage-aware image projection reuse.
3. Migrate CPU/WGPU/app production consumers to row iteration.
4. Prove full rebuild and damage update snapshots have identical content and
   CPU/WGPU digests for ASCII, CJK, emoji, hyperlinks, and inline images.

### Batch 3: bounded caches and accounting

1. Add failing tests for exact byte accounting, zero/tiny budgets, deterministic
   eviction, oversize bypass, no over-budget retained state, and aggregate cache
   reports.
2. Implement bounded intern/row/image caches and surface existing font/GPU cache
   metrics without changing their limits.
3. Add security/robustness tests for overflow, adversarial unique graphemes,
   oversized images, resize churn, and device-loss invalidation.

### Batch 4: benchmarks and release evidence

1. Add deterministic snapshot benchmark modes for 80x24 and 200x60 full and
   one-row damage builds, recording active bytes, retained bytes, reuse ratio,
   and elapsed time.
2. Add CJK/emoji/image benchmark fixtures and full/damage equivalence gates.
3. Run the existing parser benchmark and require at least 98% of the checked-in
   Stage 0 baseline.
4. Run Stage 0 empty-window and SSH1 diagnostics on the fixed Windows runner and
   publish before/after values. A missing result fails the job; memory targets
   remain unchanged and are not weakened.

## Verification

Focused gates are followed by:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Also run CPU/WGPU golden equivalence, native ten-frame E2E, same-window
CPU-to-GPU handoff, parser provenance/throughput, package smoke, architecture
scan, and Stage 0 diagnostics. Stage 5 does not start until Stage 4 is merged and
post-merge `main` CI is green.

## Commit and rollback boundaries

1. `test: define Stage 4 snapshot memory contracts`
2. `refactor: share immutable renderer snapshot payloads`
3. `feat: reuse bounded row snapshots across damage`
4. `perf: gate Stage 4 snapshot and cache memory`

Each production commit must leave the compatibility facade green and can be
reverted independently. No budget, timeout, or performance threshold may be
raised to make the stage pass.
