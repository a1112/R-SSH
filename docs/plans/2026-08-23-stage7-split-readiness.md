# Stage 7 Split Readiness and Physical Extraction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Pass the approved Stage 7 startup and memory conditions without weakening them, then extract the seven R-Term packages into an independently verified repository and switch R-SSH to one immutable external R-Term commit.

**Architecture:** A versioned fail-closed gate consumes raw fixed-runner evidence and advances through attribution, product-memory, cross-platform, extraction, and dual-source states. Gate 0 proves shared/lazy font ownership, exact GPU-stage stopping, and real immutable Git consumption before production behavior changes. Physical history extraction runs only in disposable clones; R-SSH keeps recoverable local packages until the external source, rollback, package, functional, and release rehearsals are green.

**Tech Stack:** Rust 1.89/Rust 2024, WGPU 30, cosmic-text 0.19/fontdb 0.23, Winit 0.30, Serde JSON, Python 3 standard library, PowerShell 7, POSIX shell on Linux/macOS, Git/filter-repo, Cargo locked workspaces, GitHub Actions protected runners.

---

## Execution invariants

- Start implementation from the committed tip of `codex/stage7-split-readiness-plan` (which contains approved design commit `b293fffb` and this plan), or from a fresh branch/worktree at that exact plan tip. Never start directly from `main` or from a commit that omits the approved design/plan.
- Preserve `docs/plans/2026-08-23-stage7-split-readiness-design.md` as the approved design baseline.
- For every new local Windows execution shell, run this bootstrap before any Cargo command; scripts/checkpoints must repeat and verify the same contract rather than relying on inherited state:

```powershell
$stage7CargoTarget = 'L:\rssh-targets\stage7-split-readiness'
$stage7Temp = 'L:\rssh-targets\tmp\stage7-split-readiness'
New-Item -ItemType Directory -Force -Path $stage7CargoTarget, $stage7Temp | Out-Null
$env:CARGO_TARGET_DIR = $stage7CargoTarget
$env:TEMP = $stage7Temp
$env:TMP = $stage7Temp
$resolvedCargoTarget = [IO.Path]::GetFullPath(((cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json).target_directory))
if ($resolvedCargoTarget -ne [IO.Path]::GetFullPath($stage7CargoTarget)) { throw "unexpected Cargo target: $resolvedCargoTarget" }
```

  Do not repopulate `E:\project\R-SSH\target` with build/release artifacts. Every Windows runner accepts or derives this explicit target and fails if it resolves under the R-SSH checkout. Protected Linux/macOS jobs use their own bounded job-local target directories and record them in the runner fingerprint.
- Keep the Stage 7 state NO-GO until the validator derives a later state from immutable evidence.
- Do not change normal production backend selection in this plan. Any such change requires a separate approved design.
- Do not create or push `https://github.com/lcxinc/R-Term.git`, update a remote default branch, or delete local R-Term packages without the explicit checkpoints below.
- Never run history filtering against the original R-SSH checkout or object database.

### Task 1: Freeze the fail-closed Stage 7 gate contract

**Files:**
- Create: `scripts/ci/stage7-split-contract.json`
- Create: `scripts/ci/stage7-evidence-manifest.schema.json`
- Create: `scripts/ci/assemble-stage7-evidence.py`
- Create: `scripts/ci/check-stage7-split-gate.py`
- Create: `scripts/ci/tests/test_check_stage7_split_gate.py`
- Modify: `crates/rssh-app/tests/performance_scorecard_contract.rs`

**Step 1: Write the failing schema tests**

Require schema `rssh.stage7-split-contract/v1`, initial state `blocked`, full immutable refs, approved metrics, and these exact Windows product gates:

```json
{
  "first_present_p50_ms_max": 400,
  "first_present_p95_ms_max": 500,
  "first_frame_private_bytes_p95_max": 57671680,
  "first_frame_private_bytes_max_exclusive": 62914560,
  "empty_window_private_working_set_p95_max": 47185920,
  "ssh1_private_working_set_p95_max": 62914560,
  "gpu_steady_bytes_max": 268435456,
  "relative_regression_ratio_max": 1.05
}
```

Require `auto` as the only required Windows product backend, `dx12/vulkan/gl` as diagnostic-only probes, `final_renderer=gpu` for empty-window and SSH1, and `connection_state=connected` for SSH1. Require 5 warmups, 30 measured cold processes, nearest-rank cross-process p50/p95, raw maximum, and a 60 second timeout, but freeze scenario-specific sampling:

- startup/first-frame: each `--benchmark-startup` process contributes the single Private-Bytes value carried by its `first_frame_memory` marker and exits immediately after the CPU bootstrap present; p50/p95 and max are computed over those 30 marker values, with no 5-second stabilization or later residence sampling;
- attribution, font proof, empty-window, SSH1, and applicable GPU-steady residence probes: after the owner-specific ready marker, stabilize 5,000 ms, collect ten child-process samples at 100 ms intervals, take one nearest-rank median representative per process, then compute nearest-rank p50/p95 over the 30 representatives and maximum over all raw samples.

Reject a startup record fabricated from residence samples and reject a residence record flattened across all 300 points.

Pin the initial same-machine R-SSH product LKG field `lkg_rssh_ref` to full SHA `21dd01b3d73dd9c9241ac10e7a25d92cb2bcfea6`. This is distinct from the first post-filter R-Term LKG established later through the source-to-filtered map.

Freeze evidence manifest schema `rssh.stage7-evidence-manifest/v1`. Every entry contains a closed `artifact_type`, relative path beneath the manifest directory, SHA-256, producing command, full source SHA, binary hashes where applicable, runner fingerprint hash where applicable, platform, and run identifier. Reject absolute/escaping paths, duplicate artifact types where the contract requires one, unreferenced files, summaries without raw children, identity drift, and hashes supplied only on the command line.

Bind state advancement to these artifact sets:

- `attribution-ready`: raw/aggregate font ownership proof, raw/aggregate eight-stage stopping matrix, runner and font-catalog fingerprints, immutable local two-bare-Git source proof, and their deterministic test results.
- `windows-memory-go`: everything above plus one locked Windows release-build provenance record; raw first-present, first-frame, empty-window, SSH1, and GPU-steady records; package smoke and native ten-frame results; loopback native SSH results covering host-key unknown/changed, secret masking, resize, cancel, disconnect, and reconnect; a zero-hit secret scan over stdout/stderr/markers/JSON/session log/snapshot; and the exact deterministic test suite result.
- `cross-platform-go`: the complete Windows set plus Linux PSS and macOS physical-footprint raw records, same-platform immutable-LKG comparisons, native window/PTY/SSH/package results, and protected-job provenance from both platforms.
- `extraction-ready`, `dual-source-verified`, and `split-complete`: the release-contract, history-map, full-history scan, SBOM/license, standalone CI, dual-repository Task 10, external-source, rollback, protected-release, and deletion evidence named in Tasks 13-19. Later states include every earlier state's manifest entries.

Add negative fixtures for every required artifact omitted at every state, bad manifest containment/hash, missing source/binary/runner identity, mutable refs, backend mismatch, CPU fallback in empty-window/SSH1/GPU-steady scenarios that require GPU, unexpected GPU final/backend identity in startup, flattened 300-point residence statistics, stale binary hashes, runner-fingerprint mismatch, threshold violations, and a requested state later than the evidence permits.

**Step 2: Run the tests and observe RED**

Run:

```powershell
python -m unittest scripts.ci.tests.test_check_stage7_split_gate -v
cargo test --locked -p rssh-app --test performance_scorecard_contract stage7_split -j1
```

Expected: FAIL because the contract and validator do not exist.

**Step 3: Implement the contract and pure validator core**

Model ordered states and fail closed:

```python
STATES = (
    "blocked",
    "attribution-ready",
    "windows-memory-go",
    "cross-platform-go",
    "extraction-ready",
    "dual-source-verified",
    "split-complete",
)

def nearest_rank(values, percentile):
    ordered = sorted(values)
    index = max(1, math.ceil(len(ordered) * percentile)) - 1
    return ordered[index]

def process_representative(samples):
    return nearest_rank(samples, 0.50)
```

The CLI is frozen as:

```text
check-stage7-split-gate.py
  --contract <stage7-split-contract.json>
  --requested-state <state>
  [--evidence-manifest <stage7-evidence-manifest.json>]
```

`blocked` accepts no manifest and reports the existing NO-GO. Every later state requires exactly one manifest conforming to the frozen schema. The validator verifies containment and SHA-256 before parsing, recomputes every statistic from raw data, validates the per-state artifact set and common identities, prints one JSON decision, and exits nonzero unless the requested state is proven. It must never infer GO from an evidence summary without raw records.

Every evidence-producing runner writes an atomic `artifact-manifest-fragment.json`. Freeze the assembler CLI as `assemble-stage7-evidence.py --contract <path> --requested-state <state> --evidence-root <bounded-root> [--prior-manifest <relative-path>] --fragment <relative-path>... --output <relative-path>`. It verifies each fragment and source file, rejects escaping/duplicate inputs, sorts entries deterministically, and writes the one manifest consumed by the validator. A transition after `attribution-ready` requires exactly the immediately preceding state manifest plus new state artifacts; the validator recursively validates the full predecessor chain and keeps each certification epoch's source/binary/runner identity separate. It rejects a skipped/reordered state, a predecessor manifest outside the bounded root, or a current commit not descended from the prior certified commit. Unit tests assemble real temporary files and then mutate, remove, duplicate, reorder, or escape one input to prove fail-closed behavior.

**Step 4: Verify GREEN and compatibility**

Run the two focused commands from Step 2 plus:

```powershell
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state blocked
```

Expected: tests pass and the validator reports `blocked` with the existing Stage 7 NO-GO reason.

**Step 5: Commit**

```powershell
git add scripts/ci/stage7-split-contract.json scripts/ci/stage7-evidence-manifest.schema.json scripts/ci/assemble-stage7-evidence.py scripts/ci/check-stage7-split-gate.py scripts/ci/tests/test_check_stage7_split_gate.py crates/rssh-app/tests/performance_scorecard_contract.rs
git commit -m "feat(ci): freeze the Stage 7 split gate"
```

### Task 2: Share active font allocations and add transactional batches

**Files:**
- Modify: `crates/rterm-fonts/Cargo.toml`
- Modify: `crates/rterm-fonts/src/catalog.rs`
- Modify: `crates/rterm-fonts/src/lib.rs`
- Modify: `crates/rterm-fonts/tests/shaping.rs`
- Modify: `crates/rterm-fonts/tests/caches.rs`

**Step 1: Write failing ownership and compatibility tests**

Add tests proving:

- `FontSource::new`, `bytes`, `from_file`, `load_source`, and `load_file` remain source compatible;
- a fixed `FontSource` golden value has byte-for-byte identical `Debug` output before and after the internal allocation change, and existing read/catalog error `Display` plus `source()` behavior is unchanged;
- one active source contributes its bytes once to `CatalogMemoryMetrics::retained_source_bytes`;
- fontdb and the catalog receive the same shared allocation rather than a `to_vec()` copy;
- `load_sources` performs one transactional rebuild and one generation increment;
- any invalid source rejects the entire batch without changing generation, fingerprint, face records, or memory metrics;
- the ordered catalog fingerprint changes when active source order changes; and
- a diagnostic-only copied mode retains the old double-ownership behavior for Gate 0 comparison.

**Step 2: Run focused tests and observe RED**

```powershell
cargo test --locked -p rterm-fonts --test shaping shared_source -j1
cargo test --locked -p rterm-fonts --test caches transactional_batch -j1
```

Expected: FAIL because shared ownership, batch load, and metrics are missing.

**Step 3: Implement selectable copied/shared ownership without changing production**

Use one owned buffer behind an `Arc` that also implements `AsRef<[u8]>`:

```rust
#[derive(Debug, Eq, PartialEq)]
struct FontBytes(Box<[u8]>);

impl AsRef<[u8]> for FontBytes {
    fn as_ref(&self) -> &[u8] { &self.0 }
}

pub struct FontSource {
    pub label: String,
    bytes: Arc<FontBytes>,
}
```

Pass `Arc::clone(&source.bytes)` to `fontdb::Source::Binary` as the trait-object allocation for the shared policy. Add an internal copied/shared build policy and expose non-default diagnostic constructors under `diagnostic-tools`. Keep the existing `FontCatalog::from_sources` and production call sites on copied ownership until Gate 0 passes and Task 10 promotes shared ownership; this task must not change packaged behavior.

Replace derived `Debug` for `FontSource` with a compatibility implementation that renders the existing `label` and byte-slice field shape; the private `FontBytes` wrapper must never appear in public debug output. Keep existing error variants and formatting unchanged.

Add:

```rust
pub struct CatalogMemoryMetrics {
    pub retained_source_bytes: usize,
    pub active_source_count: usize,
    pub catalog_builds: u64,
}
```

Implement `load_sources` by building a complete candidate first and committing sources, records, `FontSystem`, ordered fingerprint, generation, and metrics together.

**Step 4: Verify the full font crate**

```powershell
cargo test --locked -p rterm-fonts --all-targets -j1
cargo clippy --locked -p rterm-fonts --all-targets -- -D warnings
```

Expected: PASS with no public compatibility regression.

**Step 5: Commit**

