# Stage 6: Cross-Repository Release Contract Design

## Status and scope

Stage 6 prepares the logical R-Term surface for a later physical repository
split. It does not create a second repository, rewrite history, or replace the
current workspace path dependencies. Those operations remain Stage 7 and require
separate approval.

The approved Stage 6 outcome is an executable release contract: a candidate
R-Term revision can be overlaid into a clean R-SSH consumer checkout, the
declared downstream API compiles, the candidate and last-known-good revisions
both pass the required consumer gates, vendor patches resolve identically, and
fixed-runner performance evidence remains within the existing absolute and
relative budgets.

## Recommended architecture

Use a single-repository, two-checkout rehearsal. CI creates independent clean
directories for:

- the candidate R-Term source from the current commit; and
- the R-SSH consumer from the configured consumer ref.

The rehearsal copies only the declared R-Term-owned paths into the clean
consumer. R-SSH crates, application code, workflows, and package scripts remain
from the consumer checkout. This proves that the consumer compiles against the
candidate boundary without pretending that the physical split already exists.
The same mechanism overlays the immutable last-known-good R-Term ref and reruns
the consumer/package gate, making rollback a one-ref operation.

Alternatives were rejected for Stage 6:

- a temporary second GitHub repository would prematurely perform Stage 7;
- Git submodules add checkout state without improving Cargo resolution;
- publishing temporary crates would weaken reproducibility and require registry
  credentials; and
- testing only the monorepo workspace would not prove the downstream consumer
  boundary.

## Versioned API and compatibility policy

All declared R-Term packages remain on the `0.1` compatibility line. Patch
releases may add compatible APIs and fixes; removing or changing a declared
public API requires the next minor version. `1.0` is deferred until the
runtime, snapshot, and renderer contracts are mature.

A standalone downstream probe crate depends on every declared R-Term package
and compiles representative public types, runtime batches, terminal snapshots,
font APIs, and CPU/WGPU entry points. It must not depend on any R-SSH product
crate. The real R-SSH consumer rehearsal then checks the application and the SSH,
PTY, native, and functional adapter crates against the candidate overlay.

## Vendor patch decision

Stage 6 adopts an explicit consumer-root patch strategy for the existing
`glyphon 0.12.0` and `gpu-allocator 0.28.0` forks. The contract records the
exact vendor paths and Git tree object IDs, and verifies that both candidate and
consumer roots resolve those paths. This is a deliberate transitional strategy:
the patch is reproducible and no longer implicit, but Stage 7 may not remove the
consumer-root patches until the changes are upstreamed or replaced by immutable
fork revisions.

## History extraction map

A checked-in path map freezes all old and new R-Term crate paths, both vendor
forks, licenses, and architecture/performance documents. Validation checks that
current paths exist and that historical predecessor paths are present in Git
history. Stage 6 does not run `git filter-repo`; it only makes the future input
reviewable and deterministic.

## CI, performance, and rollback

Hosted PR CI runs deterministic contract validation, the standalone API probe,
the clean R-SSH consumer candidate build, and the last-known-good rollback
rehearsal. It emits structured JSON evidence with refs, tree identities,
commands, and outcomes.

The protected Windows fixed runner remains the only authority for absolute and
relative startup or memory gates. It compares the minimal production GUI built
from the candidate with the immutable last-known-good R-Term revision on the
same machine, while retaining the Stage 5 first-present and Private Bytes
limits. Package smoke remains required after the candidate build and after the
rollback rehearsal.

## Failure policy and exit criteria

The contract fails closed on a mutable or missing ref, an undeclared R-Term
path, a reverse `rterm-* -> rssh-*` dependency, an API probe failure, a vendor
tree mismatch, a consumer build failure, a rollback failure, or a protected
performance/package regression. Hosted runners do not enforce absolute memory
or latency thresholds.

Stage 6 exits only when:

- the declared R-Term API and version policy are machine checked;
- a clean R-SSH consumer compiles and tests against the candidate overlay;
- the immutable last-known-good ref can be restored by changing one contract
  ref and passes the rollback gate;
- vendor patch resolution and the future history extraction map are explicit;
- release/package and protected fixed-runner comparisons pass; and
- no Stage 7 repository creation or history rewriting has occurred.
