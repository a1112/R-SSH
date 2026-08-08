# Dependency and supply-chain policy

All Rust and Node dependency changes must update and commit their lockfiles.
CI rejects known Rust advisories, high-severity Node advisories, unapproved
licenses, wildcard Rust requirements, and unknown registries or Git sources.
Temporary advisory or license exceptions require an inline reason, owner,
expiry date, and a linked tracking issue or migration plan.

The workspace patches `glyphon` 0.12.0 and `gpu-allocator` 0.28.0 to local
copies. Changes under `vendor/` must stay minimal, retain upstream license
files, identify the upstream version and patch rationale in the pull request,
and receive the same tests and audits as first-party code. Vendored code must
not be refreshed by copying an unreviewed source tree.

Release jobs generate SPDX SBOMs for final packages, produce SHA-256 checksums,
and attest build provenance through GitHub's artifact attestation service.
Release artifacts are not considered complete without these records.

Parser-facing dependencies such as `ttf-parser`, terminal escape processing,
image decoding, and SSH protocol crates receive priority during advisory review
because they process untrusted input. A dependency becoming unmaintained does
not by itself justify an unreviewed replacement; first record the reachable
surface, migration candidate, compatibility tests, and removal plan.