```powershell
git add crates/rterm-fonts
git commit -m "feat(fonts): add shared catalog ownership"
```

### Task 3: Add an app-owned lazy platform-font repository

**Files:**
- Create: `crates/rssh-app/src/platform_fonts.rs`
- Modify: `crates/rssh-app/src/main.rs`
- Modify: `crates/rssh-app/src/window_gpu.rs`
- Modify: `crates/rterm-render-wgpu/src/gpu/text.rs`
- Modify: `crates/rterm-render-wgpu/src/gpu/render_graph.rs`
- Modify: `crates/rterm-render-wgpu/tests/gpu_text.rs`
- Modify: `crates/rssh-app/src/window_compat_tests/part02_tests.rs`
- Modify: `crates/rssh-app/tests/fixtures/task23_app_test_manifest.txt`

**Step 1: Write failing repository and whole-frame tests**

Cover:

- the Windows candidate index retains paths/metadata but zero font-file bytes;
- ASCII preflight activates only the primary/emergency set;
- CJK and emoji preflight activates the minimum matching sources once;
- Arabic, Devanagari, and Hebrew preflight each activate the minimum matching source once and shape without tofu when the fixed fixture supplies coverage;
- a missing-font fixture takes the stable emergency/missing-glyph path without repeated activation or an infinite frame restart;
- activation computes an ordered catalog fingerprint and generation atomically;
- one presented frame never mixes catalog generations;
- a late unresolved script returns `CatalogExpanded` and restarts the whole frame once;
- device loss rebuilds from the same app-owned repository without multiplying retained bytes; and
- Stage 7 diagnostics expose counts/digests, never raw paths.

**Step 2: Observe RED**

```powershell
cargo test --locked -p rssh-app platform_fonts -j1
cargo test --locked -p rterm-render-wgpu --test gpu_text catalog_generation -j1
```

Expected: FAIL because the repository and preflight contract are absent.

**Step 3: Implement the bounded repository**

Model indexed and active entries separately:

```rust
pub(crate) struct PlatformFontRepository {
    policy_version: u32,
    indexed: Vec<IndexedFont>,
    active: BTreeMap<FontKey, FontSource>,
    activation_order: Vec<FontKey>,
}

pub(crate) struct FrameFontPlan {
    required: Vec<FontKey>,
    catalog_fingerprint: [u8; 32],
}
```

Index only the reviewed platform candidate list. Read a selected file once during preflight, convert it to the shared Binary source from Task 2, and batch-activate before `GpuText::ensure_scope` or row shaping. Keep the repository owned above `WindowGpu` so renderer/device recreation borrows the same active set.

`BTreeMap` provides keyed lookup only. Compute the catalog fingerprint and batch source order from `activation_order`; never substitute key sort order for activation order.

**Step 4: Replace repeated platform catalog rebuilds**

Route catalog construction through an explicit private mode. `CurrentCopied` preserves the existing `for ... catalog.load_source(...)` behavior exactly; `SharedAll` collects all sources and builds once; `Lazy` builds the initial active set once and retains only the index for inactive sources.

Keep all three selectable modes private to diagnostics in this task. The normal product path remains `CurrentCopied` until Gate 0 passes and Task 10 promotes `Lazy`; only then remove repeated production rebuilds.

**Step 5: Verify scripts, recovery, and rendering**

```powershell
cargo test --locked -p rterm-fonts --all-targets -j1
cargo test --locked -p rterm-render-wgpu --test gpu_text -j1
cargo test --locked -p rssh-app platform_fonts -j1
cargo test --locked -p rssh-app deferred_gpu -j1
```

Expected: PASS; CJK/emoji tests contain no tofu and recovery retains one active source set.

**Step 6: Commit**

```powershell
git add crates/rssh-app/src/platform_fonts.rs crates/rssh-app/src/main.rs crates/rssh-app/src/window_gpu.rs crates/rssh-app/src/window_compat_tests/part02_tests.rs crates/rssh-app/tests/fixtures/task23_app_test_manifest.txt crates/rterm-render-wgpu crates/rterm-fonts
git commit -m "feat(app): load platform fallback fonts on demand"
```

### Task 4: Add the private font-proof diagnostic mode

**Files:**
- Modify: `crates/rssh-diagnostics/src/schema.rs`
- Modify: `crates/rssh-diagnostics/src/launcher.rs`
- Modify: `crates/rssh-diagnostics/src/production.rs`
- Modify: `crates/rssh-diagnostics/src/lib.rs`
- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `crates/rssh-app/src/cli.rs`
- Modify: `crates/rssh-app/src/window_gpu.rs`
- Modify: `crates/rssh-app/src/window_parts/diagnostics.rs`
- Create: `scripts/ci/run-stage7-font-proof.ps1`
- Create: `scripts/ci/collect-stage7-runner-fingerprint.ps1`
- Modify: `crates/rssh-app/tests/gpu_backend_memory_matrix_behavior.rs`
- Modify: `crates/rssh-app/tests/fixtures/task23_app_test_manifest.txt`
- Modify: `scripts/ci/tests/test_check_stage7_split_gate.py`

**Step 1: Write failing CLI/schema/runner tests**

Add `DiagnosticFontMode::{CurrentCopied, SharedAll, Lazy}` and `DiagnosticFontSpecimen::{Ascii, Cjk, Emoji}`. Require `--font-mode` and `--font-specimen` only on the private `diagnostic-gui`/launcher path, reject them with CPU, and omit both fields from default JSON for wire compatibility.

Require the PowerShell proof runner to interleave modes by round, use production `auto`, verify one actual backend, perform 5+30 cold runs per mode, retain all 900 raw memory samples, compute one median per process, and enforce the 64 MiB then 32 MiB reductions.

Define each mode's comparison value as nearest-rank p50 over its 30 per-process medians. Require `p50(current) - p50(shared) >= 64 MiB` and `p50(shared) - p50(lazy) >= 32 MiB`; never compare flattened raw samples or only one selected process.

**Step 2: Observe RED**

```powershell
cargo test --locked -p rssh-diagnostics font_mode -j1
cargo test --locked -p rssh-app font_mode -j1
cargo test --locked -p rssh-app --test gpu_backend_memory_matrix_behavior font_proof -j1
```

Expected: FAIL because the mode and script do not exist.

**Step 3: Implement private forwarding and safe metrics**

Forward the mode launcher -> `diagnostic-gui` -> app override -> font repository. Change the existing app feature to `diagnostic-tools = ["rssh-fonts/diagnostic-tools"]`; the existing developer-full/default graph may continue to include diagnostics, but `--no-default-features --features production-gui` must not enable it. Add compile-contract tests proving packaged builds cannot name the copied/shared proof constructors while diagnostic builds can. Add a structured resource summary containing retained source bytes, indexed/active counts, catalog builds, generation, and irreversible fingerprints. Reject raw path keys in marker tests.

**Step 4: Implement the proof runner**

Write ASCII memory records atomically beneath `raw/current`, `raw/shared`, and `raw/lazy`; include source/binary/runner hashes and the exact sampling configuration. Run separate bounded CJK and emoji activation specimens for every candidate mode and record first activation latency, tofu count, frame generation, and recovery retained bytes. Fail immediately on a product-backend mismatch, mode/specimen fallback, missing resource counter, dirty binary, reduction below the approved values, tofu, mixed frame generation, or recovery duplication. Activation latency remains report-only.

Write an atomic `artifact-manifest-fragment.json` covering every raw record, aggregate, functional specimen result, binary/source identity, runner fingerprint, and catalog fingerprint; the fragment itself is not a substitute for its raw children.

**Step 5: Run deterministic checks**

```powershell
cargo test --locked -p rssh-diagnostics --all-targets -j1
cargo test --locked -p rssh-app --test gpu_backend_memory_matrix_behavior -j1
pwsh -NoProfile -File scripts/ci/run-stage7-font-proof.ps1 -WhatIf
pwsh -NoProfile -File scripts/ci/update-task23-test-manifest.ps1
```

Expected: all tests pass; `-WhatIf` prints the exact 5+30 interleaved schedule without opening a window.

**Step 6: Commit**

```powershell
git add crates/rssh-diagnostics crates/rssh-app scripts/ci/run-stage7-font-proof.ps1 scripts/ci/tests/test_check_stage7_split_gate.py
git commit -m "test(perf): add the Stage 7 font ownership proof"
```

### Task 5: Make GPU attribution stages real and stoppable

**Files:**
- Modify: `crates/rterm-render-wgpu/src/gpu/context.rs`
- Modify: `crates/rterm-render-wgpu/src/gpu/mod.rs`
- Modify: `crates/rterm-render-wgpu/src/gpu/metrics.rs`
- Modify: `crates/rterm-render-wgpu/src/gpu/render_graph.rs`
- Modify: `crates/rterm-render-wgpu/src/gpu/text.rs`
- Modify: `crates/rterm-render-wgpu/src/lib.rs`
- Modify: `crates/rterm-render-wgpu/tests/gpu.rs`
- Modify: `crates/rssh-app/src/window_gpu.rs`
- Create: `crates/rssh-app/src/stage7_attribution.rs`
- Modify: `crates/rssh-app/src/main.rs`
- Modify: `crates/rssh-app/src/window_compat_tests/part02_tests.rs`
- Modify: `crates/rssh-app/tests/fixtures/task23_app_test_manifest.txt`

**Step 1: Write failing lifecycle tests**

Add named tests `holds_each_of_eight_stages_without_later_work`, `stage_resource_matrix_is_fail_closed`, `attribution_never_starts_product_services`, and `production_composition_is_unchanged`. Require these exact cumulative ownership boundaries:

| Stage | Work completed before hold | Work that must still be absent |
|---|---|---|
| `cpu-window` | create `WindowBootstrapSurface`, draw and present the non-empty CPU bootstrap frame | every WGPU, config, font-index, snapshot, PTY, and SSH resource |
| `instance-surface` | retain the CPU fallback and create only WGPU instance plus surface | adapter/device/queue and every later resource |
| `adapter-device` | request exactly one adapter, device, and queue | surface configure/acquire, layer pipelines, fonts, and later work |
| `configured-surface-clear` | configure the surface and acquire/clear/present exactly one frame | layer pipelines, font/text, platform index, and product snapshot |
| `layer-pipelines` | create base layer pipelines/layouts and their explicit base buffers | fixture catalog/text renderer, platform index, and product snapshot |
| `fixture-font-text` | create the fixed embedded fixture catalog/text renderer and present the fixture text once | platform font index, production font activation, and product snapshot |
| `platform-font-index` | build the redacted candidate metadata index while retaining zero inactive font-file bytes | production snapshot, on-demand platform activation, PTY, SSH, and config watcher |
| `full-frame` | run the production lazy-font preflight and complete one full GPU frame from the diagnostic empty-window snapshot | config watcher, PTY, SSH, or any post-ready background task |

Prove each state can be held for the 5-second sampling window, drops safely, and still supports the normal one-call production path. For each row, assert exact create/materialization counts, all forbidden counters remain zero, and no scheduled task crosses the hold.

**Step 2: Observe RED**

```powershell
cargo test --locked -p rterm-render-wgpu --test gpu attribution_stage -j1
cargo test --locked -p rssh-app exact_gpu_stop_stage -j1
```

Expected: FAIL because `finish_windowed` currently selects the adapter/device and configures the surface in one operation.

**Step 3: Split initialization without changing production behavior**

Introduce R-Term-owned initialization states similar to:

```rust
pub enum GpuInitializationStage {
    InstanceSurface,
    AdapterDevice,
    ConfiguredSurfaceClear,
    LayerPipelines,
}

pub struct WindowedGpuDevice { /* instance, surface, adapter, device, queue */ }
```

Add `select_device`, `configure_surface`, and `present_clear_once`. Keep `finish_windowed` as a compatibility composition of those steps. A diagnostic stage controller acknowledges and holds an exact state; the normal production controller always runs through full frame.

Keep the eight-value `GpuAttributionStage` in R-SSH diagnostics/app code and map its GPU subset to `GpuInitializationStage`; R-Term must not depend on `rssh-diagnostics`.

The R-SSH controller owns `CpuWindow`, `FixtureFontText`, `PlatformFontIndex`, and `FullFrame` composition. It must not fake a hold by completing the full initializer and delaying acknowledgement. The controller disables deferred configuration, watcher, PTY, and SSH scheduling before it starts any requested stage.

**Step 4: Prove no later work crosses the hold**

Instrument a complete project-owned snapshot without estimating driver memory. At every stage, emit all fields (zero where not owned yet): CPU staging bytes; instance/surface/adapter/device/configure counts; pipeline/layout/materialization counts; retained font bytes; indexed and active font counts; catalog builds/generation; glyph-atlas bytes; raster-cache bytes; image-texture bytes; snapshot bytes; instance-buffer bytes; upload-buffer bytes; total explicitly allocated buffer and texture bytes; base and cursor text-renderer materialization counts. Task 6 exposes this as versioned diagnostics JSON.

Use counters/fakes to assert no config, font, layer, PTY, or SSH task starts after a stage hold. Require actual backend/adapter fields to be absent before `AdapterDevice` and present thereafter. Require the contract's per-stage allowlist to name every field that may be nonzero; an unknown counter, a missing required counter, or a nonzero later-stage counter fails closed.

**Step 5: Verify GPU and app recovery paths**

