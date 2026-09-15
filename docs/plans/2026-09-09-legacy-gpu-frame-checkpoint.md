# Legacy GPU full-frame checkpoint

Task 4 frame preparation substep, based on consumer
`e55d50d1758cd0380e6d5d236c7c0a523b0b5a07`. Device-loss retirement and full
frozen-profile compatibility remain unfinished. Do not enable the rollback
preparer in release rehearsal or infer physical-split GO from this checkpoint.

## Implementation

The app-owned legacy frame path uses only frozen public methods. It rebuilds
the text backend conservatively for every frame because the old GPU scope only
checks catalog generation, not incarnation. Every attempt prepares all visible
rows, with at most one late-font expansion/restart. Remaining candidates request
the existing bounded follow-up frame. Modern incremental preparation is unchanged.

On preparation or late-font failure the adapter resets text state before
returning the error. If reset also fails, it preserves both errors, returns no
frame, and requires the caller to quarantine the renderer. `WindowGpu::present_once`
propagates preparation errors before `render_graph`. This does not establish safe
retirement of a lost device; that requires the remaining recovery work.

Tests use the frozen constructor's explicit `Rgba8Unorm` format. The matching
wgpu version is a dev-only dependency; the lockfile adds only that app dependency
edge. No frozen package is edited and no normal production feature is added.

## Evidence and limits

- The initial two executable tests failed on the not-implemented adapter.
- A real frozen GPU negative test then exposed one retained atlas entry after
  invalid late-font activation. After cleanup was implemented, atlas entries
  were zero and actual pixel readback matched an empty graph, on repeated failure.
- Six real GPU tests pass on frozen libraries: nonempty full frames, same-frame
  CJK fallback, partial-frame cleanup, same-epoch replacement plus resize/DPI,
  bounded static fallback convergence, and failure of both prepare and cleanup.
- The same legacy-selector integration harness passes 19 tests on modern
  libraries (13 font tests plus 6 GPU tests).
- Four existing modern `gpu_text_frame` regression tests pass. App clippy passes
  for normal binary/tests and legacy-selector binary/GPU harness. Format and
  diff checks pass. Independent review found no Critical/Important issue in this
  bounded slice; its optional combined-cleanup-error test was added.

Frozen artifact provenance is recorded at
`L:/rssh-evidence/dual-adapter-font-wiring-e55d50d1-20260909/gpu-boundary-build.jsonl`.
The artifact build used the verified frozen checkout and
`cargo build --locked -p rterm-render-wgpu -p rssh-core --all-targets --message-format=json`.
The app boundary harness was separately compiled with those exact rlibs via
`rustc --test`; this is not a complete frozen app build. An earlier attempt to
select the registry package pollster directly alongside workspace packages
triggered a Cargo resolver panic; building the workspace targets and their
dependencies completed successfully without changing package sources.

Per-frame rebuilding is intentionally conservative and has not been certified
against performance budgets. Headless GPU readback is not a native-window
present, device-loss recovery, shutdown-workaround or fixed-runner benchmark.
All original immutable LKG refs and absolute/relative budgets remain in force.
Next: implement safe legacy lost-device retirement, isolate unsupported
diagnostic implementations, then build and test both complete profiles from
new exact-commit preparations before changing rehearsal integration.
