# Legacy font adapter foundation checkpoint

This is a bounded part of dual-adapter Task 3, not completion of that task.
The adapter is compiled for tests and the inert legacy selector only; it is
not yet called by `PlatformFontRepository`. Modern production is unchanged.

## Implemented

- Reconstruct an ordered catalog at a validated logical epoch with the frozen
  public `from_sources` and single-source `load_source` APIs.
- Build the replacement privately and publish only after every source succeeds.
- Advance an expansion batch by one logical epoch. Recovery preserves the epoch
  but obtains a new incarnation, invalidating instance-scoped font IDs.
- Do not report these logical epochs as measured allocation/build diagnostics.

## Evidence

The first executable RED run failed three success-path assertions because the
adapter returned its explicit not-implemented error. The implementation then
passed the five initial tests; two additional checks exercise source ordering
and cached font-ID invalidation at the same epoch.

- Modern `cargo test --locked -p rssh-app --bin rssh-app legacy_fonts -- --test-threads=1`:
  7 passed.
- Modern `cargo test --locked -p rssh-app --bin rssh-app platform_fonts -- --test-threads=1`:
  27 passed.
- `cargo test --locked -p rssh-app --test task23_test_manifest`: 1 passed.
- `cargo clippy --locked -p rssh-app --bin rssh-app --tests -- -D warnings`,
  `cargo fmt --all -- --check` and `git diff --check`: passed.
- The same `rterm_compat.rs` was compiled as a standalone Rust test harness
  against the real frozen `rterm-fonts` rlib: 7 passed. This isolates the font
  adapter from the still-unimplemented app GPU compatibility boundary; it is
  **not** a legacy app build or GPU test.

Frozen evidence is retained under
`L:/rssh-evidence/dual-adapter-frozen-20260909`. `preparation.json` still names
source `0e8ebd5de22758275cbb6a849c19c032268d7fac` and consumer
`2902c42beb8abfdadb4e1d75ec3210b9119e4632`; it has not been relabeled as evidence
for a newer consumer. `font-build.jsonl` records the exact frozen Cargo artifact
used by the standalone harness. No frozen package source was changed.

## Next

Wire the adapter into repository activation/recovery, implement explicit
unsupported legacy diagnostic commands, and verify the complete font boundary.
Then implement the GPU adapter before changing rehearsal preparation. Re-run
both complete profiles from newly prepared exact-commit checkouts. CI rollback,
the separate Windows quality failure, and physical-split certification remain
open. All immutable LKG references and performance budgets remain unchanged.

Independent read-only review found no Critical/Important issues in this bounded
step. Its optional repeated-expansion test should accompany repository wiring,
where consecutive differently sized batches can exercise the actual caller.
