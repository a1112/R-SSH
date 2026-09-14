# Legacy default diagnostic compilation boundary

Task 4 continuation, based on consumer
`afd2ed4aabd4dcb3e1b2627a908c92c219b81d60`. The previous recovery commit was
already pushed; this change does not include the nine unfinished Task 12 drafts.

## Scope

The frozen default binary's 14 compiler errors were reproduced before this
change in `diagnostic-red-20260914.log` under
`L:/rssh-evidence/dual-adapter-gpu-recovery-afd2ed4a-20260909`.
They belong to two unavailable capabilities: staged GPU resource attribution
and font memory/catalog-frame diagnostics. Production GPU recovery, resize and
ten-frame rendering had already passed on that exact consumer.

Modern implementations and their original feature constraints remain intact.
An additional legacy exclusion prevents compiling unavailable APIs. The legacy
attribution entry returns Unsupported, without producing substitute resources.
The font preparation boundary keeps normal None/None production initialization
and returns no diagnostic summary; explicit diagnostic font options are rejected.
The legacy presented-frame path never attempts modern resource finalization.

The existing CLI early rejection remains the externally reachable boundary.
Internal window diagnostic functions are not new public API: direct internal
calls are not claimed to be independently guarded before all setup. Their
existing sole CLI dispatcher rejects legacy diagnostic commands before entry.

## Verification boundary

The new bounded process test exercises bench, doctor, self-test, ordinary GUI
diagnostics, attribution-stage and font-proof arguments. It requires non-success,
empty stdout and exactly the legacy Unsupported error on stderr. A first test
fixture omitted mandatory `--hold-ms`; that invalid fixture was corrected rather
than changing argument validation. All six cases pass with the local selector.

Modern font diagnostic regression tests pass (13 tests). The Stage 7 source
contract caught placement of the additional cfg after its original annotation;
the added condition was reordered, preserving the original assertion rather
than relaxing it. The rerun passes all 22 Stage 7 stop-stage tests; one child-only
entry remains ignored for direct invocation and is launched by the bounded
native parents. The three focused Stage 7 marker/teardown tests also pass.

The exact committed frozen consumer must next pass its default binary build and
the new process boundary test, plus the reduced production configuration. This
checkpoint does not claim frozen all-target unit-test compatibility: modern-only
test bodies still need an explicit capability review. It does not authorize
rehearsal integration, merge, performance certification or physical extraction.
All frozen baselines and original performance limits remain unchanged.