```powershell
cargo test --locked -p rterm-render-wgpu --all-targets -j1
cargo test --locked -p rssh-app exact_gpu_stop_stage -j1
cargo test --locked -p rssh-app device_loss -j1
pwsh -NoProfile -File scripts/ci/update-task23-test-manifest.ps1
cargo test --locked -p rssh-app --test task23_test_manifest -j1
```

Expected: PASS with the production path semantically unchanged.

**Step 6: Commit**

```powershell
git add crates/rterm-render-wgpu crates/rssh-app/src/stage7_attribution.rs crates/rssh-app/src/window_gpu.rs crates/rssh-app/src/main.rs crates/rssh-app/src/window_compat_tests/part02_tests.rs crates/rssh-app/tests/fixtures/task23_app_test_manifest.txt
git commit -m "refactor(gpu): expose stoppable attribution stages"
```

### Task 6: Extend diagnostic markers and JSON for exact stages

**Files:**
- Modify: `crates/rssh-diagnostics/src/schema.rs`
- Modify: `crates/rssh-diagnostics/src/marker.rs`
- Modify: `crates/rssh-diagnostics/src/launcher.rs`
- Modify: `crates/rssh-diagnostics/src/production.rs`
- Modify: `crates/rssh-diagnostics/tests/schema_v2.rs`
- Modify: `crates/rssh-diagnostics/tests/marker_protocol.rs`
- Modify: `crates/rssh-diagnostics/tests/launcher_state.rs`
- Modify: `crates/rssh-app/src/cli.rs`
- Modify: `crates/rssh-app/src/window_parts/diagnostics.rs`
- Modify: `crates/rssh-app/tests/diagnostics_gui.rs`

**Step 1: Write failing protocol tests**

Add `--attribution-stage` to private diagnostics. Add `ProjectOwnedResourceMetricsV1` with the complete fields instrumented in Task 5 and a closed `resource_summary_schema = "rssh.project-owned-resources/v1"` discriminator. Require exactly one `attribution_stage_ready` marker with the requested stage, every schema field present, the contract's required/nonzero matrix satisfied for all eight stages, and no marker or counter from a later stage. Preserve legacy JSON byte shape when no stage is requested.

**Step 2: Observe RED**

```powershell
cargo test --locked -p rssh-diagnostics attribution_stage -j1
cargo test --locked -p rssh-app --test diagnostics_gui attribution_stage -j1
```

Expected: FAIL because the CLI and marker kind are missing.

**Step 3: Implement typed schema and marker collection**

Serialize the enum as kebab-case. Store requested/final stage and a structured resource summary in final launcher JSON. Permit backend/adapter identity only from `adapter-device` onward. Reject duplicate ready markers, out-of-order markers, any later-stage marker, and unknown resource keys.

Add one fixture per stage plus negative fixtures for a missing required counter, a nonzero later-stage counter, a fabricated adapter identity before `adapter-device`, absent identity at/after that stage, and an otherwise valid record followed by config/PTY/SSH activity. The ready marker for `fixture-font-text`, `platform-font-index`, and `full-frame` must be generated by the real owners described in Task 5, not inferred from elapsed time.

**Step 4: Verify wire compatibility and failure behavior**

```powershell
cargo test --locked -p rssh-diagnostics --all-targets -j1
cargo test --locked -p rssh-app --test diagnostics_gui -j1
```

Expected: PASS; existing default fixtures remain byte-for-byte compatible where fields are omitted.

**Step 5: Commit**

```powershell
git add crates/rssh-diagnostics crates/rssh-app/src/cli.rs crates/rssh-app/src/window_parts/diagnostics.rs crates/rssh-app/tests/diagnostics_gui.rs
git commit -m "feat(diagnostics): report exact GPU attribution stages"
```

### Task 7: Build and validate the cumulative stage matrix

**Files:**
- Create: `scripts/ci/run-stage7-attribution-matrix.ps1`
- Create: `scripts/ci/run-stage7-attribution-deterministic-tests.ps1`
- Modify: `scripts/ci/collect-stage7-runner-fingerprint.ps1`
- Modify: `scripts/ci/run-gpu-backend-memory-matrix.ps1`
- Modify: `crates/rssh-app/tests/gpu_backend_memory_matrix_behavior.rs`
- Modify: `scripts/ci/tests/test_check_stage7_split_gate.py`
- Modify: `.github/workflows/release.yml`

**Step 1: Write failing static and aggregation tests**

Require eight ordered stages, `auto` product plus DX12/Vulkan/GL diagnostics, round-interleaving, fixed runner fingerprint fields, 5+30 cold runs, per-process medians, nearest-rank p50/p95 across 30 representatives, maximum from raw samples, atomic raw/aggregate files, and certification-ineligible output for any identity drift.

For every one of the eight stages, require the exact owner-produced ready marker and validate the complete `ProjectOwnedResourceMetricsV1` row from Task 5. Add named negative fixtures `rejects_fixture_stage_without_fixture_text`, `rejects_platform_index_with_retained_inactive_bytes`, and `rejects_full_frame_without_present`; a timed delay or generic scenario-ready marker cannot stand in for stage completion.

**Step 2: Observe RED**

```powershell
cargo test --locked -p rssh-app --test gpu_backend_memory_matrix_behavior stage7_attribution -j1
python -m unittest scripts.ci.tests.test_check_stage7_split_gate.Stage7SplitGateTests.test_attribution_matrix -v
```

Expected: FAIL because the stage matrix is absent and the old runner flattens 300 points.

**Step 3: Implement the matrix runner**

Build once with `--locked --release`, hash the source and both executables, then run every stage/backend pair from fresh processes. After the ready marker, stabilize 5,000 ms and take ten 100 ms samples. Write one raw file per process and an aggregate containing representatives, raw maxima, identities, failure classifications, and report-only adjacent-stage deltas.

Write an atomic `artifact-manifest-fragment.json` that binds all raw stage records, the aggregate, source/binary hashes, runner fingerprint, resource-summary schema, and actual backend/adapter identities.

Add a separate fail-closed deterministic proof runner for the approved Rust matrix behavior tests and Python gate attribution test. It binds the same immutable source, release binary hashes, and runner fingerprint, then emits the required `attribution-deterministic-tests` result and its own fragment without claiming hardware measurements.

Reuse the machine cohort identity produced by Task 4. The font fragment remains the sole owner of the singleton artifact; the stage fragment references the same fingerprint identity and must not emit a second `runner-fingerprint` singleton.

**Step 4: Wire a protected manual/default-branch job**

Hosted PR CI runs only deterministic contract tests. The protected Windows job runs the hardware matrix and uploads raw plus aggregate evidence. Missing DX12/Vulkan/GL support is recorded for diagnostic probes; missing or failed production `auto` is fatal.

**Step 5: Verify without claiming certification**

```powershell
pwsh -NoProfile -File scripts/ci/run-stage7-attribution-matrix.ps1 -WhatIf
pwsh -NoProfile -File scripts/ci/run-stage7-attribution-deterministic-tests.ps1 -WhatIf
cargo test --locked -p rssh-app --test gpu_backend_memory_matrix_behavior -j1
python -m unittest scripts.ci.tests.test_check_stage7_split_gate -v
```

Expected: PASS; `-WhatIf` shows the complete interleaved schedule.

**Step 6: Commit**

```powershell
git add scripts/ci/run-stage7-attribution-matrix.ps1 scripts/ci/run-stage7-attribution-deterministic-tests.ps1 scripts/ci/collect-stage7-runner-fingerprint.ps1 scripts/ci/run-gpu-backend-memory-matrix.ps1 scripts/ci/tests/test_check_stage7_split_gate.py crates/rssh-app/tests/gpu_backend_memory_matrix_behavior.rs .github/workflows/release.yml
git commit -m "test(perf): add cumulative GPU attribution evidence"
```

### Task 8: Prove immutable external Git consumption locally

**Files:**
- Create: `scripts/ci/prove-rterm-external-source.py`
- Create: `scripts/ci/tests/test_prove_rterm_external_source.py`
- Create: `scripts/ci/rterm-external-source-proof.json`
- Modify: `scripts/ci/tests/test_check_stage7_split_gate.py`

**Step 1: Write failing disposable-repository tests**

Using temporary real Git repositories, cover path containment, two distinct bare remotes, full SHA enforcement, committed lockfiles, dirty-worktree detection, seven-package source equality, rejection of `path+file` sources, consumer-root vendor resolution, and one-commit rollback. Support two mutually exclusive modes: `--synthesize` for the pre-R1 Gate 0 topology proof, and `--candidate-repo <path> --candidate-ref <full-sha>` for later proof of the canonical extracted workspace. Reject mixed modes, mutable refs, a dirty candidate, or candidate `HEAD` unequal to the requested SHA. Do not mock Git or Cargo output.

**Step 2: Observe RED**

```powershell
python -m unittest scripts.ci.tests.test_prove_rterm_external_source -v
```

Expected: FAIL because the proof tool is missing.

**Step 3: Implement the local bare-Git proof**

In `--synthesize` mode, the tool creates bounded disposable staging clones and synthesizes a candidate R-Term workspace from contract-owned paths. In canonical mode, it clones the supplied R-Term repository at the supplied full SHA without changing its tree. Both modes commit/push the candidate to one local bare remote, commit a temporary R-SSH source switch to a second bare remote, and run only `--locked` commands. It must not call `cargo generate-lockfile` after the proof commit.

Inspect `cargo metadata --locked` and require all seven packages to share the candidate Git SHA. Require `glyphon` and `gpu-allocator` manifest paths to resolve under the R-SSH consumer vendor root. Revert the source-switch commit and prove path-source restoration.

Write an atomic `artifact-manifest-fragment.json` containing the mode, both bare-repository identities, full candidate/source-switch/rollback SHAs, locked metadata and worktree hashes, vendor resolutions, and all command results. Canonical mode additionally writes `r1_ref` and requires every candidate/metadata identity to equal it; synthesized evidence is never accepted as canonical R1 evidence.

**Step 4: Verify GREEN and cleanup semantics**

```powershell
python -m unittest scripts.ci.tests.test_prove_rterm_external_source -v
python scripts/ci/prove-rterm-external-source.py --contract scripts/ci/rterm-external-source-proof.json --synthesize --output L:\rssh-evidence\stage7-external-source-smoke --keep-on-failure
```

Expected: PASS, two immutable local Git SHAs in evidence, unchanged committed lockfiles, and no retained successful checkout.

**Step 5: Commit**

```powershell
git add scripts/ci/prove-rterm-external-source.py scripts/ci/tests/test_prove_rterm_external_source.py scripts/ci/rterm-external-source-proof.json scripts/ci/tests/test_check_stage7_split_gate.py
git commit -m "test(release): prove immutable R-Term Git consumption"
```

### Task 9: Run Gate 0 and publish its decision

**Files:**
- Create: `docs/performance/stage7-gate0-evidence.md`

**Step 1: Prewarm the shared release artifacts used by the other proofs**

```powershell
$env:CARGO_TARGET_DIR='L:\rssh-targets\stage7-split-readiness'
$env:TEMP='L:\rssh-targets\tmp\stage7-split-readiness'
$env:TMP=$env:TEMP
New-Item -ItemType Directory -Force -Path $env:CARGO_TARGET_DIR, $env:TEMP | Out-Null
cargo build --locked --release -p rssh-app --bin rssh-app --no-default-features --features production-gui,diagnostic-tools
cargo build --locked --release -p rssh-diagnostics --bin rssh-bench-launcher
```

Record the source and executable hashes before running proofs.
The font proof runner performs its own locked release provenance-bound build in Step 2; this prewarm is not the font proof's exact-once build authority.

**Step 2: Run the four Gate 0 proofs**

```powershell
$gate0Root = 'L:\rssh-evidence\stage7-gate0'
pwsh -NoProfile -File scripts/ci/run-stage7-font-proof.ps1 -Profile release -Warmups 5 -MeasuredRounds 30 -OutputDirectory "$gate0Root\font"
pwsh -NoProfile -File scripts/ci/run-stage7-attribution-matrix.ps1 -Profile release -Warmups 5 -Samples 30 -OutputDirectory "$gate0Root\stages" -SkipBuild
pwsh -NoProfile -File scripts/ci/run-stage7-attribution-deterministic-tests.ps1 -OutputDirectory "$gate0Root\tests"
python scripts/ci/prove-rterm-external-source.py --contract scripts/ci/rterm-external-source-proof.json --synthesize --output "$gate0Root\external" --keep-on-failure
```

Expected: every proof exits 0 and retains raw evidence.

**Step 3: Derive the state**

Assemble the four runner fragments and request `attribution-ready` with the frozen interface:

```powershell
$gate0Root = 'L:\rssh-evidence\stage7-gate0'
python scripts/ci/assemble-stage7-evidence.py --contract scripts/ci/stage7-split-contract.json --requested-state attribution-ready --evidence-root $gate0Root --fragment font/artifact-manifest-fragment.json --fragment stages/artifact-manifest-fragment.json --fragment tests/artifact-manifest-fragment.json --fragment external/artifact-manifest-fragment.json --output stage7-evidence-manifest.json
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state attribution-ready --evidence-manifest "$gate0Root\stage7-evidence-manifest.json"
```

Expected: `attribution-ready`. If any proof fails, write the exact NO-GO evidence, stop this plan, and return to design; do not continue to Task 10.

