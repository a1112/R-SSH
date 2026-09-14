# Frozen startup contract test compatibility checkpoint

This is a bounded continuation of Tasks 3–4 in the approved dual-adapter plan,
not completion of those tasks or approval for extraction.

## Failure and change

The `cb52d3d2` frozen consumer's `cargo test --locked -p rssh-app
--test stage5_startup_contract --no-run` freshly failed with E0599: frozen
`FontCatalog` has no `memory_metrics`. This is a separate integration target
failure in addition to the previously reported 30 app bin-test compile errors.

The startup contract now distinguishes the verified prepared legacy manifest
from the modern manifest. Modern shared-source feature and retained-byte
assertions are unchanged. Legacy verifies the exact empty forwarding arrays,
the explicit app selector, real fixture catalog loading, absence of the frozen
diagnostic feature, and failure to compile private proof constructors. Both
profiles retain decoder, deferred-startup and package workflow checks.

## Verification before commit

- Modern Cargo startup contract: 7 passed, none ignored.
- Current test source compiled directly with rustc against the actual frozen
  `rterm-fonts` artifact emitted by Cargo in the prepared `cb52d3d2` consumer:
  6 passed, none ignored. `CARGO_MANIFEST_DIR` pointed at that prepared consumer
  for manifest/source assertions; its files were not edited.
- Modern focused clippy with `-D warnings`, format and diff checks passed.

The direct harness checks this test source against real old APIs, not a complete
new consumer build. An exact committed consumer preparation and Cargo rerun are
the next verification step. No runtime implementation, frozen packages, baseline
references, budgets, release rehearsal commands or Task 12 drafts changed.

## Remaining work

The 30 app bin-test errors still need capability-aware test adapters and checks.
Further all-target compile failures may become visible afterward. Full workspace
tests, rollback rehearsal integration and exact-SHA CI are not certified by this
checkpoint. No merge or physical extraction is authorized by these results.
