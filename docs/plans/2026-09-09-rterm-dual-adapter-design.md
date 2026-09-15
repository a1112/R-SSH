# R-Term frozen-LKG dual-adapter design

Status: approved in the task on 2026-09-09; implementation and certification pending.

## Decision and invariants

Keep `last_known_good_rterm_ref` at
`0e8ebd5de22758275cbb6a849c19c032268d7fac`. Keep the independent Stage 7
history and product-performance references unchanged. Current R-SSH source must
consume both current R-Term and the frozen R-Term source. Do not substitute a
whole-product rollback, patch the frozen packages, or disable the failed check.

Use two app-owned adapters and a reproducible build-preparation entry point.
The modern adapter retains shared font allocation, lazy loading, generation-aware
text preparation and GPU resource retirement. The legacy adapter uses the old
public API to provide actual GUI, native SSH and GPU rendering. It does not
replace the product with a CPU-only or CLI-only implementation.

Alternatives rejected: restricting the whole product to old APIs would disturb
the completed optimizations; advancing the frozen reference would change the
approved rollback promise rather than satisfy it.

## Why a preparation step is necessary

The current manifest forwards `diagnostic-tools` and `shared-source-ownership`
to `rterm-fonts`. Neither feature exists in the frozen source. Cargo resolution
fails before Rust conditional compilation can select an adapter. Beyond that,
old source lacks `FontCatalog::load_sources`, catalog memory diagnostics,
generation-aware GPU text preparation, CPU font-state retirement and staged GPU
initialization diagnostics. Removing feature forwarding alone cannot fix it.

The standard preparation command will work identically in local builds and CI.
It operates only in an explicit disposable consumer checkout, after validating
immutable source and consumer commits. It verifies all seven overlaid packages
and both vendor trees against the declared source, and verifies retained product
source against the consumer commit. Unknown or mixed source is an error, never
an automatic legacy fallback.

Modern preparation preserves the app manifest byte-for-byte. Legacy preparation
may change only three app feature arrays: remove the two unavailable dependency
feature edges and append the app-owned `rterm-legacy-0-1` selector to
`production-gui`. That selector is declared in the normal source manifest but
never enabled by a normal modern build. No product Rust source is generated or
replaced. The lockfile may be regenerated using the existing contract policy.
Ordinary source worktrees must not be rewritten by this operation.

## Adapter responsibilities

The app boundary contains explicit modern and legacy implementations, selected
by the prepared manifest. Transport, terminal parsing, input, secrets, pane
generation, overlays and the CPU bootstrap remain shared product code.

Legacy font expansion must be transactional: validate/rebuild a complete ordered
source set with the old public API, then publish the new catalog and logical
generation. Do not emulate a batch by partially committing repeated loads.
Cache invalidation must account for catalog incarnation as well as generation.
Late fallback must preserve full-frame redraw and bounded restart behavior.

Legacy GPU rendering uses the old context and renderer entry points. Partial
prepared frames must never be presented after a catalog change. Recovery must
retain the established safe device/surface teardown behavior; a missing modern
retirement hook must not become an unsafe early drop. Actual native tests are
required for resize, text fallback and recovery.

Legacy diagnostic capabilities which cannot be measured faithfully return an
explicit unsupported result. No fabricated zero metrics, shared-allocation
claims, stage markers or performance certificates. Modern diagnostic behavior
and production performance gates remain unchanged. Legacy functional rollback
evidence is not Stage 7 production performance evidence.

## Evidence and verification

Preparation receipts identify profile, source/consumer full SHAs, source trees,
original/generated manifest SHA-256 and regenerated lockfile SHA-256. Build and
package receipts also record their exact artifacts and commands. Verify the
allowed mutation footprint and source identity again after preparation/build.
Retain receipts and command diagnostics on failure. A profile receipt alone
does not establish compilation, compatibility or successful rollback.

The rehearsal keeps its existing candidate/rollback source substitution and all
existing consumer commands. Extend its preparation phase and evidence; do not
relax command success requirements. Add packaged GUI/native SSH/GPU functional
evidence for both profiles, separate from protected performance certification.

Before merge: both rehearsals, the complete workspace suite, format, clippy and
PR checks must pass. Independently investigate the Windows window-probe
diagnostic failure and the previously observed local access violation. Physical
extraction remains NO-GO until the original Task 12 and later extraction gates
are met; this design does not provide fixed runners or authorize publication.
