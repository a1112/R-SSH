# Project Split Stages 1–6 Design

**Date:** 2026-08-18

**Status:** Approved

**Baseline:** Stage 0 merged as `e5168bc0048af3c4fd829c3ceb01683b44c20d84`

## Objective

Complete the logical R-Term/R-SSH separation inside the existing monorepo before
any physical repository extraction. Stages 1–6 establish dependency direction,
runtime transport ownership, renderer boundaries, memory-efficient snapshots,
lazy startup composition, and cross-repository release contracts. Physical
repository extraction remains Stage 7 and is intentionally excluded.

## Chosen Approach

Each stage lands as an independently reviewed PR based on the latest protected
`main`. Stage 1 is a hard prerequisite. Stages 2 and 3 may proceed independently
after Stage 1, but Stage 4 waits for the renderer boundary, Stage 5 waits for the
snapshot/cache work, and Stage 6 waits for the production dependency graph and
performance contracts to stabilize.

This approach was selected over:

1. **One monolithic Stage 1–6 branch.** Rejected because it would make dependency
   regressions, performance changes, and rollback boundaries impossible to
   isolate.
2. **Skipping Stage 1 and moving runtime/renderer code immediately.** Rejected
   because shared identifiers and terminal geometry would keep the old
   dependency direction alive under new paths.
3. **Physical repository extraction first.** Rejected because the current vendor
   patches, compatibility imports, and performance work still require atomic
   changes inside one workspace.

## Repository and Branch Policy

- Never implement directly in the user's dirty root checkout.
- Create a clean `codex/` branch and worktree from current `origin/main` for each
  stage.
- Keep `main` protected; every stage goes through PR CI and review.
- Preserve compatibility for one stage through explicit re-export facades rather
  than broad wildcard shims.
- Do not raise architecture, memory, test, or package budgets to make a move pass.

## Target Dependency Direction

```text
R-Term foundations
  rterm-types
      ↓
  rterm-terminal / rterm-fonts
      ↓
  rterm-runtime
      ↓
  rterm-render-core
      ↓
  rterm-render-cpu / rterm-render-wgpu

R-SSH product
  rssh-domain
      ↓
  rssh-pty / rssh-ssh (runtime adapters)
      ↓
  rssh-native / rssh-config
      ↓
  rssh-app (composition root)

Diagnostics
  rssh-diagnostics → benchmark/test-facing public contracts only
```

R-Term crates may not depend on SSH, PTY, application, diagnostics, Winit, or
Tauri crates. Protocol/platform adapters depend inward on `rterm-runtime`; the
runtime never depends outward on concrete transports.

## Stage 1: Types and Domain

Create `rterm-types` for terminal-sized, renderer-neutral value types and
`rssh-domain` for application identity and shell-domain models.

- Move `TerminalSize`, `DamageRegion`, and `SessionId` into `rterm-types`.
- Move `WindowId`, `WorkspaceId`, `TabId`, `PaneId`, pane launch/domain, and app
  shell state into `rssh-domain`.
- Rename `rssh-terminal` to `rterm-terminal` and `rssh-fonts` to `rterm-fonts`
  using `git mv` so history remains extractable.
- Keep `rssh-core` as a narrow compatibility facade that re-exports the exact
  public API currently used by downstream crates.
- Add architecture-policy rules that reject any R-Term dependency on R-SSH.

Exit criteria: zero intended behavior change, all workspace targets compile and
test, package metadata uses the new crate names, and architecture checks prove
the dependency direction.

## Stage 2: Runtime and Transport

Rename `rssh-runtime` to `rterm-runtime`. Move concrete SSH and local PTY runtime
adapters out of the runtime crate:

- `transport/ssh.rs` becomes `rssh-ssh/src/runtime_adapter.rs`.
- `transport/local.rs` becomes `rssh-pty/src/runtime_adapter.rs`.
- Remove runtime features `local-transport`, `ssh-transport`, and
  `transport-adapters`.
- Concrete transport crates may expose a narrowly scoped `runtime-adapter`
  feature.
- Preserve the existing `SessionTransport` reader/writer/control/interrupt
  ownership model; do not rewrite it as an async trait.

Exit criteria: `cargo tree` for `rterm-runtime` contains neither Russh nor PTY
dependencies, while fake transport, burst, worker, real PTY, loopback SSH, and
shutdown tests remain green.

## Stage 3: Renderer Structure

