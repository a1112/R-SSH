# Legacy font repository wiring checkpoint

Continues the foundation commit `7c36b8cadc0a2dbef3d4f2a3a87bda52389af7d5`.
This is the font repository and runtime diagnostic boundary portion of Task 3.
It is not full frozen-app compatibility, complete Task 3 acceptance, or split GO.

## Changes

- Legacy activation builds the complete ordered candidate through the app-owned
  adapter and commits repository state only after successful catalog replacement.
  Recovery reconstructs the logical epoch without reading unavailable metrics.
- The modern batch-load path is unchanged. Legacy has no `catalog_builds` field;
  logical generations are not substituted for measured build/allocation counts.
- Repository diagnostics return a `Result`. Modern diagnostics retain their
  fields; legacy returns an `Unsupported` error. Diagnostic font modes cannot
  populate or mutate a legacy repository. Window diagnostic callers propagate
  errors instead of constructing a fabricated successful summary.
- Legacy command dispatch rejects bench, doctor, diagnostic-gui and self-test
  before those services start, including when diagnostic-tools was requested.
- Normal modern tests remain enabled in the modern profile. The font-only
  `legacy_font_boundary` harness compiles the real app repository and adapter
  against the frozen public font/render-core APIs without pulling in window GPU.

## Verification

TDD RED: three repository tests failed because expansion retained the modern
catalog incarnation and diagnostics were still successful. The command test
then failed because doctor returned success. Both changed paths passed after
implementation. Additional tests cover consecutive batches, recovery, source
failure, late fallback, width-variant routing, unsupported modes and mismatched
repository/catalog generations.

- Modern platform font tests: 27 passed.
- Modern diagnostic font tests: 13 passed.
- Package, Stage 5 and Task 23 contracts: 10 + 7 + 1 passed.
- Modern libraries with the legacy selector: 13 font-boundary tests passed;
  one dispatcher test covered all four rejected command variants.
- Actual frozen font/render-core libraries: the same 13 font-boundary tests
  passed via standalone `rustc --test`, with legacy and diagnostic-tools cfgs.
  Frozen Cargo artifacts are recorded in
  `L:/rssh-evidence/dual-adapter-frozen-20260909/font-boundary-build.jsonl`.
- App clippy passed for the normal binary/tests and for the legacy-selector
  binary/font-only harness; format and diff checks passed. The first legacy
  clippy run identified an intentionally unused receiver on the uniform
  diagnostic boundary; its narrowly scoped lint annotation explains this.
- Independent review found no Critical/Important issue in this bounded slice.
  Its additional generation-mismatch test recommendation was implemented.

## Remaining boundary

The frozen source remains `0e8ebd5de22758275cbb6a849c19c032268d7fac`.
Existing receipts have not been relabeled as newer-consumer evidence. Prepare
a new exact-commit checkout for the next complete app compiler check.

The CLI runtime rejection is **not** proof that frozen diagnostic builds compile.
`window_gpu.rs` still uses unavailable production and diagnostic GPU/font APIs,
and its diagnostic tests still require modern metrics. Task 4 must isolate those
implementations, test real GPU frames/recovery, and finish complete legacy
profile validation. In particular, legacy-selector full test suites are not
claimed green: the modern GPU diagnostic tests are not legacy capability tests.
Rehearsal integration remains unchanged until both adapters work. All frozen
LKG references, source boundaries and performance budgets are unchanged.
