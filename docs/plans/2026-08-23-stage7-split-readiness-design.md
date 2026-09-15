# Stage 7 Split Readiness and Physical Extraction Design

**Date:** 2026-08-23

**Status:** Approved; implementation is gated by the Gate 0 proofs below

**Baseline:** `21dd01b3d73dd9c9241ac10e7a25d92cb2bcfea6`

## Objective

Earn a machine-verifiable Stage 7 GO decision without weakening the approved
memory contract, then physically extract the seven R-Term packages into an
independent repository while preserving R-SSH behavior, history, rollback, and
release evidence.

The work has two strictly ordered outcomes:

1. make the existing split conditions measurable and pass them; and
2. perform the repository split only after all conditions are green.

## Current Decision and Evidence

Stage 7 is currently a strict NO-GO. The protected Windows x64 release matrix at
commit `53dec0ed22db168d235670a982652d8f71206ac0` measured the following empty
window Private Working Set p95 values:

| Renderer | p95 | 45 MiB target |
| --- | ---: | --- |
| CPU | 7.34 MiB | met |
| DX12 | 246.32 MiB | not met |
| Vulkan | 249.30 MiB | not met |
| GL | 227.34 MiB | not met |

The aggregate evidence is fixed by SHA-256
`40781B82D6F13DDCCD24CAE891A58BF7350B2C09D70A27B7EAF2FCE9739A3829`.

The first code-level attribution pass found a large R-Term-owned contributor,
not merely an unexplained driver floor. The Windows GPU path reads twelve system
font files totalling 90,121,536 bytes (85.95 MiB). `FontCatalog` retains the
bytes in `Arc<[u8]>`, then `FontCatalog::build` copies every source with
`to_vec()` into fontdb. The resulting stable state retains at least two copies,
accounting for roughly 171.9 MiB before WGPU, the platform driver, surfaces,
pipelines, atlases, and other resources are considered. Catalog construction
also creates shorter-lived copies, and loading each platform font through
`load_source` rebuilds the complete database repeatedly.

This evidence makes font ownership and loading the first remediation target. It
does not yet prove the size of the minimum WGPU/platform baseline.

## Chosen Approach

Use a strict-gate-first design with an evidence-based fallback:

1. keep the original absolute memory targets unchanged;
2. add a cumulative GPU initialization matrix and project-owned resource
   counters;
3. remove duplicate font ownership, repeated catalog rebuilds, and unnecessary
   empty-window resources;
4. rerun the protected release gates;
5. proceed directly to extraction if the original targets pass; and
6. if a minimal WGPU clear frame alone is proven to exceed 45 MiB, stop and
   submit a separate design for a platform-baseline plus R-Term-delta gate.

The fallback is not part of the current GO criteria. It cannot silently replace
the absolute target or authorize extraction.

### Gate 0 feasibility proofs

Three bounded proofs must pass before production remediation or extraction code
begins:

1. **Font ownership proof:** compare the current catalog with a shared-Binary
   catalog and a path-indexed, demand-loaded catalog on the fixed Windows host.
   Record Private Working Set, retained source bytes, catalog build count, ASCII
   startup, first CJK/emoji activation latency, and post-activation rendering.
2. **Stage stopping proof:** demonstrate a typed diagnostic controller that can
   hold a fresh process at `cpu-window`, `instance-surface`, `adapter-device`,
   and `configured-surface-clear` without scheduling later work. Adapter/device
   creation and surface configuration may be split internally to make those
   states real.
3. **External-source proof:** create two local bare Git repositories from clean
   disposable clones, consume all seven R-Term packages from one immutable Git
   SHA, prove both vendor patches resolve from the R-SSH consumer root, verify
   the committed lockfile with `--locked`, and rehearse a one-commit rollback.

Any failed proof returns the design to `blocked` and requires a documented
revision. A proof result cannot authorize a product backend change, remote
repository creation, or physical extraction.

Each proof writes hashed raw records and one versioned result. The font proof
passes only when fontdb and `FontSource` share one allocation per active source,
inactive entries retain zero font-file bytes, ASCII activates no large fallback,
the initial catalog is built once, each activation batch adds exactly one build,
CJK and emoji render without tofu, every presented frame uses one catalog
generation, and device recovery does not multiply retained source bytes. On the
same backend and host, shared Binary must lower median Private Working Set by at
least 64 MiB from the current baseline and lazy ASCII must lower it by at least
another 32 MiB. First fallback activation latency is recorded report-only until
a stable envelope exists. The stage-stopping and external-source proofs pass
only when their structural assertions and all configured commands succeed with
zero dirty files.