This evidence task does not modify implementation or tests. If a proof exposes a defect, first add a separate RED/GREEN fix commit to the plan execution log, rebuild from that immutable commit, rerun all three Gate 0 proofs from scratch, and only then restart this evidence task.

**Step 4: Independently recompute and document**

Recompute every raw-file count, hash, representative, reduction, stage identity, deterministic-suite result, source mapping, vendor path, and dirty-state assertion in a separate read-only command. Record host fingerprint and report-only first fallback latency without machine-unique paths.

**Step 5: Verify and commit**

```powershell
git add docs/performance/stage7-gate0-evidence.md
git diff --cached --check
cargo fmt --all -- --check
cargo test --locked -p rterm-fonts --all-targets -j1
cargo test --locked -p rssh-diagnostics --all-targets -j1
cargo test --locked -p rssh-app --test gpu_backend_memory_matrix_behavior -j1
git commit -m "docs: record Stage 7 Gate 0 evidence"
```

### Task 10: Promote lazy fonts to the production GPU path

**Files:**
- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `crates/rterm-fonts/Cargo.toml`
- Modify: `crates/rssh-app/src/window_gpu.rs`
- Modify: `crates/rssh-app/src/platform_fonts.rs`
- Modify: `crates/rterm-fonts/src/catalog.rs`
- Modify: `crates/rterm-fonts/src/lib.rs`
- Modify: `crates/rssh-app/tests/stage5_startup_contract.rs`
- Modify: `crates/rssh-app/tests/package_release_contract.rs`
- Modify: `crates/rssh-app/tests/fixtures/task23_app_test_manifest.txt`

**Step 1: Write failing production-feature tests**

Require `production-gui` to select lazy/shared font ownership, make the normal `FontCatalog::from_sources` path shared, keep copied/all-font code behind `diagnostic-tools`, omit raw font paths from markers, and preserve deferred initialization after first present. Require the packaged feature graph not to enable the legacy proof mode.

Before promotion, add named production fixtures/tests for ASCII, CJK, emoji, Arabic, Devanagari, Hebrew, and missing-font. Require the six covered scripts to shape/render without tofu using the fixed licensed fixtures, require the missing-font case to produce the stable emergency glyph exactly once, and require every frame to use one catalog generation with at most one whole-frame restart.

**Step 2: Observe RED**

```powershell
cargo test --locked -p rssh-app --test stage5_startup_contract production_fonts -j1
cargo test --locked -p rssh-app --test package_release_contract diagnostic_font_mode -j1
```

Expected: FAIL until the normal path selects lazy mode and feature isolation is explicit.

**Step 3: Promote the proven mode**

Make lazy/shared the normal `auto` GPU path. Keep `current/shared/lazy` selection private to diagnostics. Preserve CPU bootstrap, full-config deferral, and GPU fallback ordering. Make device recovery reuse the active app repository rather than re-indexing or re-reading every system font.

**Step 4: Run functional and bounded native checks**

```powershell
cargo test --locked -p rterm-fonts --all-targets -j1
cargo test --locked -p rterm-render-wgpu --all-targets -j1
cargo test --locked -p rssh-app platform_fonts -j1
cargo test --locked -p rssh-app --test stage5_startup_contract -j1
cargo test --locked -p rssh-app --test package_release_contract -j1
pwsh -NoProfile -File scripts/ci/update-task23-test-manifest.ps1
cargo test --locked -p rssh-app --test task23_test_manifest -j1
```

Expected: PASS with no product CLI or wire-schema addition.

**Step 5: Commit**

```powershell
git add crates/rssh-app crates/rterm-fonts crates/rterm-render-wgpu
git commit -m "perf(app): activate platform fonts lazily"
```

### Task 11: Lazily create image-only GPU resources

**Files:**
- Modify: `crates/rterm-render-wgpu/src/gpu/render_graph.rs`
- Modify: `crates/rterm-render-wgpu/src/gpu/images.rs`
- Modify: `crates/rterm-render-wgpu/src/gpu/metrics.rs`
- Modify: `crates/rterm-render-wgpu/src/gpu/text.rs`
- Modify: `crates/rterm-render-wgpu/tests/gpu_layers.rs`
- Modify: `crates/rterm-render-wgpu/tests/gpu_text.rs`
- Modify: `crates/rssh-app/src/window_gpu.rs`

**Step 1: Write failing empty/image transition tests**

Add named tests `image_pipeline_materializes_once_on_first_image`, `cursor_renderer_materializes_once_on_first_cursor_foreground`, and `recovery_does_not_duplicate_lazy_resources`. Require a new `GpuLayerRenderer` to own zero image-pipeline/image-texture bytes, an empty snapshot never to create those resources, the first image snapshot to create them once, subsequent frames to reuse them, and creation failure to return a typed error that preserves CPU fallback. Require `GlyphonState` to own no second/cursor renderer until a cursor foreground actually needs it; ordinary text and hidden-cursor frames must not materialize it.

**Step 2: Observe RED**

```powershell
cargo test --locked -p rterm-render-wgpu --test gpu_layers lazy_image_pipeline -j1
cargo test --locked -p rterm-render-wgpu --test gpu_text lazy_cursor_renderer -j1
```

Expected: FAIL because constructors eagerly create the image shader/layout/pipeline and the second cursor renderer.

**Step 3: Extract an on-demand image pipeline owner**

Store `Option<GpuImagePipeline>` and initialize it only when the prepared graph contains images. Store the cursor-specific renderer behind an option and create it only on the first cursor-foreground draw. Count image pipeline state, retained image texture bytes, base/cursor renderer materializations, glyph-atlas/raster-cache bytes, snapshot bytes, instance/upload buffers, and all explicit buffer/texture bytes in the Task 5 metrics. Do not alter quad/text ordering, cursor colors, or image/cache budgets.

**Step 4: Verify visual semantics and fallback**

```powershell
cargo test --locked -p rterm-render-wgpu --test gpu_layers -j1
cargo test --locked -p rterm-render-wgpu --test gpu_text -j1
cargo test --locked -p rssh-app device_loss -j1
```

Expected: PASS; snapshot digests, cursor visuals, layer order, and device-recovery counts remain unchanged, and a recovery never raises either materialization count by more than the one new device-owned instance.

**Step 5: Commit**

```powershell
git add crates/rterm-render-wgpu crates/rssh-app/src/window_gpu.rs
git commit -m "perf(gpu): create image resources on first use"
```

### Task 12: Certify product memory and cross-platform GO

**Files:**
- Create: `docs/performance/stage7-product-gate-evidence.md`
- Create: `scripts/ci/run-stage7-product-gates.ps1`
- Create: `scripts/ci/run-stage7-product-gates.sh`
- Create: `scripts/ci/tests/test_stage7_product_runners.py`
- Modify: `scripts/ci/run-ssh-gui-startup.ps1`
- Modify: `scripts/ci/run-stage0-diagnostics.ps1`
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/ci/stage7-split-contract.json`
- Modify: `crates/rssh-app/tests/performance_scorecard_contract.rs`

**Step 1: Write failing runner and evidence-chain tests**

Add named tests `startup_runner_writes_atomic_raw_output`, `stage0_uses_explicit_target_and_process_representatives`, `product_runner_pins_contract_lkg`, `product_runner_builds_each_checkout_once`, `product_runner_interleaves_candidate_and_lkg`, and `windows_go_requires_functional_and_secret_artifacts`.

Require:

- `run-ssh-gui-startup.ps1` to accept atomic `-OutputPath`, explicit `-ExecutablePath`, and `-SkipBuild`, and to record source SHA, executable hash, runner fingerprint, one first-frame marker value as each process representative, cross-process p50/p95/max, and the unchanged p50/p95/Private-Bytes gates without adding a post-present sampling delay;
- `run-stage0-diagnostics.ps1` to honor `CARGO_TARGET_DIR` or explicit `-AppPath`/`-LauncherPath`, support `-SkipBuild`, retain all raw samples, compute one nearest-rank median per process and then nearest-rank p50/p95 over process representatives, and record source/binary/runner identities;
- the new Stage 7 coordinator to require `-Contract`, read the exact `lkg_rssh_ref` from `scripts/ci/stage7-split-contract.json`, reject `HEAD`/branches/overrides, clone candidate and LKG with `--no-local` into separate object stores, and interleave candidate/LKG and scenario by round;
- each checkout's measured product app to be built exactly once in its own target with `cargo build --locked --release -p rssh-app --no-default-features --features production-gui`; the candidate diagnostic app to be built exactly once in a different target with `--no-default-features --features production-gui,diagnostic-tools`; product and diagnostic executable paths/hashes must differ in role and one flavor may never overwrite or satisfy the other's evidence;
- candidate and LKG startup/first-frame records to request production `auto` but, because `--benchmark-startup` exits immediately after the CPU bootstrap present, require `final_renderer=cpu` with actual GPU backend/adapter fields absent;
- candidate and LKG empty-window PWS, SSH1 PWS, and GPU-steady records to require `final_renderer=gpu`, use the same actual production `auto` backend/adapter, and share the fixed runner fingerprint;
- every ratio for measured latency and memory to be at most `1.05`, and the aggregate's `rollback_ref` to equal exactly `21dd01b3d73dd9c9241ac10e7a25d92cb2bcfea6`; and
- package smoke, native ten-frame, loopback host-key/secret/resize/cancel/disconnect/reconnect tests, and zero-hit secret scans to be hashed inputs to the Windows evidence fragment.

Require `run-stage7-product-gates.sh` to implement the same full-SHA/isolated-checkout/explicit-binary/process-representative/identity/atomic-fragment contract for Linux PSS and macOS physical footprint. It detects the platform, never calls the legacy `run-stage0-diagnostics.sh`, and fails if the requested native metric is unavailable. Add fixtures that prove the legacy shell runner's flattened statistics, hard-coded target path, or missing identities can never satisfy a Stage 7 artifact type.

The historical Stage 6 `run-rterm-release-comparison.ps1` is not a Stage 7 certification input: it defaults to a different LKG and covers too few metrics. The upgraded Stage 0 script remains report-compatible but is also not accepted as a product-GO artifact by itself.

**Step 2: Observe RED**

```powershell
python -m unittest scripts.ci.tests.test_stage7_product_runners -v
cargo test --locked -p rssh-app --test performance_scorecard_contract stage7_product_runner -j1
pwsh -NoProfile -File scripts/ci/run-ssh-gui-startup.ps1 -Profile release -Warmups 0 -Samples 1 -SkipBuild -ExecutablePath L:\missing\rssh-app.exe -OutputPath L:\missing\startup.json
```

Expected: the unit/static tests fail because the new interfaces and coordinator do not exist; the smoke command must fail on the missing executable only after PowerShell has accepted `-OutputPath` once the implementation is green.

**Step 3: Implement, verify, and commit gate tooling before hardware CI**

Implement atomic temp-file-plus-rename output, exact identity fields, the Task 1 scenario-specific startup versus residence aggregation, the bounded coordinator, explicit product/diagnostic target separation, and protected Windows/Linux/macOS workflow entries. The coordinator calls the startup runner with the packaged product executable via `-SkipBuild -ExecutablePath ... -OutputPath ...`; product empty/SSH1/steady probes also use only that executable. Font/stage proofs receive the separately hashed diagnostic executable and launcher paths. No measured sub-run may invoke Cargo. It writes an atomic fragment for raw startup/product/functional/security records. It also reruns the Task 4 font proof, Task 7 stage matrix, and Task 8 external-source proof at the exact candidate SHA so `windows-memory-go` never reuses attribution evidence from an earlier commit.

Run:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked -j1
python -m unittest discover -s scripts/ci/tests -p 'test_*.py' -v
pwsh -NoProfile -File scripts/ci/run-stage7-product-gates.ps1 -Contract scripts/ci/stage7-split-contract.json -CandidateRef 0000000000000000000000000000000000000000 -OutputDirectory L:\rssh-evidence\stage7-product-dry-run -WhatIf
```

Expected: deterministic checks pass; PowerShell `-WhatIf` prints the exact two-checkout build plan, all interleaved rounds, all `-SkipBuild` child calls, exact contract LKG, and artifact layout without cloning, building, or launching a window. The shell runner's `--what-if` is executed by both protected Unix jobs before their real run; the local Windows plan does not require WSL/Git-Bash.

Commit and push/PR this tooling before triggering protected jobs:

```powershell
git add scripts/ci/run-stage7-product-gates.ps1 scripts/ci/run-stage7-product-gates.sh scripts/ci/tests/test_stage7_product_runners.py scripts/ci/run-ssh-gui-startup.ps1 scripts/ci/run-stage0-diagnostics.ps1 .github/workflows/release.yml scripts/ci/stage7-split-contract.json crates/rssh-app/tests/performance_scorecard_contract.rs
git commit -m "test(ci): add the Stage 7 product gate runner"
$candidateSha = (git rev-parse HEAD).Trim()
if ($candidateSha -notmatch '^[0-9a-f]{40}$') { throw 'candidate must be a full SHA' }
```

Any product-code defect found by the gate becomes a new RED/GREEN commit before this step is repeated; do not mix an unreviewed code fix into the evidence-only commit.

**Step 4: Run the protected Windows product gate from the immutable tooling commit**

On the fixed Windows runner, run the committed coordinator:

