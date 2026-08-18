# R-Term 0.1 release contract

R-Term remains a logical package family inside the R-SSH repository during
Stage 6. This contract validates a future repository boundary without moving
code or rewriting history. A physical extraction is Stage 7 work and is not
authorized by this contract.

## Compatibility policy

All public R-Term packages currently publish the `0.1` compatibility line.
Patch releases may add backward-compatible APIs and fix behavior. Removing or
changing a public API requires the next minor line, beginning with `0.2`.
Downstream rehearsals always identify both candidate and last-known-good inputs
with immutable 40-character Git commits; branches and tags are not release
evidence.

The package list, owned paths, internal dependency edges, last-known-good commit,
and vendored dependency trees are defined in
`scripts/ci/rterm-release-contract.json`. The contract forbids every dependency
from an `rterm-*` package to an `rssh-*` package.

## Consumer and vendor policy

The standalone consumer compiles against the seven public `rterm-*` packages at
version `0.1.0`. The real R-SSH consumer rehearsal overlays only contract-owned
R-Term paths into an independent clean checkout.

`glyphon` and `gpu-allocator` remain repository patches. A future consumer must
declare the same patches in its root manifest using the
`consumer-root-path-patch` strategy; transitive package manifests must not try to
own those patches. Each patch is pinned by its Git tree identity, so content
drift fails the contract even when the directory name is unchanged.

## History extraction

`docs/release/rterm-history-paths.txt` is the reviewed old-to-current path map for
future history extraction. It is evidence and planning input only: Stage 6 does
not run `filter-repo`, create a second repository, publish crates, or change the
authorized single-repository topology.