The font proof uses the normal production `auto` selection and requires the
same actual backend for `current`, `shared`, and `lazy`. The three variants are
interleaved by round, each with five warmups and at least thirty process-cold
measurements. Every process stabilizes for 5,000 ms and contributes the
nearest-rank median of ten 100 ms memory samples. The proof computes
`current_median - shared_median >= 64 MiB` and
`shared_median - lazy_median >= 32 MiB` from the thirty process
representatives. All variants must use identical source commit, release binary,
runner fingerprint, geometry, and sampling configuration; otherwise the proof
is certification-ineligible.

The following alternatives remain rejected:

- **Change the split gate to an incremental metric immediately.** This would
  move the target before the known 171.9 MiB font problem is fixed.
- **Keep optimizing without stage attribution.** This would not distinguish
  project resources from WGPU/platform initialization and could not prove
  infeasibility.
- **Move GPU rendering to a helper process.** The current Stage 0 sampler
  intentionally measures only the launched child, so a helper could make the
  existing number look smaller without reducing total memory. Any future helper
  design would first have to approve a process-tree metric contract. It would
  also add window, input, IME, clipboard, snapshot IPC, and security complexity
  that is unrelated to repository ownership.
- **Make CPU rendering the permanent production default.** CPU fallback remains
  mandatory, but removing the approved GPU-active product path is not an
  authorized way to pass Stage 7.

## Split Gate State Machine

The Stage 7 contract is fail-closed and moves through these states:

```text
blocked
  -> attribution-ready
  -> windows-memory-go
  -> cross-platform-go
  -> extraction-ready
  -> dual-source-verified
  -> split-complete
```

- `blocked`: the current state; one or more hard conditions fail or evidence is
  missing.
- `attribution-ready`: cumulative stage probes are self-identifying,
  reproducible, and validated from raw records.
- `windows-memory-go`: the protected Windows runner passes all absolute and
  relative product budgets with production renderer semantics.
- `cross-platform-go`: protected Linux and macOS jobs pass native function,
  package, and relative-regression checks using their platform-specific memory
  metrics.
- `extraction-ready`: the complete owned-path manifest, history provenance,
  standalone R-Term workspace, vendor strategy, and rollback inputs validate.
- `dual-source-verified`: R-SSH builds and tests against one immutable commit
  from the independent R-Term repository while local R-Term paths remain
  recoverable.
- `split-complete`: local R-Term packages have been removed from R-SSH, both
  repositories pass protected CI, and one full release rehearsal succeeds.

No state may be inferred from a document alone. A versioned Stage 7 JSON
contract and validator must derive it from immutable evidence.

## Performance and Evidence Contract

### Product gates

The protected Windows x64 fixed runner remains authoritative for absolute
budgets. It uses a locked release build, 80x24 geometry, scale factor 1.0, five
warmups, and at least thirty process-cold measurements.

The unchanged hard budgets are:

- first-present p95 at or below 500 ms;
- first-frame Private Bytes p95 at or below 55 MiB and every sample below
  60 MiB;
- GPU-active empty-window Private Working Set p95 at or below 45 MiB;
- one native SSH pane Private Working Set p95 at or below 60 MiB;
- existing GPU steady-state maximum at or below 256 MiB; and
- no same-machine candidate regression above 5% versus the immutable
  last-known-good input for latency or memory.

The empty-window gate must use the normal production `auto` renderer and prove
that the final renderer is GPU. A CPU-only success cannot satisfy it. SSH1 must
retain native SSH, host-key, secret-handling, cancellation, and reconnect
semantics, reach `connected`, and report the same production `auto` GPU backend
at steady-state. Both product scenarios use one median representative per
process followed by nearest-rank p95 over at least thirty process-cold runs.

The versioned contract lists `auto` as the required Windows product backend.
DX12, Vulkan, and GL remain required diagnostic probes when the fixed host
supports them, but their individual 45 MiB results are explanatory and do not
all have to pass. An unsupported diagnostic-only backend is recorded and does
not block GO; a missing or failed production `auto` probe always blocks. If a
future design changes the production backend set, that design must update this
required-versus-diagnostic list explicitly.

Linux records PSS and macOS records physical footprint. Their protected jobs
must produce valid native samples, pass product/package tests, and remain within
5% of their immutable platform LKG. Their absolute 45/60 MiB observations stay
visible; promotion to a new cross-platform absolute hard gate requires explicit
contract history rather than an implicit metric comparison across operating
systems.

### Cumulative attribution matrix

Each probe starts a fresh process and stops at exactly one cumulative stage:

```text
cpu-window
instance-surface
adapter-device
configured-surface-clear
layer-pipelines
fixture-font-text
platform-font-index
full-frame
```

The runner interleaves stages by round to reduce temperature and background
load bias. For every stage it records the requested backend, source and binary
hashes, dimensions, milestones, memory samples, and final stage. Actual backend
and adapter identity are required from `adapter-device` onward and must be
absent from earlier stages. Unsupported stages fail; they do not fall back to a
different renderer or backend.

Every stage emits one `attribution_stage_ready` marker only after all work owned
by that stage is complete. The launcher then applies the same 5,000 ms
stabilization period and ten 100 ms child-process samples used by the existing
GPU matrix. Stage timeout remains bounded at 60 seconds. A stage controller
prevents later GPU, font, renderer, configuration, PTY, or SSH tasks from being
scheduled; the aggregate validator rejects a record if any later-stage marker or
resource counter appears. This freezes residence time and stopping behavior so
adjacent stages are comparable.

Each measured process contributes the nearest-rank median of its ten memory
samples. The reported p50 and p95 are then nearest-rank statistics over the
thirty process representatives; the maximum is the largest raw memory sample.
Raw values, per-process representatives, and aggregate values are all retained.
The existing 2026-08-23 matrix remains valid historical NO-GO evidence but is
not silently reinterpreted under this new aggregation rule.

The fixed-runner fingerprint includes OS build, GPU vendor/device and driver,
WDDM version, installed RAM, pagefile mode, display resolution and DPI, power
plan, local-versus-remote session state, locale, a sorted digest of candidate
font identities and content, font-index policy version, and cold-cache policy.
A mismatch with the protected runner contract makes certification ineligible.

This runner fingerprint is intentionally order-insensitive for candidate font
inventory. It is distinct from the catalog fingerprint used by rendering and
caches. The catalog fingerprint includes locale, the ordered active-source
content digests, face/collection identities, and the catalog policy version.
Activation atomically commits the new ordered fingerprint and generation, then
invalidates shape, raster, glyph-atlas, and GPU text caches as one transaction.

Renderer metrics additionally report only project-owned quantities that can be
counted without guessing: retained in-memory font bytes, indexed and active font
source counts, glyph atlas bytes, raster-cache bytes, image texture bytes, snapshot
bytes, and explicitly allocated buffer/texture bytes. OS Private Working Set
remains the authoritative absolute metric. Project counters explain it but do
not replace it.

If `configured-surface-clear` still exceeds 45 MiB after project-owned resources
are bounded, the evidence may justify a separate gate-change design. The stage
must be described as a WGPU/platform baseline, not as pure driver memory.

## Memory Remediation Architecture

### Shared font sources and a lazy platform index

The public `FontSource::new`, `FontSource::bytes`, and existing in-memory
behavior remain source compatible. A new shared constructor lets `FontSource`
and fontdb's `Source::Binary` own the same allocation instead of calling
`to_vec()`. Bundled fixtures remain immutable in-memory sources.

The locked `cosmic-text 0.19.0` cannot shape a plain
`fontdb::Source::File`; a `SharedFile` implementation would require a separately
reviewed dependency upgrade or mapping strategy and is not assumed here.
Instead, an app-owned platform font repository indexes candidate paths and
coverage without making every large font resident. On activation it reads the
selected file exactly once into shared immutable bytes and adds that shared
Binary source to the catalog. The repository survives renderer recreation and
device recovery so a recovery does not duplicate the active font set.

The catalog records retained in-memory byte totals plus indexed and active
source counts. Embedded and activated bytes use a content digest. Inactive
entries use an internal bounded path/metadata identity only for invalidation;
raw font/host paths, environment values, and unapproved machine fields never
enter Stage 7 markers or aggregate evidence. The schema explicitly permits the
enumerated runner-fingerprint fields and irreversible font digests. Existing
caller-facing `FontSource::from_file`, error, and Debug behavior remains
compatible; the diagnostic boundary performs the redaction.

Initial active sources are collected first and the catalog is built once. Adding
a batch is transactional: one invalid source rejects the candidate batch
without mutating the active catalog. The existing single-source API stays as a
compatibility wrapper over the batch path. Font parsing, color-table checks,
coverage, and metrics use the database's borrowed face-data callback so they do
not require a second owned byte vector.

### Demand-driven resources

The ASCII empty-window path loads only the primary terminal face plus the
bundled emergency coverage needed for visible bootstrap text. Large CJK, emoji,
and other platform fallbacks are indexed and activated through a preflight over
the complete frame snapshot before text cache scope and row shaping begin.
Activation advances the catalog generation, invalidates bounded shape/raster
caches, and restarts the whole frame if another missing script is discovered.
Catalog generation may not change silently in the middle of row shaping, so one
presented frame never mixes old and new generations.

