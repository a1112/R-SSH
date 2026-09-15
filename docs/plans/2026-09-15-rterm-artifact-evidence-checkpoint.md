# Task 5 executable identity checkpoint

PR #28 was merged by the user as `094c9d9fbb0615366fa97506d3700fd67ef6c6de`.
The next work is based on that main commit, on `codex/rterm-artifact-evidence`,
reusing the isolated stage7 worktree and preserving all nine Task 12 drafts.

## Prior integration evidence

Push CI run 34912908830 passed with candidate and consumer both
`6fd3a746b276caedd6bc4e1268e9d7a706bbf859` and frozen R-Term
`0e8ebd5de22758275cbb6a849c19c032268d7fac`. PR CI run 34912914028 also passed,
using its synthetic merge commit `83a7b55ccf376cd60ec893710326f770d48d5575`.
Functional run 34912914047 and CodeQL run 34912909492 passed. These observations
close the previous integration's actual candidate/rollback execution gap, not
the original performance or physical-extraction gates.

## This substep

The immutable release contract now requires the built executable's SHA-256 and
size immediately after the existing product build command. Candidate/rollback
records remain distinct even when Cargo target storage is shared. The runner
checks the artifact again after later consumer commands and fails on mutation,
missing/empty files, or links. Existing build/test commands and frozen refs are
unchanged. Hashes remain inside each mode's retained JSON; this does not archive
the executable or create a release package.

Review identified stale cross-profile artifacts and inherited test target
directories. Tests now use a fixture-owned target. Each declared build first
moves any validated old regular output to an owned recovery directory, leaving
the dependency cache intact; missing/reconfigured outputs cannot borrow an old
hash. Failure retains the old file and its backup path in evidence, while
successful cleanup removes owned old copies. Two stale-output regression tests
failed before this fix; an actual Cargo-build fixture checks cache reuse and
recreation of the required binary across both modes.
The recovery destination and its existing ancestors are also checked before
directory creation and again before moving. A real Windows junction fixture
first demonstrated an external write; the regression requires the original
artifact to remain in place and the external directory to remain empty.

Four regression tests first failed on the previous runner/contract: absent hash
evidence, missing artifact falsely passing, later artifact mutation falsely
passing, and absent production artifact requirement. Real temporary Git/Cargo
fixtures exercise the common preparer in both profiles; they do not substitute
for actual product CI on this new commit.

## Remaining

Observe the new exact-SHA CI's real executable identities. Add archive/package
hashes and both-profile packaged GUI/native SSH/GPU functional evidence before
closing Task 5. Task 6 and fixed-runner performance/extraction gates remain
separate; no GO or physical extraction is claimed here.
