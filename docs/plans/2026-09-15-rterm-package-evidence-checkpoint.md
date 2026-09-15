# Task 5 native archive identity checkpoint

PR #29's `702a06b7bdc265c0bbe36d86f25762085cff459d` passed CI, Functional and
CodeQL. Push run 34917050015 retained successful modern and legacy receipts for
that exact consumer, with frozen source `0e8ebd5de22758275cbb6a849c19c032268d7fac`.
The actual Linux executable identities were:

- modern: `2ec2f3d778f15f77d232e193e9731f85ea6c94f0a0a165d384b5076b3580c168`
  (422392256 bytes);
- legacy: `d0a153d91db378597ac9a6fc4944939046f4a9a2e7600f471030502a30e11400`
  (422536792 bytes).

## Current substep

Continue on the still-open PR #29 branch. Add unsigned native archive assembly
using the existing committed platform scripts, without changing their format,
the consumer build/test commands, frozen references or performance thresholds.
Each profile's archive is retained independently and bound to its previously
verified executable. The helper compares archive payload hashes without
extracting members, requires the packaged binary to match the build, and sets
manifest provenance from the immutable consumer commit rather than ambient CI
state. CI retains zip/tar.gz files with each mode's JSON.

Real temporary Git/Cargo fixtures run both native Windows package assemblies.
Three new assertions first failed on the previous implementation: missing
package evidence, packaging failure being ignored, and missing production
package selection. No test password or private key is included in packages.
Nine existing Task 12 drafts remain outside this change.

Review added two regression requirements. A malformed manifest initially raised
TypeError without evidence; explicit shape validation now reports a structured
failure. A rollback command initially could alter the retained candidate archive
while the overall rehearsal passed; final cross-mode size/hash/link validation
now invalidates the affected evidence and retains the failed workspace. Both
tests were observed failing before their fixes. Independent review accepted the
archive-identity scope after these corrections.

## Remaining

Confirm real product packaging on the new committed SHA in CI. Run packaged
GUI/native SSH/GPU acceptance for both profiles before closing Task 5. These
unsigned debug-profile rehearsal archives are not signed release builds or
performance GO. Task 6 and original fixed-runner/extraction gates still apply.