```powershell
$candidateSha = if (-not [string]::IsNullOrWhiteSpace($env:STAGE7_CANDIDATE_SHA)) { $env:STAGE7_CANDIDATE_SHA } else { $env:GITHUB_SHA }
if ($candidateSha -notmatch '^[0-9a-f]{40}$') { throw 'protected candidate must be a full SHA' }
$protectedCheckoutSha = (git rev-parse HEAD).Trim()
if ($protectedCheckoutSha -ne $candidateSha) { throw "protected checkout mismatch: $protectedCheckoutSha" }
$windowsRoot = 'L:\rssh-evidence\stage7-product'
pwsh -NoProfile -File scripts/ci/run-stage7-product-gates.ps1 -Contract scripts/ci/stage7-split-contract.json -CandidateRef $candidateSha -Profile release -Warmups 5 -Samples 30 -OutputDirectory $windowsRoot
```

Require startup/first-frame to request `auto`, finish the CPU bootstrap frame, omit GPU backend/adapter identity, and meet first-present p50 <=400 ms, p95 <=500 ms, first-frame p95 <=55 MiB, and every raw sample <60 MiB. Require only empty-window, SSH1, and GPU-steady scenarios to finish production `auto` on GPU with matching candidate/LKG backend/adapter; empty p95 <=45 MiB, SSH1 connected/GPU p95 <=60 MiB, GPU steady raw max <=256 MiB, and every candidate/LKG latency or memory ratio <=1.05. Recompute all statistics from raw records and assert the output `candidate_ref`, exact contract `rollback_ref`, executable hashes, and one runner fingerprint.

Assemble and validate the complete Windows state:

```powershell
$windowsRoot = 'L:\rssh-evidence\stage7-product'
python scripts/ci/assemble-stage7-evidence.py --contract scripts/ci/stage7-split-contract.json --requested-state attribution-ready --evidence-root $windowsRoot --fragment font/artifact-manifest-fragment.json --fragment stages/artifact-manifest-fragment.json --fragment external/artifact-manifest-fragment.json --output stage7-attribution-evidence-manifest.json
python scripts/ci/assemble-stage7-evidence.py --contract scripts/ci/stage7-split-contract.json --requested-state windows-memory-go --evidence-root $windowsRoot --prior-manifest stage7-attribution-evidence-manifest.json --fragment product/artifact-manifest-fragment.json --output stage7-evidence-manifest.json
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state windows-memory-go --evidence-manifest "$windowsRoot\stage7-evidence-manifest.json"
```

If it fails, keep Stage 7 NO-GO, record the exact failing cumulative stage, and stop before Task 13. A minimal clear-frame failure requires the separately approved fallback-gate design; it is not handled by raising a value here.

**Step 5: Run already-committed protected Linux and macOS jobs**

Only after the Task 12 tooling commit is pushed and its workflow is visible at `$candidateSha`, trigger native protected jobs from that exact SHA. Both jobs invoke committed `scripts/ci/run-stage7-product-gates.sh`; they do not invoke `run-stage0-diagnostics.sh` and do not depend on `pwsh`. Collect Linux PSS and macOS physical footprint, package smoke, native window/PTY/SSH tests, source/binary/runner identities, and same-platform exact-LKG ratios. Require valid raw samples and <=5% regressions. Each job writes a signed-by-hash fragment; download it with all referenced raw artifacts beneath `$windowsRoot\linux` or `$windowsRoot\macos` and verify every hash before assembly.

**Step 6: Derive `cross-platform-go` with the frozen manifest interface**

```powershell
$windowsRoot = 'L:\rssh-evidence\stage7-product'
python scripts/ci/assemble-stage7-evidence.py --contract scripts/ci/stage7-split-contract.json --requested-state cross-platform-go --evidence-root $windowsRoot --prior-manifest stage7-evidence-manifest.json --fragment linux/artifact-manifest-fragment.json --fragment macos/artifact-manifest-fragment.json --output stage7-cross-platform-evidence-manifest.json
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state cross-platform-go --evidence-manifest "$windowsRoot\stage7-cross-platform-evidence-manifest.json"
```

Independently recompute all reports and record runner fingerprints, source/binary hashes, raw counts, statistics, functional/security artifact hashes, and the decision. Do not copy raw machine paths into the document.

**Step 7: Commit only the evidence summary**

```powershell
git add docs/performance/stage7-product-gate-evidence.md
git commit -m "docs: certify Stage 7 product memory gates"
```

### Task 13: Remove product coupling from the R-Term extraction boundary

**Files:**
- Modify if dependency graph changes: `Cargo.lock`
- Create: `scripts/ci/rterm-release-contract-v2.json`
- Create: `scripts/ci/check-rterm-release-contract-v2.py`
- Create: `scripts/ci/tests/test_check_rterm_release_contract_v2.py`
- Create: `scripts/ci/check-rterm-bootstrap-templates.py`
- Create: `scripts/ci/tests/test_check_rterm_bootstrap_templates.py`
- Create: `docs/release/rterm-extraction-manifest.json`
- Modify: `docs/release/rterm-history-paths.txt`
- Create: `release/rterm-bootstrap/Cargo.toml`
- Create: `release/rterm-bootstrap/rust-toolchain.toml`
- Create: `release/rterm-bootstrap/.gitignore`
- Create: `release/rterm-bootstrap/.gitattributes`
- Create: `release/rterm-bootstrap/README.md`
- Create: `release/rterm-bootstrap/CONTRIBUTING.md`
- Create: `release/rterm-bootstrap/SECURITY.md`
- Create: `release/rterm-bootstrap/LICENSE`
- Create: `release/rterm-bootstrap/NOTICE`
- Create: `release/rterm-bootstrap/deny.toml`
- Create: `release/rterm-bootstrap/.github/workflows/ci.yml`
- Create: `release/rterm-bootstrap/contracts/rterm-consumer/Cargo.toml`
- Create: `release/rterm-bootstrap/docs/release-policy.md`
- Move: `crates/rterm-render-wgpu/tests/release_api.rs` -> `crates/rssh-renderer/tests/release_api.rs`
- Modify: `crates/rssh-renderer/Cargo.toml`
- Modify: `crates/rssh-renderer/tests/release_api.rs`
- Modify: `crates/rssh-runtime/tests/equivalence.rs`
- Modify: `crates/rssh-functional-tests/Cargo.toml`
- Create: `crates/rssh-functional-tests/tests/runtime_product_equivalence.rs`
- Move: `crates/rssh-runtime/tests/fixtures/transcripts/ssh_disconnect.txt` -> `crates/rssh-functional-tests/tests/fixtures/runtime-transcripts/ssh_disconnect.txt`
- Move: `crates/rssh-runtime/tests/fixtures/transcripts/mouse_ime_input.txt` -> `crates/rssh-functional-tests/tests/fixtures/runtime-transcripts/mouse_ime_input.txt`
- Move: `crates/rssh-runtime/tests/fixtures/transcripts/local_exit.txt` -> `crates/rssh-functional-tests/tests/fixtures/runtime-transcripts/local_exit.txt`

**Step 1: Write failing product-boundary tests**

Add named tests `product_transcripts_are_owned_by_rssh`, `rterm_equivalence_is_transport_neutral`, and `release_probe_is_facade_owned`. Require the three product transcripts to exist only in `rssh-functional-tests`, the R-Term runtime equivalence suite to read only terminal/runtime-owned fixtures, and the release API probe to run from `rssh-renderer` without a repository-relative local R-Term path.

Before creating any template, add a standalone template checker/test requiring the exact thirteen bootstrap files, seven workspace members, canonical paths, pinned toolchain, workspace lints/profiles/license/repository metadata, independent consumer workspace, consumer-root vendor patches, read-only CI permissions, locked fmt/Clippy/test/API/Task10 commands, and zero R-SSH product dependencies.

**Step 2: Observe RED**

```powershell
cargo test --locked -p rssh-functional-tests --test runtime_product_equivalence -j1
cargo test --locked -p rterm-runtime --test equivalence -j1
cargo test --locked -p rssh-renderer --test release_api -j1
python -m unittest scripts.ci.tests.test_check_rterm_bootstrap_templates -v
```

Expected: FAIL because product fixtures/probe still reside in R-Term, product test dependencies are missing, and bootstrap templates do not exist.

**Step 3: Move product-owned tests out of R-Term and verify the clean boundary**

Use `git mv` for the renderer facade release probe. Add `serde_json` to `rssh-renderer` dev-dependencies and rewrite the probe to inspect Cargo metadata/the external workspace package identity instead of hard-coding `crates/rterm-fonts`, so it survives R3. Split product transcript assertions and the three product fixtures into `rssh-functional-tests`; add direct versioned local path dev-dependencies for `rterm-runtime`, `rterm-terminal`, and `rterm-types` while the monorepo topology is still authoritative. Task 18, not this task, introduces root `[workspace.dependencies]` and centralizes those sources. Retain only transport-neutral terminal/runtime equivalence in R-Term. Update relative Task 10/parser and font-fixture paths so the future standalone workspace owns every referenced asset.

Create the complete reviewed standalone bootstrap templates listed above before R0. They are inert release inputs in R-SSH, not members of the product workspace. The future extractor maps these R0 blobs to the filtered repository root; Task 15 may not silently edit a template after R0.

After the manifest edits, run `cargo metadata --format-version 1` once without `--locked` solely to refresh `Cargo.lock` if Cargo requires it. Inspect the lockfile diff and reject unrelated version/source changes; from that point onward every Cargo command uses `--locked`.

Run:

```powershell
cargo test --locked -p rterm-render-wgpu --all-targets -j1
cargo test --locked -p rterm-runtime --all-targets -j1
cargo test --locked -p rssh-renderer --test release_api -j1
cargo test --locked -p rssh-functional-tests --all-targets -j1
python -m unittest scripts.ci.tests.test_check_rterm_bootstrap_templates -v
python scripts/ci/check-rterm-bootstrap-templates.py --root release/rterm-bootstrap
```

Expected: PASS, the three old product fixture paths are absent, and no R-Term test reads an R-SSH product path.

**Step 4: Commit the boundary first and freeze its full SHA as R0**

```powershell
git add Cargo.lock release/rterm-bootstrap scripts/ci/check-rterm-bootstrap-templates.py scripts/ci/tests/test_check_rterm_bootstrap_templates.py crates/rterm-render-wgpu/tests crates/rssh-renderer/Cargo.toml crates/rssh-renderer/tests crates/rssh-runtime/tests crates/rssh-functional-tests/Cargo.toml crates/rssh-functional-tests/tests
git commit -m "refactor: close the physical R-Term boundary"
$r0Sha = (git rev-parse HEAD).Trim()
if ($r0Sha -notmatch '^[0-9a-f]{40}$') { throw 'R0 must be a full SHA' }
if (-not [string]::IsNullOrWhiteSpace((git status --porcelain))) { throw 'R0 worktree must be clean' }
```

R0 is this boundary commit. It is intentionally the parent of the contract commit, so no tracked file attempts to contain its own commit SHA.

**Step 5: Write failing v2 contract tests against immutable R0**

Require the three contract views, both repository roots, exact `$r0Sha`, full filtered/bootstrap SHAs when those states exist, source-to-filtered map, seven canonical post-split paths, every old path, tree/blob identities recomputed from R0, Task 10 assets, font fixtures, licenses/docs/root files, and both vendor trees. Reject any undeclared file referenced by an R-Term test or build script, any `r0_ref` unequal to `$r0Sha`, or identities read from the later contract commit instead of R0.

```powershell
$r0Sha = (git rev-parse HEAD).Trim()
if ($r0Sha -notmatch '^[0-9a-f]{40}$') { throw 'R0 must be a full SHA' }
python -m unittest scripts.ci.tests.test_check_rterm_release_contract_v2 -v
```

Expected: FAIL because schema v2 and the complete extraction manifest are absent.

**Step 6: Implement the v2 checker and manifest**

Write `$r0Sha` into `r0_ref`. Enumerate exact R0 source paths, bootstrap-template source/destination paths, kind, source tree/blob ID, license class, and owner by reading immutable Git objects, not the current worktree. Freeze the bootstrap commit author, committer, email, message, and timestamp inputs used by Task 15 so two clean extractions produce the same child commit. Keep v1 unchanged as historical evidence. The v2 checker supports monorepo, R-Term, and R-SSH views and fails on reverse dependencies, undeclared assets, old package paths after normalization, template drift, or vendor drift.

For certification invocations, accept a bounded `--output` directory and atomically write raw recomputation results plus `artifact-manifest-fragment.json`; record full input SHAs and content identities, never mutable branch names.

**Step 7: Verify the contract against R0**

```powershell
$r0Sha = [string]((Get-Content -LiteralPath scripts/ci/rterm-release-contract-v2.json -Raw | ConvertFrom-Json).r0_ref)
if ($r0Sha -notmatch '^[0-9a-f]{40}$') { throw 'contract R0 must be a full SHA' }
python -m unittest scripts.ci.tests.test_check_rterm_release_contract_v2 -v
python scripts/ci/check-rterm-release-contract-v2.py --contract scripts/ci/rterm-release-contract-v2.json --view monorepo --rssh-root . --rssh-ref $r0Sha
```

Expected: PASS and every manifest tree/blob identity recomputes from exact R0.

**Step 8: Commit the non-self-referential contract**