GPU image pipelines and textures are created only when a snapshot contains an
image layer. Any cursor-specific renderer that duplicates durable resources is
created on first use. Surface frame latency, WGPU allocator block hints, and
backend restriction are diagnostic experiments, not assumed production
optimizations. Any change to the normal `auto` production backend set or
selection policy requires a separate design and approval, followed by
compatibility, recovery, and presentation certification. Stage 7 memory
evidence alone cannot change production backend semantics.

CPU fallback stays alive across GPU initialization and failure. No remediation
may move font scanning, PTY creation, SSH authentication, or GPU device creation
back in front of first present.

## Physical Split Boundary

The independent R-Term repository contains these seven packages:

- `rterm-types`;
- `rterm-terminal` (currently at `crates/rssh-terminal`);
- `rterm-runtime` (currently at `crates/rssh-runtime`);
- `rterm-fonts`;
- `rterm-render-core`;
- `rterm-render-cpu`; and
- `rterm-render-wgpu`.

It also receives the R-Term-owned fixtures, provenance tools, licenses,
architecture/performance documents, root workspace files, and independent CI.
The extraction manifest must enumerate and hash all of them. In particular it
must cover the Task 10 trace provenance assets, font fixtures, and the
`glyphon`/`gpu-allocator` vendor trees.

R-SSH retains the application, domain, config, diagnostics, native SSH and PTY
adapters, web/Tauri frontends, product tests, packaging, and the temporary
`rssh-renderer` compatibility facade. Product-coupled facade and transcript
tests move back to R-SSH before extraction so the R-Term workspace is
independently testable.

The original R-SSH repository history is never rewritten. History extraction
runs only in a disposable clone and produces a source-to-filtered commit map.
The new R-Term repository normalizes the legacy terminal/runtime directory names
after extraction with history-preserving moves.

The Stage 6 single-repository release contract remains immutable historical
evidence. Stage 7 introduces release-contract schema v2 with three explicit
views:

- the monorepo view validates R0 and the legacy owned paths;
- the R-Term view validates the filtered SHA, normalized package paths,
  fixtures, vendor trees, and standalone workspace; and
- the R-SSH view validates that all seven packages resolve from the exact
  external Git SHA.

The v2 checker accepts both repository roots and the source-to-filtered map. It
records full R0 and R-Term SHAs and rejects a mapping whose current tree/blob
identities do not match the extraction manifest. Schema v1 is not rewritten and
cannot be used to claim a post-split GO state.

Task 10 provenance uses a dual-repository verifier after extraction. It reads
the immutable source commits, trees, and blobs from a clean R-SSH R0 clone, then
reads current code and fixtures from the filtered R-Term SHA and validates their
content-addressed relationship through the source-to-filtered map. The filtered
repository is not expected to contain or reproduce original monorepo commit
IDs. A self-contained signed/content-addressed attestation may replace the
dual-repository read only through a separately reviewed schema migration.

Both repositories initially retain identical, hashed `glyphon` and
`gpu-allocator` vendor trees under the existing consumer-root patch strategy.
The external-consumer validator inspects complete Cargo metadata and requires
both patched packages' resolved `manifest_path` values to point at the R-SSH
consumer vendor directories; directory presence alone is insufficient.
Replacing them with immutable upstream/fork revisions is separate work.

## Source Switch, Publication, and Rollback

The proposed new remote is `https://github.com/lcxinc/R-Term.git`, but creating
or pushing it requires explicit authorization at execution time. Publication is
split into explicit steps to avoid a protected-CI bootstrap cycle:

1. pass the full standalone R-Term contract locally in a disposable checkout;
2. obtain authorization to create an isolated/private repository and push one
   candidate branch without changing its default branch;
3. configure and pass protected CI on that candidate;
4. obtain the normal approval to update the default branch or make the
   repository public; and
5. only then allow R-SSH to consume the protected immutable commit.

R-SSH switches to an immutable R-Term Git commit in two steps:

1. centralize the seven package sources under root workspace dependencies, then
   update every product manifest and the standalone consumer to consume one
   external URL and full commit while the local seven package directories
   remain present but excluded from the workspace;
2. after external-consumer, package, functional, performance, and rollback
   rehearsals pass, delete the local package directories in a separate commit.

