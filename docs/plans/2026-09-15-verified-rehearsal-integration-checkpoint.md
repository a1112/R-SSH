# Task 5 verified rehearsal integration checkpoint

This checkpoint covers preparation integration, not completion of Task 5 or
approval for physical extraction.

## Change

- The production contract selects `verified-dual-adapter-v1` for candidate and
  frozen rollback consumers. Both invoke the existing common preparer.
- Original standalone probe and all six consumer commands remain unchanged.
  The lock-generation command runs inside verified preparation.
- Each mode retains preparation provenance and rechecks actual source/product
  bytes and the generated lockfile after consumer commands. A failed command or
  identity check cannot become success. Failed checkouts remain available.
- The immutable consumer's product contract is checked before running any
  contract-controlled command. A dirty selector cannot downgrade verification.
- CI runs the profile/preparer/integration regression suites before the real
  candidate/rollback rehearsal. Owned temporary checkouts and default Cargo
  outputs are outside the verified source trees.

## Verification scope

Real temporary Git repositories and Cargo lock generation cover both profiles,
failure propagation, retained preparation errors, source/lock mutations and
dirty-contract probe rejection (including removed/null selectors). Regression
tests demonstrated the old failure and the unsafe probe ordering before fixes.
Independent review accepted the preparation-integration scope after correcting
the validation-order and selector-downgrade issues.

The existing release-contract, profile, preparer and generic rehearsal suites
contain 47 tests; the new verified rehearsal suite contains seven tests. Rust
format and the unchanged architecture policy are checked as well. No product
Rust, frozen source, performance threshold or baseline reference is changed.
The nine existing Task 12 draft files are excluded from this commit.

## Remaining acceptance

- Observe real candidate and rollback CI against this integration's committed
  consumer SHA; fixture success does not certify the actual product.
- Record executable/package hashes and complete both-profile packaged
  GUI/native SSH/GPU functional evidence.
- Continue Task 6 and the original fixed-runner/extraction gates. This change
  supplies neither performance GO nor merge/physical-split approval.