```powershell
git add scripts/ci/rterm-release-contract-v2.json scripts/ci/check-rterm-release-contract-v2.py scripts/ci/tests/test_check_rterm_release_contract_v2.py docs/release/rterm-extraction-manifest.json docs/release/rterm-history-paths.txt
git commit -m "feat(release): freeze the R-Term extraction contract v2"
```

### Task 14: Rehearse deterministic history extraction and Task 10 provenance

**Files:**
- Create: `scripts/ci/extract-rterm-history.py`
- Create: `scripts/ci/tests/test_extract_rterm_history.py`
- Create: `scripts/ci/verify-task10-cross-repo.py`
- Create: `scripts/ci/tests/test_verify_task10_cross_repo.py`
- Create: `docs/release/rterm-source-to-filtered.schema.json`
- Modify: `scripts/ci/tests/test_check_rterm_release_contract_v2.py`

**Prerequisite: Verify the history tool explicitly**

Run `git filter-repo --version` and record the version/path. It is currently absent on the planning host, so execution must stop here and request authorization to install one pinned `git-filter-repo` release into a bounded tools directory such as `L:\rssh-tools\git-filter-repo`; verify its published/source hash and put only that exact directory on the task-local `PATH`. Do not use an unpinned download, silently switch algorithms, or claim the Task 14 real rehearsal passed while the tool is missing.

**Step 1: Write failing real-Git extraction tests**

Create temporary fixture repositories and test exact path inclusion, canonical path renames, deleted non-R-Term files, source-to-filtered commit mapping, tree/blob identity recording, absence of remotes after filtering, refusal to use the live workspace, and preservation of the original source refs. Resolve real paths and allow exactly two containment relationships: source and destination are distinct proper descendants of the caller-selected staging root. Reject equality/containment between source and destination, any intersection between the staging tree and live workspace, and any source/destination `git rev-parse --git-common-dir` or `objects/info/alternates` that resolves into the original object database. Require `git filter-repo`; fail with an actionable missing-tool result rather than silently using another history algorithm.

**Step 2: Write failing dual-repository Task 10 tests**

Require the verifier to read original commits/trees/blobs from an immutable R-SSH R0 clone and current sources/fixtures from an immutable filtered R-Term clone. Reject a missing object, wrong map, rewritten fixture, provenance mismatch, mutable ref, dirty checkout, or an attempt to require the old SHA inside the filtered repository.

**Step 3: Observe RED**

```powershell
python -m unittest scripts.ci.tests.test_extract_rterm_history -v
python -m unittest scripts.ci.tests.test_verify_task10_cross_repo -v
```

Expected: FAIL because both tools are missing.

**Step 4: Implement bounded extraction**

The extractor accepts an absolute source clone, an absent destination under a caller-selected staging root, full R0 SHA, and the checked manifest. Resolve and verify every path/tree before invoking `git filter-repo` with argument arrays. Rename `crates/rssh-terminal` and `crates/rssh-runtime` to canonical R-Term paths during filtering. Capture filter-repo's commit map plus destination tree/blob hashes into one atomic JSON artifact.

Require source and destination to be distinct proper descendants of the staging root. Refuse source/destination equality or mutual containment; any equality/containment between the staging tree and original workspace; a dirty source; any shared `git-common-dir`/object alternate with the original repository; a source with mutable R0; or any undeclared path.

**Step 5: Implement the Task 10 cross-repository verifier**

Keep `check-task10-provenance.py` and schema v1 unchanged. The new verifier composes its existing content/provenance checks across two object stores using the mapping artifact and emits a v2 attestation with both full SHAs and content hashes.

The extractor and verifier each write an atomic `artifact-manifest-fragment.json` covering the full source/filter map, all output refs/trees/blobs, original-object-database independence proof, and dual-repository Task 10 attestation.

**Step 6: Verify on fixtures and one disposable R-SSH clone**

```powershell
python -m unittest scripts.ci.tests.test_extract_rterm_history scripts.ci.tests.test_verify_task10_cross_repo -v
$stage7Root = 'L:\rssh-stage7'
$sourceClone = "$stage7Root\rssh-source-clone"
$filteredClone = "$stage7Root\rterm-filtered"
New-Item -ItemType Directory -Force -Path $stage7Root | Out-Null
if (Test-Path -LiteralPath $sourceClone) { throw "fresh source clone path must be absent: $sourceClone" }
if (Test-Path -LiteralPath $filteredClone) { throw "fresh filtered destination must be absent: $filteredClone" }
$releaseContract = Get-Content -LiteralPath scripts/ci/rterm-release-contract-v2.json -Raw | ConvertFrom-Json
$r0Sha = [string]$releaseContract.r0_ref
if ($r0Sha -notmatch '^[0-9a-f]{40}$') { throw 'R0 must be a full SHA' }
git clone --no-local --no-checkout E:\project\R-SSH $sourceClone
git -C $sourceClone checkout --detach $r0Sha
if ((git -C $sourceClone rev-parse HEAD).Trim() -ne $r0Sha) { throw 'fresh clone is not at contract R0' }
if (-not [string]::IsNullOrWhiteSpace((git -C $sourceClone status --porcelain))) { throw 'fresh clone is dirty' }
$commonDir = (git -C $sourceClone rev-parse --path-format=absolute --git-common-dir).Trim()
$originalCommonDir = (git -C E:\project\R-SSH rev-parse --path-format=absolute --git-common-dir).Trim()
if ($commonDir -eq $originalCommonDir) { throw 'source clone shares the original object database' }
$alternates = Join-Path $commonDir 'objects\info\alternates'
if (Test-Path -LiteralPath $alternates) { throw 'source clone has object alternates' }
python scripts/ci/extract-rterm-history.py --source $sourceClone --source-ref $r0Sha --destination $filteredClone --staging-root $stage7Root --manifest docs/release/rterm-extraction-manifest.json --map-output "$stage7Root\source-to-filtered.json"
```

Expected: tests pass; the fresh non-local clone owns an independent object database; the disposable extraction contains only declared R-Term history, has no object alternates and no remote, and the original refs/object directory fingerprint is unchanged. Never substitute the live `E:\project\R-SSH` checkout for the disposable source clone.

**Step 7: Commit**

```powershell
git add scripts/ci/extract-rterm-history.py scripts/ci/tests/test_extract_rterm_history.py scripts/ci/verify-task10-cross-repo.py scripts/ci/tests/test_verify_task10_cross_repo.py docs/release/rterm-source-to-filtered.schema.json scripts/ci/tests/test_check_rterm_release_contract_v2.py
git commit -m "feat(release): rehearse filtered R-Term history"
```

### Task 15: Add the standalone R-Term bootstrap and publishability gate

**Files:**
- Consume unchanged from R0: `release/rterm-bootstrap/**`
- Create: `scripts/ci/check-rterm-publishability.py`
- Create: `scripts/ci/tests/test_check_rterm_publishability.py`
- Modify: `scripts/ci/extract-rterm-history.py`
- Modify: `scripts/ci/tests/test_extract_rterm_history.py`
- Create: `docs/release/rterm-license-policy.json`
- Generate only in disposable R-Term: `Cargo.lock`
- Generate only in disposable R-Term: `contracts/rterm-consumer/Cargo.lock`

**Step 1: Write failing bootstrap materialization contracts**

Require a seven-member standalone workspace, canonical package paths, pinned toolchain, workspace lint/profile/license/repository metadata, consumer-root vendor patches, read-only CI permissions, locked fmt/Clippy/test/API/Task10 commands, two committed lockfiles (root plus the independent consumer workspace), and no R-SSH product dependency. Verify every template blob against its R0 identity; a needed template change invalidates R0 and returns to Task 13 rather than being patched here.

**Step 2: Write failing full-history safety tests**

Using temporary Git histories, require detection of private-key headers, credential patterns, forbidden absolute machine paths, unapproved binary/large objects, missing licenses, vendor license drift, and incomplete SBOM entries. Require scanner version, full input SHA, reachable-object count, and report hash in output.

**Step 3: Observe RED**

```powershell
python -m unittest scripts.ci.tests.test_check_rterm_publishability -v
python -m unittest scripts.ci.tests.test_check_rterm_release_contract_v2.RTermReleaseContractV2Tests.test_bootstrap -v
```

Expected: FAIL because publishability/materialization logic and the generated locked workspaces are absent; the reviewed bootstrap templates themselves already exist at R0.

**Step 4: Implement bootstrap materialization**

Add an explicit extractor mode `--bootstrap-template-root <R0-path> --commit-bootstrap`. It materializes the exact R0 bootstrap-template blobs at the filtered root, preserves MIT `LICENSE`/`NOTICE`, generates the standalone root `Cargo.lock` and independent `contracts/rterm-consumer/Cargo.lock` exactly once, and commits both as one child of the filtered boundary. Set author/committer names, emails, message, and timestamps to the fixed contract values; tests run two fresh extractions and require identical tree, both lockfile hashes, and bootstrap commit SHA. Record both `filtered_boundary_sha` and `bootstrap_sha`; all later commands use both committed lockfiles with `--locked`.

**Step 5: Implement publishability and SBOM checks**

Enumerate all reachable objects with Git plumbing, scan bounded text blobs for secrets/private keys/machine paths, classify large/binary objects against an allowlist, verify every source/vendor license, and create a deterministic package/license/source SBOM from locked Cargo metadata. Do not print matched secret contents. Permit only schema-approved runner fields and irreversible digests in evidence.

Write raw object/license/SBOM inventories plus one atomic `artifact-manifest-fragment.json` binding scanner versions, full input SHA, reachable-object count, zero unresolved findings, standalone command results, and every inventory hash.

**Step 6: Validate the disposable R-Term repository**

```powershell
$bootstrapSource = 'L:\rssh-stage7\rssh-bootstrap-source'
$bootstrapRepo = 'L:\rssh-stage7\rterm-bootstrap-rehearsal'
if (Test-Path -LiteralPath $bootstrapSource) { throw "fresh bootstrap source must be absent: $bootstrapSource" }
if (Test-Path -LiteralPath $bootstrapRepo) { throw "fresh bootstrap destination must be absent: $bootstrapRepo" }
$r0Sha = [string]((Get-Content -LiteralPath scripts/ci/rterm-release-contract-v2.json -Raw | ConvertFrom-Json).r0_ref)
if ($r0Sha -notmatch '^[0-9a-f]{40}$') { throw 'R0 must be a full SHA' }
git clone --no-local --no-checkout E:\project\R-SSH $bootstrapSource
git -C $bootstrapSource checkout --detach $r0Sha
python scripts/ci/extract-rterm-history.py --source $bootstrapSource --source-ref $r0Sha --destination $bootstrapRepo --staging-root L:\rssh-stage7 --manifest docs/release/rterm-extraction-manifest.json --map-output L:\rssh-stage7\bootstrap-source-to-filtered.json --bootstrap-template-root "$bootstrapSource\release\rterm-bootstrap" --commit-bootstrap
$bootstrapSha = (git -C $bootstrapRepo rev-parse HEAD).Trim()
if ($bootstrapSha -notmatch '^[0-9a-f]{40}$') { throw 'bootstrap ref must be a full SHA' }
$bootstrapRehearsalSha = $bootstrapSha
python scripts/ci/check-rterm-publishability.py --repo $bootstrapRepo --ref $bootstrapSha --license-policy docs/release/rterm-license-policy.json --output L:\rssh-stage7\publishability.json --sbom-output L:\rssh-stage7\rterm-sbom.json
cargo fmt --manifest-path "$bootstrapRepo\Cargo.toml" --all -- --check
cargo clippy --manifest-path "$bootstrapRepo\Cargo.toml" --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path "$bootstrapRepo\Cargo.toml" --workspace --all-targets --locked -j1
cargo check --manifest-path "$bootstrapRepo\contracts\rterm-consumer\Cargo.toml" --locked
```

Expected: PASS, zero unresolved findings, two unchanged committed lockfiles, deterministic SBOM, and clean worktree. Record `$bootstrapRehearsalSha` only as rehearsal evidence; Task 16 assigns canonical R1 from its fresh reproducible certification extraction.

**Step 7: Commit**

```powershell
git add scripts/ci/check-rterm-publishability.py scripts/ci/tests/test_check_rterm_publishability.py scripts/ci/extract-rterm-history.py scripts/ci/tests/test_extract_rterm_history.py docs/release/rterm-license-policy.json
git commit -m "feat(release): bootstrap a publishable R-Term workspace"
```

### Task 16: Certify `extraction-ready` locally

**Files:**
- Create: `docs/release/stage7-extraction-evidence.md`

**Step 1: Re-extract from a fresh non-local clone**

Clone the exact protected R-SSH boundary into a new bounded staging directory, run Task 14 extraction, and materialize Task 15 bootstrap. Do not reuse a previous failed checkout.