Split the current renderer without changing visual semantics:

- `rterm-render-core`: snapshots, geometry, damage, layer plans, shared value
  contracts.
- `rterm-render-cpu`: PixelRenderer and software-only raster composition.
- `rterm-render-wgpu`: WGPU surface/context, GPU text, render graph, textures,
  recovery, and presentation.

The GUI explicitly composes the bootstrap CPU renderer and the WGPU renderer.
Renderer core must not depend on WGPU, window handles, or image decoding
backends. Production feature graphs must make CPU and WGPU linkage observable.

Exit criteria: golden snapshots, CPU/WGPU equivalence, device loss, same-window
handoff, native window E2E, and visual digests remain unchanged.

## Stage 4: Snapshot and Cache Memory

Replace per-cell owned payloads with bounded shared identities:

- Intern graphemes, styles, and hyperlinks.
- Replace image payload ownership in snapshots with image handles.
- Introduce immutable row-level snapshots and damage-aware reuse.
- Apply explicit byte limits to shape, raster, texture, image, and snapshot
  caches.
- Keep one compatibility snapshot builder while consumers migrate.

Exit criteria: 80×24 and 200×60 snapshot benchmarks, full/damage equivalence,
CJK/emoji/image tests, and Stage 0 empty/SSH1 measurements show a downward memory
trend without parser throughput falling below 98% of baseline.

## Stage 5: Startup, Lazy Resources, and Composition

Make `rssh-app` the minimal GUI composition root:

- Separate GUI, CLI, and diagnostics entrypoint dependencies.
- Share one bounded lazy Tokio runtime for native SSH work.
- Keep SSH runtime, SFTP, image decoders, fallback font catalogs, and other
  optional resources out of the pre-first-present path.
- Preserve the validated CPU-first/GPU-later handoff and renderer rollback
  controls.
- Add production features for native GUI, SSH, local PTY, images, and diagnostic
  tools, with minimal defaults for packaged GUI binaries.

Exit criteria: Windows first-present p95 remains ≤500 ms; empty-window and SSH1
memory are candidates for the <45 MiB and <60 MiB blocking gates; host-key,
authentication, reconnect, and cancellation UX remain unchanged.

## Stage 6: Cross-Repository Release Contract

Prepare the logical R-Term surface for later extraction without creating the
second repository yet:

- Define versioned public APIs and compatibility policy for R-Term crates.
- Add an R-SSH consumer build that uses only the declared R-Term API.
- Add fixed-runner performance comparison, package smoke, clean-clone, and
  last-known-good rollback rehearsal.
- Resolve or explicitly pin the `glyphon` and `gpu-allocator` patch strategy.
- Freeze the file-history extraction map containing both old and new crate paths.

Exit criteria: a candidate R-Term revision can be consumed by R-SSH in CI, a
last-known-good revision can be restored in one change, all release/package jobs
pass, and no unresolved vendor patch blocks Stage 7.

## Data and Control Flow

The terminal parser produces R-Term-owned state and damage. `rterm-runtime`
publishes transport-neutral batches. R-SSH transport adapters translate PTY or
SSH resources into that runtime contract. The application shell owns product
identity and routes batches to renderer-core snapshots. CPU and WGPU renderers
consume the same immutable snapshot contract. Diagnostics observe public markers
and process metrics but do not participate in the production GUI dependency
graph.

## Error and Rollback Policy

- Moves must preserve typed errors; no stage may replace errors with strings only
  to cross a crate boundary.
- Runtime cancellation, close, interrupt, and reader-drain ordering are frozen by
  existing tests before adapter movement.
- Each stage remains revertible as one PR.
- Compatibility facades are removed only after all consumers migrate and CI has
  proven the new path.
- A performance regression pauses the next stage; it is not hidden by changing
  the Stage 0 schema or thresholds.

## Verification Strategy

Every behavioral or architectural change follows RED/GREEN TDD. Required gates
include:

- focused crate tests for each moved API;
- compile-fail or metadata contracts for forbidden dependency direction;
- `cargo fmt --all -- --check`;
- strict workspace Clippy;
- `cargo test --workspace --all-targets --locked`;
- native PTY/SSH/window functional tests;
- package and feature-isolation tests;
- Stage 0 fixed-runner startup and memory reports at each stage boundary.

Stage 7 is not authorized by this design. It begins only after Stage 6 exit
criteria and a separate physical-extraction approval.