The authenticated rehearsal receives separate R-SSH and R-Term repository URLs
plus full SHAs. It never runs `cargo generate-lockfile`: committed lockfiles are
inputs and every command uses `--locked`. Cargo metadata must show all seven
packages from the same Git commit, no `path+file` source for those packages, the
expected consumer-root vendor patches, and no worktree or lockfile change after
the build. A private R-Term repository additionally requires an approved
read-only cross-repository credential and a documented fork-PR degradation
policy before the candidate branch is used by CI.

Rollback points are immutable and explicit:

- `R0`: full pre-split R-SSH commit SHA;
- `R1`: extracted R-Term exists but R-SSH still uses local paths;
- `R2`: R-SSH consumes external R-Term while local packages remain; revert one
  source-switch commit to restore path dependencies;
- `R3`: local packages are deleted; revert deletion first, then source switch.

The first R-Term LKG is the filtered commit corresponding to the approved R-SSH
boundary, not the old monorepo SHA. The source-to-filtered map makes that
relationship auditable. Every R0-R3 point is recorded as a full immutable commit
SHA. A protected tag may be an additional human-readable alias but is never the
sole rollback identity.

## Failure, Security, and Cleanup Policy

- Any missing sample, hash mismatch, backend mismatch, unsupported metric,
  mutable ref, dirty extraction input, reverse dependency, vendor drift, or
  failed rollback keeps the state at NO-GO.
- Optimization failures retain the CPU renderer and current monorepo topology.
- Temporary extraction directories are created under a caller-selected bounded
  root. Successful rehearsals may remove their own verified temporary clones;
  failed rehearsals retain evidence and exact paths for diagnosis.
- Passwords, passphrases, keys, host paths, environment values, and terminal
  content do not enter stage markers or aggregate evidence.
- Before the first remote push, scan every reachable filtered commit and Git
  object, both vendor histories, fixtures, and large objects for credentials,
  private keys, machine-specific paths, and forbidden material. Generate a
  license/SBOM inventory and fail on an unapproved or missing license. The Stage
  7 gate records the scanner versions, input commit, result hashes, and zero
  unresolved findings.
- The split does not change SSH secret lifetime, known-host behavior, terminal
  scrollback ownership, or stale-generation cancellation.
- Remote repository creation, history publication, default-branch updates, and
  deletion of the local R-Term packages are separate externally visible actions
  and require their normal review/approval boundaries.

## Verification Strategy

All behavior changes follow RED/GREEN TDD. Required checks include:

- `FontSource` shared-Binary ownership, lazy platform indexing, transactional
  batch load, generation, invalidation, fingerprint, and retained-byte metrics;
- platform fallback selection for ASCII, CJK, emoji, Arabic, Devanagari, Hebrew,
  and missing-font cases;
- cumulative stage CLI/schema/parser validation and exact stage stopping;
- raw-artifact recomputation of p50/p95/max and source/binary hashes;
- CPU/GPU snapshot equivalence, same-window handoff, device loss, resize, DPI,
  IME, image lazy creation, and CPU fallback;
- fixed Windows empty-window and SSH1 5+30 release gates;
- native Linux/macOS metrics, package smoke, and LKG comparison;
- a complete extraction-manifest validator and disposable-clone filter rehearsal;
- standalone R-Term workspace fmt, Clippy, tests, public API probe, vendor tree,
  Task 10 provenance, and clean-clone build;
- real R-SSH consumption from an immutable external R-Term commit;
- locked external-source metadata, consumer-root vendor resolution, unchanged
  lockfiles/worktrees, and source-switch rollback;
- package smoke, native SSH/PTY/functional tests, and a
  full release rehearsal before local package deletion; and
- secret scans over stdout, stderr, markers, JSON, package logs, and visible
  snapshots;
- full filtered-history and object scans plus license/SBOM validation before
  any candidate branch is pushed to the new remote.

## Exit Criteria

Stage 7 reaches `split-complete` only when all of the following are true:

1. the versioned gate validator reports GO from immutable raw evidence;
2. Windows passes the unchanged absolute startup, empty-window, SSH1, GPU
   steady-state, and relative-regression budgets;
3. Linux and macOS protected native baselines and relative gates pass;
4. the complete extraction manifest and history map validate;
5. the filtered history passes credential, private-key, machine-path,
   large-object, license, and SBOM checks;
6. the independent R-Term workspace and protected CI pass through the isolated
   candidate-branch bootstrap flow;
7. R-SSH consumes an immutable R-Term commit and passes consumer, package,
   native functional, performance, and rollback rehearsals;
8. the original R-SSH history remains unchanged;
9. local R-Term packages are removed only after dual-source verification; and
10. both repositories complete one protected release cycle with no open critical
   or important review findings.

Until then, R-Term remains logically separated inside the R-SSH monorepo.
