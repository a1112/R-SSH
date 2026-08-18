# Project Split Stage 3: Renderer Structure

**Date:** 2026-08-18

**Status:** Approved for implementation

**Baseline:** `f4d210274e05ff549093086f3a1ed859396a78b4`

## Goal

Split the current `rssh-renderer` implementation into explicit R-Term renderer
layers without changing snapshots, pixels, presentation, recovery, or the
validated CPU-first/GPU-later handoff.

## Package Boundary

- `rterm-render-core` owns terminal render snapshots, geometry, damage-facing
  value contracts, shared paint/layer plans, and stable content digests. It may
  depend on R-Term terminal/types/fonts, but never WGPU, window handles, image
  decoders, Winit, Tauri, PTY, SSH, application, or diagnostics crates.
- `rterm-render-cpu` owns `PixelRenderer`, CPU text/raster composition, image
  decoding, and the software frame path. It depends inward on
  `rterm-render-core` and contains no WGPU, glyphon, or raw-window-handle
  dependency.
- `rterm-render-wgpu` owns WGPU context/surface lifecycle, GPU text, render
  graph execution, texture caches, device recovery, and presentation. Shared
  snapshot and geometry inputs come from `rterm-render-core`.
- `rssh-renderer` remains for one stage as an explicit compatibility facade. It
  contains no implementation and re-exports the exact legacy surface from the
  three new packages.
- `rssh-app` is the production composition root and depends directly on both
  `rterm-render-cpu` and `rterm-render-wgpu`; it must not reach the GPU path
  through the compatibility facade.

The physical implementation directories move with `git mv` where history is
not provenance-frozen. Public type identity must come from the owning new crate,
not duplicate compatibility types.

## TDD Sequence

1. Add failing architecture and Cargo metadata contracts for the four package
   roles, forbidden dependencies, direct GUI composition, compatibility-only
   facade, and observable CPU/WGPU production linkage.
2. Create `rterm-render-core` and move snapshot, geometry, shared paint/layer
   values, terminal projection, and stable digests. Migrate CPU/GPU consumers to
   the single core type identity.
3. Move software raster/text/image decoding into `rterm-render-cpu`. Prove its
   dependency tree has no `wgpu`, `glyphon`, or `raw-window-handle`.
4. Move the GPU module into `rterm-render-wgpu`. Keep WGPU context preparation,
   async device finish, rendering, recovery, and texture ownership together.
   Replace any method that made the CPU renderer own GPU state with an explicit
   WGPU-side planner/composition API.
5. Rebuild `rssh-renderer` as an explicit re-export facade and migrate
   `rssh-app`/`rssh-native` production imports to the owning packages. Move tests
   to their owning crates while preserving their assertions and fixture data.
6. Update CI, architecture policy, README, behavior catalog contracts, and
   package/release API checks. Do not raise architecture or performance budgets.

## Frozen Behavior

- terminal snapshot content digests and first-row pixel digests;
- CPU shaped-text and bitmap output;
- GPU layer order, text/image equivalence, and headless readback;
- surface/device loss recovery and bounded caches;
- same-window CPU-to-GPU handoff and forced GPU failure CPU fallback;
- native ten-frame and functional observer evidence;
- public error kinds and rollback behavior.

## Verification

- focused core/CPU/WGPU/facade tests and package metadata contracts;
- `cargo tree -p rterm-render-core --all-features` contains no WGPU, window,
  decoder, PTY, SSH, app, or diagnostics dependency;
- `cargo tree -p rterm-render-cpu --all-features` contains no WGPU, glyphon, or
  raw-window-handle dependency;
- CPU/WGPU equivalence, GPU device-loss, same-window handoff/fallback, native
  window E2E, release API, and visual digest tests;
- architecture policy and Task 10 provenance checks;
- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --all-targets --locked`;
- PR CI and post-merge `main` CI.

## Exit Criteria

The three renderer packages have enforceable ownership and dependency
direction, the application visibly composes CPU and WGPU backends, the legacy
facade is implementation-free, all frozen visual/functional evidence is
unchanged, and no budget is increased.
