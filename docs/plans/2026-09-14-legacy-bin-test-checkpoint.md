# Frozen bin-test capability checkpoint

Continuation of approved dual-adapter Tasks 3–4. No runtime or frozen package
changes, no release rehearsal integration, no physical extraction.

## Reproduction and classification

The exact `ef2560d0` frozen consumer freshly reproduced 30 app bin-test compiler
errors. Evidence: `L:/rssh-evidence/dual-adapter-startup-ef2560d0-20260914/bin-test-red.log`.
Tests referenced modern-only diagnostic helpers, report generation fields,
headless renderer constructors, CPU cache accounting and lazy GPU resource
accounting. Merely changing dependency feature forwarding cannot fix them.

## Test boundaries

- Keep modern diagnostic proof, partial-state accounting and retirement memory
  assertions unchanged under the modern capability selector.
- Keep four common real GPU text tests on both profiles: covered scripts, late
  fallback, bounded restart with tofu, and static fallback convergence. Legacy
  uses the old public constructor and reads actual live catalog generation and
  face count; it does not invent unavailable report/resource fields.
- Legacy partial-state cleanup and real device recovery remain tested in
  `rterm_compat_gpu::tests`, including actual pixel readback and failure paths.
- Limit the native eight-stage resource-accounting child/parent to modern.
  Keep shared timeout, child protocol, service audit and teardown tests.
- Add direct legacy diagnostic font preparation tests: ordinary production
  catalog succeeds without a diagnostic summary; all explicit modes and a
  specimen-only request return typed Unsupported.
- Fix the previous startup contract's legacy-only clippy item-order warning.
  Regenerate the Task 23 inventory with one added test; no removed mappings.

## Verification boundary

Before committing, modern GPU tests passed 35/35; legacy adapter selected against
modern dependencies passed 26/26. Both focused app bin/tests clippy invocations
with `-D warnings` passed. These selector checks are not frozen API proof.
The exact committed consumer must subsequently be prepared with unchanged
`0e8ebd5de22758275cbb6a849c19c032268d7fac` and run through actual frozen Cargo
compilation and tests. Keep those results separate from full workspace/CI and
performance certification. Independent review found no semantic must-fix issue;
the unrelated include-fragment formatting it identified was removed.