```powershell
$certificationRoot = 'L:\rssh-stage7-certification'
$certificationSource = "$certificationRoot\rssh-r0"
$canonicalRterm = "$certificationRoot\rterm-r1"
if (Test-Path -LiteralPath $certificationRoot) { throw "fresh certification root must be absent: $certificationRoot" }
New-Item -ItemType Directory -Path $certificationRoot | Out-Null
$r0Sha = [string]((Get-Content -LiteralPath scripts/ci/rterm-release-contract-v2.json -Raw | ConvertFrom-Json).r0_ref)
if ($r0Sha -notmatch '^[0-9a-f]{40}$') { throw 'R0 must be a full SHA' }
git clone --no-local --no-checkout E:\project\R-SSH $certificationSource
git -C $certificationSource checkout --detach $r0Sha
python scripts/ci/extract-rterm-history.py --source $certificationSource --source-ref $r0Sha --destination $canonicalRterm --staging-root $certificationRoot --manifest docs/release/rterm-extraction-manifest.json --map-output "$certificationRoot\source-to-filtered.json" --bootstrap-template-root "$certificationSource\release\rterm-bootstrap" --commit-bootstrap
$r1Sha = (git -C $canonicalRterm rev-parse HEAD).Trim()
if ($r1Sha -notmatch '^[0-9a-f]{40}$') { throw 'canonical R1 must be a full SHA' }
$filteredBoundarySha = (git -C $canonicalRterm rev-parse "$r1Sha^").Trim()
if ($filteredBoundarySha -notmatch '^[0-9a-f]{40}$') { throw 'filtered boundary must be a full SHA' }
```

Record exact R0, filtered-boundary SHA, and `$r1Sha`. Require `$r1Sha` to equal the deterministic bootstrap SHA produced by a second fresh extraction and the Task 15 rehearsal record; otherwise stop for nondeterminism. R1 is assigned only here: it is the standalone bootstrap child while R-SSH still consumes local paths.

**Step 2: Run all standalone and cross-repository gates**

Run release-contract v2 in monorepo and R-Term views, Task 10 dual verification, publishability scan, SBOM, API consumer, fmt, Clippy, workspace tests, vendor-tree checks, package smoke, and the local bare-Git proof from Task 8. Put all raw outputs and their fragments beneath one absent bounded root, and copy the previously validated `cross-platform-go` manifest plus all recursively referenced files into a read-only `prior` subdirectory without changing bytes. Require history, Task 10, publishability, standalone, and external-source fragments all to carry `r1_ref=$r1Sha`; add a negative validator fixture that changes one fragment's R1.

For the external-source fragment, run canonical mode rather than the Gate 0 synthesizer:

```powershell
$canonicalRterm = 'L:\rssh-stage7-certification\rterm-r1'
$r1Sha = (git -C $canonicalRterm rev-parse HEAD).Trim()
if ($r1Sha -notmatch '^[0-9a-f]{40}$') { throw 'canonical R1 must be a full SHA' }
$extractionEvidenceRoot = 'L:\rssh-evidence\stage7-extraction-ready'
if (Test-Path -LiteralPath $extractionEvidenceRoot) { throw "fresh extraction evidence root must be absent: $extractionEvidenceRoot" }
New-Item -ItemType Directory -Path $extractionEvidenceRoot | Out-Null
$crossPlatformBundle = 'L:\rssh-evidence\stage7-product'
$crossPlatformManifest = "$crossPlatformBundle\stage7-cross-platform-evidence-manifest.json"
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state cross-platform-go --evidence-manifest $crossPlatformManifest
New-Item -ItemType Directory -Path "$extractionEvidenceRoot\prior" | Out-Null
Get-ChildItem -LiteralPath $crossPlatformBundle -Force | Copy-Item -Destination "$extractionEvidenceRoot\prior" -Recurse
Get-ChildItem -LiteralPath "$extractionEvidenceRoot\prior" -File -Recurse | ForEach-Object { $_.IsReadOnly = $true }
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state cross-platform-go --evidence-manifest "$extractionEvidenceRoot\prior\stage7-cross-platform-evidence-manifest.json"
python scripts/ci/prove-rterm-external-source.py --contract scripts/ci/rterm-external-source-proof.json --candidate-repo $canonicalRterm --candidate-ref $r1Sha --output "$extractionEvidenceRoot\external-source" --keep-on-failure
```

**Step 3: Derive `extraction-ready` or stop**

Use the frozen predecessor-chain interface:

```powershell
$extractionEvidenceRoot = 'L:\rssh-evidence\stage7-extraction-ready'
$canonicalRterm = 'L:\rssh-stage7-certification\rterm-r1'
$r1Sha = (git -C $canonicalRterm rev-parse HEAD).Trim()
if ($r1Sha -notmatch '^[0-9a-f]{40}$') { throw 'canonical R1 must be a full SHA' }
python scripts/ci/assemble-stage7-evidence.py --contract scripts/ci/stage7-split-contract.json --requested-state extraction-ready --evidence-root $extractionEvidenceRoot --prior-manifest prior/stage7-cross-platform-evidence-manifest.json --fragment boundary/artifact-manifest-fragment.json --fragment history/artifact-manifest-fragment.json --fragment task10/artifact-manifest-fragment.json --fragment publishability/artifact-manifest-fragment.json --fragment standalone/artifact-manifest-fragment.json --fragment external-source/artifact-manifest-fragment.json --output stage7-extraction-ready-evidence-manifest.json
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state extraction-ready --evidence-manifest "$extractionEvidenceRoot\stage7-extraction-ready-evidence-manifest.json"
$validatedR1 = [string]((Get-Content -LiteralPath "$extractionEvidenceRoot\stage7-extraction-ready-evidence-manifest.json" -Raw | ConvertFrom-Json).rterm.r1_ref)
if ($validatedR1 -ne $r1Sha) { throw "validated R1 mismatch: $validatedR1" }
```

Any missing fragment, hash/ancestry mismatch, or failed command preserves the monorepo and stops before remote creation.

Read the assembled manifest back and require its canonical `rterm.r1_ref` to equal `$r1Sha`. Store that full value in `docs/release/stage7-extraction-evidence.md`; Task 17 must reload it from this validated manifest rather than rely on a shell variable.

**Step 4: Independently review and document**

Verify the original R-SSH refs and object database are unchanged, every extracted path is declared, no remote exists in the filtered clone, all safety/license findings are closed, and rollback SHAs are full and immutable.

**Step 5: Commit**

```powershell
git add docs/release/stage7-extraction-evidence.md
git commit -m "docs: certify R-Term extraction readiness"
```

### Task 17: Publish and promote the isolated R-Term candidate (two explicit authorization checkpoints)

**Files:**
- No R-SSH source change until the candidate commit and CI evidence exist.
- Remote candidate: `https://github.com/lcxinc/R-Term.git`

**Step 1: Stop and request exact external authorization**

Revalidate the Task 16 extraction-ready manifest, reload canonical `$r1Sha` from its `rterm.r1_ref`, require a full SHA, and require the local R-Term checkout to resolve exactly to it. Present the repository URL, proposed visibility (private), that exact R1/bootstrap SHA, complete-history scan/SBOM hashes, candidate branch name `codex/stage7-rterm-candidate`, the inert `repository-placeholder` bootstrap branch described below, and the fact that filtered history/code will be sent to GitHub. Do not create or push the repository without explicit approval, and never substitute a later local `HEAD` for validated R1.

```powershell
$extractionEvidenceRoot = 'L:\rssh-evidence\stage7-extraction-ready'
$extractionManifest = "$extractionEvidenceRoot\stage7-extraction-ready-evidence-manifest.json"
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state extraction-ready --evidence-manifest $extractionManifest
$r1Sha = [string]((Get-Content -LiteralPath $extractionManifest -Raw | ConvertFrom-Json).rterm.r1_ref)
if ($r1Sha -notmatch '^[0-9a-f]{40}$') { throw 'validated R1 must be a full SHA' }
if ((git -C L:\rssh-stage7-certification\rterm-r1 rev-parse HEAD).Trim() -ne $r1Sha) { throw 'local R-Term checkout is not canonical R1' }
```

**Step 2: Verify GitHub authentication and destination state**

After approval, run read-only auth/repository checks. Refuse an existing repository unless its owner, visibility, default branch, refs, and intended reuse are separately confirmed.

**Step 3: Create a private repository without auto-promoting candidate**

Create the approved empty private repository. Before sending filtered history, push one independently scanned inert commit containing only a placeholder notice to `refs/heads/repository-placeholder`; verify GitHub makes that branch, not the candidate, the temporary default. Then push the exact local bootstrap SHA only to `refs/heads/codex/stage7-rterm-candidate`. Do not push tags or other filtered refs. Fetch both refs back into a fresh clone, compare the candidate SHA byte-for-byte, and verify the default branch is still `repository-placeholder`. If the hosting service cannot guarantee this ordering, stop rather than letting the candidate become default implicitly.

**Step 4: Configure and run protected candidate CI**

Apply read-only workflow permissions and branch protection appropriate to the candidate. Run standalone R-Term CI, upload locked evidence, and resolve all Critical/Important review findings. If private, verify the approved read-only cross-repository credential and fork-PR degradation policy without printing credentials.

**Step 5: Stop for the second external authorization**

Present the still-private remote URL, temporary/default and candidate refs, full fetch-back candidate SHA, protected CI run IDs/artifact hashes, branch-protection settings, and the exact proposed action: create `refs/heads/main` at that same candidate SHA, protect it, then change the default branch from `repository-placeholder` to `main`. Request separate approval. Public visibility is not part of this request and remains a later independent action.

**Step 6: Create and verify the protected default branch only after approval**

Pre-create the `main` protection/ruleset where the provider supports it, push the already-reviewed candidate SHA to the new `refs/heads/main` without force, fetch it back, require equality with the candidate SHA, run/confirm the same protected CI at that SHA, and only then change the remote default branch to `main`. Re-query visibility, default branch, protections, and refs from a fresh authenticated session. Any mismatch stops Task 18.

Write an atomic remote-publication evidence fragment containing both authorization decisions, remote URL/visibility, placeholder/candidate/main SHAs, fetch-back results, protection and CI hashes, and zero credentials. Do not delete the placeholder or candidate branch in this plan.

### Task 18: Switch R-SSH to the immutable external R-Term commit

**Files:**
- Create: `docs/release/stage7-dual-source-evidence.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `crates/rssh-core/Cargo.toml`
- Modify: `crates/rssh-domain/Cargo.toml`
- Modify: `crates/rssh-native/Cargo.toml`
- Modify: `crates/rssh-pty/Cargo.toml`
- Modify: `crates/rssh-renderer/Cargo.toml`
- Modify: `crates/rssh-ssh/Cargo.toml`
- Modify if Task 13 introduced direct entries: `crates/rssh-functional-tests/Cargo.toml`
- Modify: `contracts/rterm-consumer/Cargo.toml`
- Modify: `contracts/rterm-consumer/Cargo.lock`
- Modify: `scripts/ci/rterm-release-contract-v2.json`
- Modify: `scripts/ci/rehearse-rterm-consumer.py`
- Modify: `scripts/ci/tests/test_rehearse_rterm_consumer.py`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`

**Step 1: Write failing external-source tests**

Require Task 17's remote to be private, its default branch to be protected `main`, and fetched `main` plus candidate to resolve to the same approved full SHA before any manifest edit. Require all seven R-Term dependencies to use one workspace-owned Git URL/full rev, remove the seven directories from workspace members while retaining them in `exclude`, reject all `path+file` resolved sources, require the committed lockfiles, and verify consumer-root vendor paths. Require source-switch rollback to restore local paths in one commit.

**Step 2: Observe RED**

```powershell
python -m unittest scripts.ci.tests.test_rehearse_rterm_consumer -v
python scripts/ci/check-rterm-release-contract-v2.py --contract scripts/ci/rterm-release-contract-v2.json --view rssh --rssh-root . --rterm-root L:\rssh-stage7\rterm-filtered --source-map L:\rssh-stage7\source-to-filtered.json
```

Expected: FAIL while manifests still use local paths.

**Step 3: Centralize and switch sources**

Add the seven packages under `[workspace.dependencies]` using the approved R-Term URL and exact protected SHA. Change every R-SSH product manifest and standalone consumer to `workspace = true` or the same exact Git source. Remove local packages from `workspace.members`, add their seven directories to `workspace.exclude`, and update the workspace repository URL to the approved R-SSH remote.

Before editing, enumerate path references and classify them against the exact seven retained local package manifests versus the product manifests above. An unexpected product manifest is a failing test and must be added explicitly; do not mechanically rewrite the retained R-Term manifests because they are the R2 rollback topology.

Generate/update lockfiles once before commit, inspect the diff, then require all subsequent commands to use `--locked`. Do not delete the local directories.

**Step 4: Prove real external resolution**

```powershell
cargo metadata --locked --format-version 1
cargo metadata --manifest-path contracts/rterm-consumer/Cargo.toml --locked --format-version 1
cargo check --manifest-path contracts/rterm-consumer/Cargo.toml --locked
cargo check --locked -p rssh-app --no-default-features --features production-gui
cargo test --locked -p rssh-ssh --all-targets -j1
cargo test --locked -p rssh-pty --all-targets -j1
cargo test --locked -p rssh-native --all-targets -j1
cargo test --locked -p rssh-functional-tests --all-targets -j1
```

Require both root and independent-consumer metadata to show all seven packages from the same Git SHA and both patched dependencies from the R-SSH consumer vendor root. Require the locked commands not to mutate either lockfile and require `git status --short` to contain only the explicitly listed source-switch files before commit.

**Step 5: Create the unique source-switch commit and record R2**

After Step 4 is green, create exactly one source-switch commit:

```powershell
git add Cargo.toml Cargo.lock crates/rssh-app/Cargo.toml crates/rssh-core/Cargo.toml crates/rssh-domain/Cargo.toml crates/rssh-native/Cargo.toml crates/rssh-pty/Cargo.toml crates/rssh-renderer/Cargo.toml crates/rssh-ssh/Cargo.toml crates/rssh-functional-tests/Cargo.toml contracts/rterm-consumer/Cargo.toml contracts/rterm-consumer/Cargo.lock scripts/ci/rterm-release-contract-v2.json scripts/ci/rehearse-rterm-consumer.py scripts/ci/tests/test_rehearse_rterm_consumer.py .github/workflows/ci.yml .github/workflows/release.yml
git commit -m "build: consume R-Term from an immutable Git revision"
$r2Sha = (git rev-parse HEAD).Trim()
if ($r2Sha -notmatch '^[0-9a-f]{40}$') { throw 'R2 must be a full SHA' }
```

This is the sole commit whose revert restores local path dependencies. Do not amend it after rollback rehearsal starts.

**Step 6: Rehearse rollback in a fresh disposable clone**

Resolve R2 anew from the current immutable Git ref; do not depend on a prior shell variable or attempt to store a commit's own SHA inside itself:

```powershell
$r2Sha = (git rev-parse HEAD).Trim()
if ($r2Sha -notmatch '^[0-9a-f]{40}$') { throw 'R2 must be a full SHA' }
$r2Subject = (git show -s --format=%s $r2Sha).Trim()
if ($r2Subject -ne 'build: consume R-Term from an immutable Git revision') { throw "HEAD is not the source-switch commit: $r2Subject" }
```

Clone that exact R2 with `--no-local`, detach at `$r2Sha`, revert only `$r2Sha` with `--no-commit`, and prove local path metadata plus all locked tests. Abort that rehearsal revert, return to exact R2, and prove external resolution again. Store full before/reverted/reapplied SHAs, metadata, lockfile/worktree hashes, command results, and one atomic rollback fragment. Protected jobs independently receive and verify the same full SHA through `STAGE7_R2_SHA` in Step 7.

**Step 7: Run protected CI and product gates at exact R2**

Pass the reviewed source-switch commit as explicit protected workflow input `STAGE7_R2_SHA`. Create the bounded evidence root and import the complete predecessor bundle before adding new artifacts:

```powershell
$r2Sha = [string]$env:STAGE7_R2_SHA
if ($r2Sha -notmatch '^[0-9a-f]{40}$') { throw 'protected R2 input must be a full SHA' }
$protectedR2Checkout = (git rev-parse HEAD).Trim()
if ($protectedR2Checkout -ne $r2Sha) { throw "protected R2 checkout mismatch: $protectedR2Checkout" }
$extractionBundle = 'L:\rssh-evidence\stage7-extraction-ready'
$dualRoot = 'L:\rssh-evidence\stage7-dual-source'
if (Test-Path -LiteralPath $dualRoot) { throw "dual-source evidence root must be absent: $dualRoot" }
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state extraction-ready --evidence-manifest "$extractionBundle\stage7-extraction-ready-evidence-manifest.json"
New-Item -ItemType Directory -Path "$dualRoot\prior" -Force | Out-Null
Get-ChildItem -LiteralPath $extractionBundle -Force | Copy-Item -Destination "$dualRoot\prior" -Recurse
Get-ChildItem -LiteralPath "$dualRoot\prior" -File -Recurse | ForEach-Object { $_.IsReadOnly = $true }
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state extraction-ready --evidence-manifest "$dualRoot\prior\stage7-extraction-ready-evidence-manifest.json"
```

Push/PR exact R2, then run API/consumer, workspace, package, native SSH/PTY/window, fixed Windows memory/startup, Linux/macOS baseline, candidate/LKG comparisons, Task 10 dual provenance, and remote fetch-back checks against the external source. Each protected job must report `$r2Sha` and the approved R-Term SHA. Download/copy the already verified Task 17 remote fragment into `$dualRoot\remote`; write or download the new external-consumer, rollback, protected-CI, Task 10, and product bundles into the same-name `$dualRoot` subdirectories used by Step 8. Before assembly, require each subdirectory to contain its atomic fragment and every referenced raw file. Any regression keeps the state below `dual-source-verified`; local packages remain available and the single R2 commit remains revertible.

**Step 8: Derive `dual-source-verified`, then commit only its summary**

```powershell
$dualRoot = 'L:\rssh-evidence\stage7-dual-source'
python scripts/ci/assemble-stage7-evidence.py --contract scripts/ci/stage7-split-contract.json --requested-state dual-source-verified --evidence-root $dualRoot --prior-manifest prior/stage7-extraction-ready-evidence-manifest.json --fragment remote/artifact-manifest-fragment.json --fragment external-consumer/artifact-manifest-fragment.json --fragment rollback/artifact-manifest-fragment.json --fragment protected-ci/artifact-manifest-fragment.json --fragment task10/artifact-manifest-fragment.json --fragment product/artifact-manifest-fragment.json --output stage7-dual-source-evidence-manifest.json
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state dual-source-verified --evidence-manifest "$dualRoot\stage7-dual-source-evidence-manifest.json"
git add docs/release/stage7-dual-source-evidence.md
git commit -m "docs: certify immutable R-Term consumption"
```

The summary records R0/R1/R2 and all evidence hashes, but the source-switch commit remains the unique `$r2Sha`. Request `dual-source-verified` only after protected evidence and rollback pass.

### Task 19: Remove local R-Term packages and complete the split (explicit destructive checkpoint)

**Files to remove after approval:**
- `crates/rterm-types`
- `crates/rssh-terminal`
- `crates/rssh-runtime`
- `crates/rterm-fonts`
- `crates/rterm-render-core`
- `crates/rterm-render-cpu`
- `crates/rterm-render-wgpu`

**Files to modify:**
- `Cargo.toml`
- `Cargo.lock`
- `README.md`
- `docs/release/stage7-split-complete-evidence.md` (create)
- `docs/release/rterm-api-compatibility.md`
- `scripts/ci/rterm-release-contract-v2.json`
- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- `CODEOWNERS` if present

**Step 1: Stop and request deletion approval**

Show the exact seven resolved paths, existing R0/R1/R2 full SHAs, the external candidate SHA, the fact that R3 does not exist until the approved deletion commit is created, the `dual-source-verified` evidence hash, and the rollback procedure. Do not remove any directory without explicit approval.

**Step 2: Verify exact targets and references**

Resolve each path under the R-SSH workspace, verify no symlink/reparse escape, and run `rg`/Cargo metadata to prove production uses the external SHA. Confirm each local tree matches the extraction manifest or document intentional post-extraction differences.

**Step 3: Remove only the seven approved directories**

Use `git rm -r` with the seven literal verified paths in the same PowerShell/Git context. Remove their temporary workspace `exclude` entries and update ownership/docs/contracts. Keep the R-SSH consumer-root `glyphon` and `gpu-allocator` vendor trees.

**Step 4: Verify the deletion commit**

```powershell
cargo metadata --locked --format-version 1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked -j1
python -m unittest discover -s scripts/ci/tests -p 'test_*.py' -v
```

Also run package smoke, native ten-frame window E2E, loopback native SSH/PTY tests, secret scans, fixed Windows product gates, Linux/macOS baselines, and v2 R-SSH/R-Term contract checks.

**Step 5: Commit, review, and complete one protected release**

```powershell
git add Cargo.toml Cargo.lock README.md docs/release/rterm-api-compatibility.md scripts/ci/rterm-release-contract-v2.json .github/workflows/ci.yml .github/workflows/release.yml
if (Test-Path -LiteralPath CODEOWNERS) { git add CODEOWNERS }
git commit -m "refactor: complete the physical R-Term split"
$r3DeletionSha = (git rev-parse HEAD).Trim()
if ($r3DeletionSha -notmatch '^[0-9a-f]{40}$') { throw 'R3 deletion candidate must be a full SHA' }
```

Request code review with zero open Critical/Important findings. Merge through a protected non-squash PR so the reviewed deletion commit remains an ancestor, fetch the protected branch, record its full commit as R3, and verify the ancestry. Run one complete release cycle in both repositories and verify external source and artifact hashes. Make every R3 job/upload write to a fresh staging bundle named by `$r3Sha`, with the nine exact subdirectories used below. Each subdirectory must contain one atomic fragment and all of its referenced raw files.

Pass the reviewed PR head as an explicit protected workflow input `STAGE7_R3_DELETION_SHA`; never rely on the local shell variable surviving the PR/merge boundary.

```powershell
$r3DeletionSha = [string]$env:STAGE7_R3_DELETION_SHA
if ($r3DeletionSha -notmatch '^[0-9a-f]{40}$') { throw 'protected R3 deletion input must be a full SHA' }
git fetch origin main
$r3Sha = (git rev-parse origin/main).Trim()
if ($r3Sha -notmatch '^[0-9a-f]{40}$') { throw 'protected R3 must be a full SHA' }
git merge-base --is-ancestor $r3DeletionSha $r3Sha
if ($LASTEXITCODE -ne 0) { throw 'protected R3 does not preserve the reviewed deletion commit' }
$splitStaging = "L:\rssh-evidence\stage7-split-complete-staging\$r3Sha"
if (Test-Path -LiteralPath $splitStaging) { throw "fresh R3 staging bundle must be absent: $splitStaging" }
New-Item -ItemType Directory -Path $splitStaging | Out-Null
```

After the protected jobs and releases finish, assemble the final manifest from the prior `dual-source-verified` manifest plus deletion, protected-CI, both-release, rollback, product, Task 10, security, and package fragments; then run the exact final validator command in the matrix below. Only after it returns `split-complete`, create `docs/release/stage7-split-complete-evidence.md` from hashes/redacted identities and commit it separately as `docs: certify the physical R-Term split`.

```powershell
$dualBundle = 'L:\rssh-evidence\stage7-dual-source'
$splitRoot = 'L:\rssh-evidence\stage7-split-complete'
git fetch origin main
$r3Sha = (git rev-parse origin/main).Trim()
if ($r3Sha -notmatch '^[0-9a-f]{40}$') { throw 'protected R3 must be a full SHA' }
$splitStaging = "L:\rssh-evidence\stage7-split-complete-staging\$r3Sha"
if (Test-Path -LiteralPath $splitRoot) { throw "split-complete evidence root must be absent: $splitRoot" }
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state dual-source-verified --evidence-manifest "$dualBundle\stage7-dual-source-evidence-manifest.json"
New-Item -ItemType Directory -Path "$splitRoot\prior" -Force | Out-Null
Get-ChildItem -LiteralPath $dualBundle -Force | Copy-Item -Destination "$splitRoot\prior" -Recurse
Get-ChildItem -LiteralPath "$splitRoot\prior" -File -Recurse | ForEach-Object { $_.IsReadOnly = $true }
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state dual-source-verified --evidence-manifest "$splitRoot\prior\stage7-dual-source-evidence-manifest.json"
$artifactNames = @('deletion', 'protected-ci', 'rterm-release', 'rssh-release', 'rollback', 'product', 'task10', 'security', 'package')
foreach ($artifactName in $artifactNames) {
  $artifactSource = Join-Path $splitStaging $artifactName
  $fragmentSource = Join-Path $artifactSource 'artifact-manifest-fragment.json'
  if (-not (Test-Path -LiteralPath $fragmentSource -PathType Leaf)) { throw "missing protected artifact fragment: $fragmentSource" }
  $artifactDestination = Join-Path $splitRoot $artifactName
  New-Item -ItemType Directory -Path $artifactDestination | Out-Null
  Get-ChildItem -LiteralPath $artifactSource -Force | Copy-Item -Destination $artifactDestination -Recurse
}
Get-ChildItem -LiteralPath $splitRoot -File -Recurse | ForEach-Object { $_.IsReadOnly = $true }
python scripts/ci/assemble-stage7-evidence.py --contract scripts/ci/stage7-split-contract.json --requested-state split-complete --evidence-root $splitRoot --prior-manifest prior/stage7-dual-source-evidence-manifest.json --fragment deletion/artifact-manifest-fragment.json --fragment protected-ci/artifact-manifest-fragment.json --fragment rterm-release/artifact-manifest-fragment.json --fragment rssh-release/artifact-manifest-fragment.json --fragment rollback/artifact-manifest-fragment.json --fragment product/artifact-manifest-fragment.json --fragment task10/artifact-manifest-fragment.json --fragment security/artifact-manifest-fragment.json --fragment package/artifact-manifest-fragment.json --output evidence-manifest.json
```

**Step 6: Preserve rollback until soak completes**

If post-merge failure occurs, revert the deletion commit first; if necessary, revert the source-switch commit second. Do not delete the R-Term repository or rewrite either history. Remove transitional local-path rollback machinery only in a later approved cleanup after the soak window.

## Final verification matrix

Before declaring completion, all of the following must be green from immutable commits:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked -j1
python -m unittest discover -s scripts/ci/tests -p 'test_*.py' -v
python scripts/ci/check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state split-complete --evidence-manifest L:\rssh-evidence\stage7-split-complete\evidence-manifest.json
```

In addition: protected Windows 5+30 startup/empty/SSH1 evidence, protected Linux/macOS native baselines, R-Term standalone CI, R-SSH external-consumer CI, Task 10 dual provenance, publishability/SBOM, package smoke, rollback rehearsal, and both protected releases must pass. A missing artifact or unsupported required product probe is a NO-GO, not a waiver.
