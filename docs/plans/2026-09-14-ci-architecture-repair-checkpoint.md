# Post-migration CI architecture repair

Repository: `a1112/R-SSH`; PR 28; inspected consumer `f5dc0517`.

## Observed failures

The resumed Functional run `34845041571` succeeded. The two resumed CI runs
failed in `quality` and `R-Term candidate and rollback contract`.
CodeQL setup succeeded on the default branch `21dd01b3`; that does not certify
the PR head's earlier failed CodeQL run.

Quality's architecture gate reports `NativeWindowApp` with 69 fields against
68, and `part08.rs` with 8084 lines against 8081. The same failures were
reproduced locally before editing. The added GPU quarantine caused both.

## Repair

Group the adjacent active and quarantined GPU owners in `WindowGpuOwners`,
which also owns the existing quarantine transition. Preserve active-before-
quarantined drop order and both final-close paths. Rendering and resize still
access only the active owner; metrics continue to include quarantined owners.
No architecture limits, tests, frozen references or performance gates relaxed.

Verification: unchanged architecture gate passed; 2 fallback/quarantine tests
passed on modern and again with the legacy adapter selector; 31 window-manager
tests passed; modern bin/tests clippy with `-D warnings`, format and diff checks
passed. Independent review found no must-fix issue. This is not a new exact-
commit frozen consumer or complete CI certificate.

## Separate remaining failure

The rollback job fails during `cargo generate-lockfile`: its old overlay
preparation still forwards `rssh-fonts/diagnostic-tools` into frozen fonts.
The app adapter is available, but the approved Task 5 verified preparer has not
yet been wired into `rehearse-rterm-consumer.py`. That requires a separate tested
integration commit, retaining immutable receipts, all consumer commands and
fail-closed source verification. Do not remove the failed job or advance LKG.

Task 12 drafts remain untouched. No merge or physical extraction.
