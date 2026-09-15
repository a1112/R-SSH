# Legacy GPU recovery checkpoint

Task 4 recovery substep, based on consumer
`26742a624eac0a299c28ac192a10d9259a980ab9`. Full frozen-profile compatibility,
diagnostic compilation isolation and release rehearsal integration remain
pending. This checkpoint is not merge, performance or physical-split approval.

## Lifetime contract

The frozen renderer has no public CPU-cache retirement method. Its original
app retained the whole lost renderer until shutdown. The legacy adapter now
replaces only the exposed catalog with an empty catalog before rebuilding on
the recovered context. It performs no GPU operation on the lost renderer.
Private shaping/raster/payload caches and GPU objects remain quarantined; they
are not reported as released. Modern full CPU font-state retirement is unchanged.

Independent review found an existing unsafe edge in the shared caller: CPU
fallback immediately removed `WindowGpu`, bypassing the close-time NVIDIA/Vulkan
workaround. Fallback now transfers that boxed owner into a separate quarantine.
Only the active owner receives present/resize work. Both existing window-close
hooks visit quarantined owners at the actual close boundary; fallback itself
does not manufacture a close intent or abandon GPU resources early. Actual
abandonment counters from quarantined owners are included in window metrics.

This keeps frozen private-cache retention explicit. No cache-zero metrics,
performance certificates, new frozen baseline, or relaxed budgets are introduced.
Repeated recovery can retain multiple old owners, as with frozen teardown; this
is functional compatibility, not a memory/performance certification claim.

## Tests and evidence at implementation time

- Two real frozen-library recovery tests first failed because the old catalog
  was still populated at replacement construction. They then passed: catalog
  retirement precedes allocation, device generation changes, rebuilt CJK pixels
  match exactly, and repeated replacement failures retain their error context.
- All eight frozen GPU boundary tests pass. The same boundary plus font tests
  passes 21 tests against modern libraries with the legacy selector.
- Two window-owner tests first failed because CPU fallback discarded all owners.
  They pass with quarantine, including repeated fallback, close idempotence,
  recovered-owner abandonment accounting and the native-close eligibility policy.
- Modern recovery tests: 13 pass; abandonment-filter tests: 4 pass;
  window-manager tests: 31 pass.
- Final-source modern `production-gui` native device-loss, resize and static
  ten-frame integration tests pass. Exact-commit frozen production builds and
  corresponding native tests remain to be run after committing this slice.
- Both app clippy selections (modern binary/tests and legacy-selector binary/
  GPU harness), the updated Task 23 inventory, format and diff checks pass.
  Independent review confirms the caller lifetime issue is resolved; a manager-
  level quarantine aggregation test was suggested as additional coverage.

The frozen rlibs used for the isolated boundary tests are identified by
`L:/rssh-evidence/dual-adapter-font-wiring-e55d50d1-20260909/gpu-boundary-build.jsonl`.
The app test harness is compiled directly against those unchanged frozen rlibs;
this does not replace a complete prepared-consumer build or native-window test.

Next: prepare this committed consumer with the verified common preparer, build
the two production feature profiles, run native recovery/resize/ten-frame tests,
and isolate genuinely unsupported legacy diagnostic implementations. Keep the
rehearsal preparer disconnected until both actual adapters build and test.
