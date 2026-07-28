# Production Parity Foundation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Deliver a production-oriented Windows/Linux/macOS terminal foundation with safe debug GUI startup, scalable output handling, grapheme-aware GPU text, and real SSH/PTY/native-window E2E.

**Architecture:** Migrate behind compatibility seams. First establish deterministic test infrastructure and fix the proven startup/streaming bottlenecks. Then replace terminal history and cell storage, introduce a dedicated `rssh-fonts` crate, move native presentation from `pixels` to direct `wgpu`/`glyphon`, and finally promote hermetic and independent interoperability tests across six native OS/architecture targets.

**Tech Stack:** Rust 1.89, winit 0.30, cosmic-text 0.19, glyphon 0.12, wgpu 30, russh, tokio, GitHub Actions.

---

## Execution Rules

- Work only in `E:\project\R-SSH\.worktrees\production-parity` on
  `codex/production-parity-foundation`.
- Use `CARGO_TARGET_DIR=E:\project\R-SSH\target\production-parity`.
- Follow RED → verify RED → minimal GREEN → verify GREEN → refactor.
- Commit each task independently.
- After every task, run a spec-compliance review and then a code-quality review.
- Do not start the next task while either review has open Critical or Important
  findings.
- Preserve all existing selection, reflow, image, `CellAttachment`, alternate
  screen, and app-shell behavior unless a task explicitly changes it.

## Milestone 1: Test Foundation and Debug GUI

### Task 1: Pin MSRV and strengthen the deterministic CI baseline

**Files:**

- Modify: `Cargo.toml`
- Modify: `.github/workflows/ci.yml`
- Create: `rust-toolchain.toml`
- Test: `.github/workflows/ci.yml`

**Step 1: Add the RED workflow assertions**

Add an MSRV job pinned to `1.89.0`, change all Cargo invocations to `--locked`,
and require `--workspace --all-targets`. Add `permissions: contents: read`.

The workflow must contain:

```yaml
permissions:
  contents: read

env:
  CARGO_TERM_COLOR: always

jobs:
  quality:
    runs-on: windows-2025
    steps:
      - run: cargo fmt --all -- --check
      - run: cargo clippy --locked --workspace --all-targets -- -D warnings
      - run: cargo test --locked --workspace --all-targets

  msrv:
    runs-on: ubuntu-24.04
    steps:
      - uses: dtolnay/rust-toolchain@1.89.0
      - run: cargo check --locked --workspace --all-targets
```

**Step 2: Verify the current metadata fails the new MSRV expectation**

Run:

```powershell
cargo metadata --locked --no-deps --format-version 1
```

Expected: workspace metadata still reports `rust_version = 1.85`.

**Step 3: Raise the workspace MSRV**

Set `workspace.package.rust-version = "1.89"` and add:

```toml
[toolchain]
channel = "1.89.0"
profile = "minimal"
components = ["clippy", "rustfmt"]
```

**Step 4: Verify**

Run:

```powershell
cargo +1.89.0 check --locked --workspace --all-targets
cargo +1.89.0 fmt --all -- --check
git diff --check
```

Expected: all commands exit zero.

**Step 5: Commit**

```powershell
git add Cargo.toml rust-toolchain.toml .github/workflows/ci.yml
git commit -m "ci: pin production parity MSRV"
```

### Task 2: Add deadline-aware E2E process primitives

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/rssh-test-support/Cargo.toml`
- Create: `crates/rssh-test-support/src/lib.rs`
- Create: `crates/rssh-test-support/src/process.rs`
- Create: `crates/rssh-test-support/src/temp_home.rs`
- Create: `crates/rssh-test-support/src/marker.rs`
- Test: `crates/rssh-test-support/src/process.rs`

**Step 1: Write failing tests**

Create tests:

```rust
#[test]
fn child_guard_returns_output_before_deadline() {}

#[test]
fn child_guard_kills_and_reaps_timed_out_child() {}

#[test]
fn temp_home_isolates_home_and_userprofile() {}

#[test]
fn marker_command_emits_exact_utf8_marker() {}
```

Use real short-lived child processes and assert behavior, not mock calls.

**Step 2: Verify RED**

Run:

```powershell
cargo test --locked -p rssh-test-support
```

Expected: compile failure because the crate and APIs do not exist.

**Step 3: Implement the minimal harness**

Provide:

```rust
pub struct ChildGuard { /* child plus deadline */ }
pub struct ChildOutput { pub status: ExitStatus, pub stdout: Vec<u8>, pub stderr: Vec<u8> }
pub struct TempHome { /* temp path and scoped environment map */ }
pub fn platform_marker_command(marker: &str) -> Command;
```

Every timeout path must kill, wait, and preserve redacted diagnostic output.
Do not mutate process-global HOME in parallel tests; return an environment map
for the child.

**Step 4: Verify GREEN**

Run:

```powershell
cargo test --locked -p rssh-test-support
cargo clippy --locked -p rssh-test-support --all-targets -- -D warnings
```

Expected: all tests pass.

**Step 5: Commit**

```powershell
git add Cargo.toml crates/rssh-test-support
git commit -m "test: add bounded native process harness"
```

### Task 3: Capture the Windows debug GUI stack overflow

**Files:**

- Modify: `crates/rssh-app/Cargo.toml`
- Create: `crates/rssh-app/tests/native_window_debug.rs`
- Test: `crates/rssh-app/tests/native_window_debug.rs`

**Step 1: Write the failing Windows-only subprocess test**

Launch:

```text
CARGO_BIN_EXE_rssh-app -n window --frames 1
```

through `rssh-test-support::ChildGuard` with a 30-second deadline. Assert exit
zero and that stderr does not contain `overflowed its stack`. Add the
`-n window --state-json` control case.

**Step 2: Verify RED**

Run:

```powershell
cargo test --locked -p rssh-app --test native_window_debug -- --nocapture
```

Expected on current Windows debug build: failure with status `0xC00000FD`.

**Step 3: Preserve the RED evidence**

Record the exact command and stack-frame evidence in the test failure message.
Do not weaken the test or switch it to release mode.

**Step 4: Commit the RED test**

```powershell
git add crates/rssh-app/Cargo.toml crates/rssh-app/tests/native_window_debug.rs
git commit -m "test: reproduce debug GUI stack overflow"
```

### Task 4: Heap-own startup state and eliminate large debug frames

**Files:**

- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/src/config_lifecycle.rs`
- Test: `crates/rssh-app/tests/native_window_debug.rs`
- Test: unit tests near `ConfiguredStartupApp` and `NativeWindowManager`

**Step 1: Add structural RED tests**

Add size budgets for the outer startup types:

```rust
assert!(size_of::<ConfiguredStartupApp>() <= 16 * 1024);
assert!(size_of::<NativeWindowManager>() <= 16 * 1024);
```

Expected: current types exceed the budget.

**Step 2: Verify RED**

Run focused size tests and confirm they fail for the expected size.

**Step 3: Implement grouped heap-owned state**

- Group `NativeConfigOverrides` fields by subsystem and box large groups.
- Store immutable effective overrides behind `Arc`.
- Box lifecycle and app state inside `ConfiguredStartupApp`.
- Box startup/pending/active app values at the owning container boundary.
- Avoid expressions that first construct the old large aggregate on the stack.
- Keep the event loop and surface creation on the main thread.

Do not use unsafe allocation or merely increase the executable stack.

**Step 4: Verify GREEN**

Run:

```powershell
cargo test --locked -p rssh-app configured_startup
cargo test --locked -p rssh-app window_manager
cargo test --locked -p rssh-app --test native_window_debug -- --nocapture
```

Expected: size tests and real debug executable smoke pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs crates/rssh-app/src/config_lifecycle.rs
git commit -m "fix: heap-own native GUI startup state"
```

## Milestone 2: Streaming and Scrollback Performance

### Task 5: Split benchmark workloads and expose deterministic work counters

**Files:**

- Modify: `crates/rssh-app/src/bench.rs`
- Modify: `crates/rssh-app/src/cli.rs`
- Modify: `docs/mvp-4-live-pty-window.md`
- Test: `crates/rssh-app/src/bench.rs`

**Step 1: Write failing tests**

Add benchmark workload modes:

```text
plain-scroll
ansi-scroll
ansi-scroll-query
```

Add JSON fields for inspected query bytes, scrolled survivor clones, history
relocations, and metadata rebase batches. Tests must reject unknown workload
names and round-trip each report.

**Step 2: Verify RED**

Run:

```powershell
cargo test --locked -p rssh-app bench::tests
cargo test --locked -p rssh-app cli::tests::parses_console_benchmark
```

Expected: missing workload/counter assertions fail.

**Step 3: Implement reporting only**

Do not optimize yet. Wire counters so the current implementation reports its
actual work.

**Step 4: Verify GREEN and capture baseline**

Run each workload in release mode at 256 KiB and record the JSON in the commit
message or task report.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/bench.rs crates/rssh-app/src/cli.rs docs/mvp-4-live-pty-window.md
git commit -m "bench: separate terminal workload costs"
```

### Task 6: Replace duplicate query filters with one single-pass scanner

**Files:**

- Create: `crates/rssh-app/src/terminal_queries.rs`
- Modify: `crates/rssh-app/src/main.rs`
- Modify: `crates/rssh-app/src/terminal_runtime.rs`
- Modify: `crates/rssh-app/src/local.rs`
- Reference: `crates/rssh-app/src/visible_output.rs`
- Test: `crates/rssh-app/src/terminal_queries.rs`

**Step 1: Write RED tests**

Cover:

- every existing fixed and dynamic query;
- split sequences at every byte boundary;
- multiple queries in one chunk;
- unknown CSI/OSC/DCS pass-through;
- C1 and seven-bit forms;
- inspected bytes no more than four times input;
- 16 KiB and 512 B chunk work ratio no more than 1.25.

**Step 2: Verify RED**

Run:

```powershell
cargo test --locked -p rssh-app terminal_queries
```

Expected: linear-work assertions fail against the old scanner.

**Step 3: Implement a streaming state machine**

Use one pending buffer and one forward cursor. Match fixed prefixes through a
trie/table-driven state and call dynamic parsers only for relevant control
families. Compact the buffer in batches, never after every event.

Both GUI and console paths must use this module; delete the duplicate scanners
after parity tests pass.

**Step 4: Verify GREEN**

Run:

```powershell
cargo test --locked -p rssh-app terminal_queries
cargo test --locked -p rssh-app terminal_output_filter
cargo run --locked --release -p rssh-app -- bench --json --workload ansi-scroll-query --bytes 1048576 --chunk-size 8192
```

Expected: all query behavior passes, deterministic work is linear, and the
release workload reaches the approved initial budget or documents a remaining
downstream bottleneck.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/terminal_queries.rs crates/rssh-app/src/main.rs crates/rssh-app/src/terminal_runtime.rs crates/rssh-app/src/local.rs
git commit -m "perf: scan terminal queries in one pass"
```

### Task 7: Introduce logical deque history

**Files:**

- Create: `crates/rssh-terminal/src/history.rs`
- Modify: `crates/rssh-terminal/src/lib.rs`
- Modify: `crates/rssh-terminal/src/parser.rs`
- Test: `crates/rssh-terminal/src/parser.rs`

**Step 1: Add behavior guards and performance RED**

Add tests:

```text
bounded_history_eviction_does_not_relocate_survivors
history_prune_preserves_cell_attachment_source_and_stable_selection
batched_scroll_prune_matches_incremental_prune
alternate_prune_rebases_active_and_dormant_attachment_destinations
history_container_reflow_preserves_wrapped_and_overflow_cells
```

The relocation counter test must fail against `Vec::drain(..1)`.

**Step 2: Verify RED**

Run the five focused tests and confirm only the complexity assertion is RED;
behavior characterization tests should describe the existing result.

**Step 3: Implement `HistoryBuffer` over `VecDeque`**

Expose logical `len`, `get`, `iter`, range iteration, push, front eviction, clear,
and rebuild operations. Do not expose physical slices.

Batch pruning so stable-row offset and metadata rebase happen once per batch.

**Step 4: Verify GREEN**

Run:

```powershell
cargo test --locked -p rssh-terminal history_
cargo test --locked -p rssh-terminal scrollback
cargo test --locked -p rssh-terminal attachment
```

Expected: behavior remains identical and survivor relocation is zero.

**Step 5: Commit**

```powershell
git add crates/rssh-terminal/src/history.rs crates/rssh-terminal/src/lib.rs crates/rssh-terminal/src/parser.rs
git commit -m "perf: make bounded history eviction constant time"
```

### Task 8: Make the visible grid row-oriented

**Files:**

- Create: `crates/rssh-terminal/src/grid.rs`
- Modify: `crates/rssh-terminal/src/lib.rs`
- Modify: `crates/rssh-terminal/src/parser.rs`
- Test: `crates/rssh-terminal/src/parser.rs`

**Step 1: Write performance RED and row metadata guards**

Add:

```text
full_screen_scroll_does_not_clone_surviving_cells
partial_region_scroll_preserves_exterior_rows
row_rotation_preserves_wrapped_overflow_and_seqno
row_to_history_moves_cells_without_duplicate_clone
```

**Step 2: Verify RED**

Confirm the clone counter reports approximately `lines × surviving_rows × cols`
against the flat grid.

**Step 3: Implement `GridRow` and row rotation**

Move cells, reflow overflow, wrapped state, and change sequence into one row
object. Full-screen scrolling rotates/replaces rows. Partial regions use row
slice rotation. Move exiting row storage into history where ownership permits.

**Step 4: Verify GREEN**

Run:

```powershell
cargo test --locked -p rssh-terminal scroll
cargo test --locked -p rssh-terminal reflow
cargo test --locked -p rssh-terminal kitty
cargo run --locked --release -p rssh-app -- bench --json --workload plain-scroll --bytes 1048576 --chunk-size 8192
```

Expected: zero surviving-cell clones and at least 5 MiB/s plain-scroll on the
approved local baseline.

**Step 5: Commit**

```powershell
git add crates/rssh-terminal/src/grid.rs crates/rssh-terminal/src/lib.rs crates/rssh-terminal/src/parser.rs
git commit -m "perf: rotate terminal rows without cell cloning"
```

### Task 9: Install meaningful performance gates

**Files:**

- Modify: `crates/rssh-app/src/bench.rs`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Create: `docs/performance-baseline.md`
- Test: `crates/rssh-app/src/bench.rs`

**Step 1: Add threshold RED tests**

Test approved algorithmic, ratio, parser, render, idle CPU, and RSS thresholds.
Ensure failure JSON names each violated metric and observed/expected values.

**Step 2: Verify RED**

Run current release workflow command and confirm the old smoke-level thresholds
are rejected by the workflow test/inspection.

**Step 3: Implement gates**

Hosted PR jobs enforce deterministic work and relative ratios. Release and fixed
performance runners enforce the approved absolute budgets and 10-percent
same-machine regression rule.

**Step 4: Verify**

Run:

```powershell
cargo test --locked -p rssh-app bench::tests
cargo run --locked --release -p rssh-app -- bench --json --workload ansi-scroll-query --bytes 1048576 --chunk-size 8192
git diff --check
```

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/bench.rs .github/workflows/ci.yml .github/workflows/release.yml docs/performance-baseline.md
git commit -m "ci: enforce terminal performance budgets"
```

## Milestone 3: Grapheme and Font Pipeline

### Task 10: Introduce grapheme leader/continuation cells

**Files:**

- Create: `crates/rssh-terminal/src/cell.rs`
- Modify: `crates/rssh-terminal/src/lib.rs`
- Modify: `crates/rssh-terminal/src/parser.rs`
- Modify: renderer/app call sites that construct or read `Cell`
- Test: `crates/rssh-terminal/src/parser.rs`

**Step 1: Write Unicode RED tests**

Cover:

- decomposed `e + U+0301`;
- Arabic marks;
- Devanagari conjuncts;
- VS15/VS16;
- skin-tone emoji;
- regional-indicator flags;
- family ZWJ emoji;
- keycaps;
- clusters split across multiple `feed` calls.

Assert stored text, logical columns, continuation cells, cursor movement, copy,
selection, insert/delete, resize, reflow, and scrollback.

**Step 2: Verify RED**

Expected: zero-width and multi-codepoint cluster assertions fail because current
cells store one `char`.

**Step 3: Implement the model**

Use:

```rust
pub enum CellContent {
    Blank,
    Text { grapheme: SmolStr, columns: u8 },
    Continuation { leader_delta: u8 },
}
```

Preserve current style and metadata on the leader. Continuations reference the
leader and never duplicate text. Incremental writes must update the previous
leader when a grapheme extends across feed boundaries.

**Step 4: Migrate callers mechanically, then verify behavior**

Provide temporary accessor methods such as `text()`, `primary_char()`,
`columns()`, and `is_continuation()` to keep the migration reviewable. Do not
retain public mutable `.ch` access.

Run:

```powershell
cargo test --locked -p rssh-terminal
cargo test --locked -p rssh-renderer
cargo test --locked -p rssh-app window::tests
```

**Step 5: Commit**

```powershell
git add crates/rssh-terminal crates/rssh-renderer crates/rssh-app
git commit -m "feat: preserve terminal grapheme clusters"
```

### Task 11: Add deterministic licensed font fixtures

**Files:**

- Create: `tests/fixtures/fonts/README.md`
- Create: `tests/fixtures/fonts/LICENSES/`
- Create: `tests/fixtures/fonts/SHA256SUMS`
- Add: minimal/subset test fonts for Latin ligatures, CJK, Arabic, Devanagari,
  and color emoji
- Test: fixture integrity test in `rssh-fonts`

**Step 1: Write fixture integrity RED**

The test must verify every fixture has an allow-listed license, expected
SHA-256, and the documented glyph/feature coverage.

**Step 2: Verify RED**

Expected: missing fixture directory and manifest.

**Step 3: Add fixtures**

Use redistribution-compatible font files, retain original license text, record
source/version/subsetting commands, and make tests independent of installed
system fonts.

**Step 4: Verify GREEN**

Run the fixture integrity test on Windows and ensure paths are portable.

**Step 5: Commit**

```powershell
git add tests/fixtures/fonts
git commit -m "test: add deterministic shaping font fixtures"
```

### Task 12: Create `rssh-fonts` catalog, shaping, and diagnostics

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/rssh-fonts/Cargo.toml`
- Create: `crates/rssh-fonts/src/lib.rs`
- Create: `crates/rssh-fonts/src/config.rs`
- Create: `crates/rssh-fonts/src/catalog.rs`
- Create: `crates/rssh-fonts/src/shape.rs`
- Create: `crates/rssh-fonts/src/diagnostics.rs`
- Test: modules above

**Step 1: Write shaping RED tests**

Using only fixtures, verify:

- configured primary and ordered fallback;
- whole-cluster fallback;
- ligature feature on/off;
- Arabic and Devanagari cluster ranges;
- Hebrew/Arabic bidi visual order with logical cell mapping;
- CJK double-width mapping;
- emoji VS, skin tone, ZWJ, and flag cluster selection;
- missing family and visible-tofu diagnostics;
- generation and shape-cache invalidation.

**Step 2: Verify RED**

Expected: crate/APIs are missing.

**Step 3: Implement with `cosmic-text 0.19`**

Pin features intentionally. Wrap `FontSystem` behind catalog ownership. Expose
terminal-oriented shaped rows with glyphs, byte ranges, cluster ranges, cell
spans, font IDs, and visual order. Use `Wrap::None`; terminal code owns wrapping.

**Step 4: Verify GREEN**

Run:

```powershell
cargo test --locked -p rssh-fonts
cargo clippy --locked -p rssh-fonts --all-targets -- -D warnings
```

**Step 5: Commit**

```powershell
git add Cargo.toml Cargo.lock crates/rssh-fonts
git commit -m "feat: add terminal font shaping and fallback"
```

### Task 13: Add bounded raster and shape caches

**Files:**

- Create: `crates/rssh-fonts/src/raster.rs`
- Create: `crates/rssh-fonts/src/cache.rs`
- Modify: `crates/rssh-fonts/src/lib.rs`
- Test: new modules

**Step 1: Write RED tests**

Cover monochrome masks, RGBA color glyphs, cache hit/eviction, configured memory
budgets, DPI/zoom invalidation, font-generation invalidation, and corrupt-font
fallback.

**Step 2: Verify RED**

Expected: raster/cache APIs missing.

**Step 3: Implement bounded caches**

Use explicit byte accounting. Never keep an unbounded image or shaped-line map.
Expose counters needed by metrics and performance tests.

**Step 4: Verify GREEN**

Run all `rssh-fonts` tests and a headless render of the fixture specimen.

**Step 5: Commit**

```powershell
git add crates/rssh-fonts
git commit -m "perf: bound font shape and raster caches"
```

### Task 14: Integrate shaped rows into the CPU reference renderer

**Files:**

- Modify: `crates/rssh-renderer/Cargo.toml`
- Create: `crates/rssh-renderer/src/text.rs`
- Modify: `crates/rssh-renderer/src/lib.rs`
- Modify: app render snapshot construction
- Test: `crates/rssh-renderer/src/text.rs`

**Step 1: Write rendering RED tests**

Assert non-background pixels and cluster bounds for Latin ligatures, CJK,
Arabic, Devanagari, combining marks, monochrome fallback, color emoji, tofu,
selection, cursor, and IME preedit. Add a damage test that replaces a multi-cell
ligature and verifies no stale pixels remain.

**Step 2: Verify RED**

Expected: current BASIC_FONTS path skips or misrenders fixture text.

**Step 3: Implement the CPU reference path**

Build shaped rows from immutable render snapshots. Composite mask and RGBA
glyphs with current foreground/faint/blink/selection behavior. Expand damage to
the old/new shaped span.

**Step 4: Verify GREEN**

Run:

```powershell
cargo test --locked -p rssh-renderer
cargo test --locked -p rssh-app renderer
```

**Step 5: Commit**

```powershell
git add crates/rssh-renderer crates/rssh-app
git commit -m "feat: render shaped terminal text"
```

## Milestone 4: Direct GPU Renderer

### Task 15: Establish direct `wgpu 30` surface ownership

**Files:**

- Modify: `crates/rssh-renderer/Cargo.toml`
- Create: `crates/rssh-renderer/src/gpu/mod.rs`
- Create: `crates/rssh-renderer/src/gpu/context.rs`
- Create: `crates/rssh-renderer/src/gpu/metrics.rs`
- Modify: `crates/rssh-app/Cargo.toml`
- Create: `crates/rssh-app/src/window_gpu.rs`
- Modify: `crates/rssh-app/src/window.rs`
- Test: headless adapter tests and native window subprocess tests

**Step 1: Write RED tests**

Require metrics for backend, adapter name/type, software status, surface format,
present mode, and rendered/presented frame counts. Add surface outdated/lost
reconfiguration seam tests.

**Step 2: Verify RED**

Expected: current `pixels` metrics lack adapter and surface evidence.

**Step 3: Implement direct context**

Create and own `wgpu 30` instance/surface/adapter/device/queue. Keep winit main
thread rules. Support platform backends and software fallback. Do not remove
`pixels` until the direct path renders the compatibility framebuffer.

**Step 4: Verify GREEN**

Run one-, ten-, and resize-frame subprocess smokes and assert metrics.

**Step 5: Commit**

```powershell
git add Cargo.lock crates/rssh-renderer crates/rssh-app
git commit -m "feat: own native wgpu presentation"
```

### Task 16: Add GPU background, image, and decoration passes

**Files:**

- Create: `crates/rssh-renderer/src/gpu/quads.rs`
- Create: `crates/rssh-renderer/src/gpu/images.rs`
- Create: `crates/rssh-renderer/src/gpu/render_graph.rs`
- Modify: `crates/rssh-renderer/src/gpu/mod.rs`
- Test: GPU/headless render-graph ordering tests

**Step 1: Write RED ordering tests**

Cover pane/cell background, negative-z images, glyph slot, positive-z images,
underline/strikethrough, cursor, tab bar, overlay, and selection ordering.

**Step 2: Verify RED**

Expected: render graph types are absent.

**Step 3: Implement instanced pipelines**

Use persistent buffers with dirty-range updates. Preserve current Kitty/iTerm/
Sixel clipping and z-order semantics.

**Step 4: Verify GREEN**

Compare the GPU readback specimen to CPU reference invariants with tolerances,
not platform-specific exact system-font pixels.

**Step 5: Commit**

```powershell
git add crates/rssh-renderer/src/gpu
git commit -m "feat: render terminal layers on the GPU"
```

### Task 17: Integrate `glyphon 0.12` GPU glyph atlas

**Files:**

- Modify: `crates/rssh-renderer/Cargo.toml`
- Create: `crates/rssh-renderer/src/gpu/text.rs`
- Modify: `crates/rssh-renderer/src/gpu/render_graph.rs`
- Modify: app renderer configuration wiring
- Test: GPU text and cache tests

**Step 1: Write RED tests**

Verify shaped glyph preparation, monochrome/color atlas entries, bounded atlas
growth, eviction/repack, font-generation invalidation, DPI/zoom invalidation,
custom block glyphs, and full-run damage.

**Step 2: Verify RED**

Expected: text atlas path absent.

**Step 3: Implement `glyphon` integration**

Use the same `cosmic-text` buffers and mapping owned by `rssh-fonts`. Render into
the existing render pass. Keep terminal clipping and cell-span alignment.

**Step 4: Verify GREEN**

Run renderer tests and native ten-frame Unicode specimen.

**Step 5: Commit**

```powershell
git add Cargo.lock crates/rssh-renderer crates/rssh-app
git commit -m "feat: render terminal glyphs from a GPU atlas"
```

### Task 18: Promote direct GPU rendering and remove native `pixels`

**Files:**

- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-renderer/Cargo.toml`
- Remove: native `pixels` integration code
- Test: full native-window E2E and renderer suite

**Step 1: Add promotion guards**

Tests must fail if the native path constructs `Pixels`, uploads a full CPU
framebuffer for normal text, or omits adapter metrics.

**Step 2: Verify RED**

Confirm guards detect the compatibility path.

**Step 3: Switch the default and remove dependency**

Make direct GPU rendering the only normal native path. Retain CPU rendering as a
headless reference command/test facility, not as an implicit per-frame upload.

**Step 4: Verify GREEN**

Run:

```powershell
cargo test --locked --workspace --all-targets
cargo build --locked --release -p rssh-app
```

Run debug/release ten-frame native smokes and device-recovery seam tests.

**Step 5: Commit**

```powershell
git add Cargo.toml Cargo.lock crates/rssh-app crates/rssh-renderer
git commit -m "refactor: promote the direct GPU terminal renderer"
```

## Milestone 5: SSH and Native E2E

### Task 19: Make native SSH sessions full duplex and preserve remote status

**Files:**

- Modify: `crates/rssh-ssh/src/lib.rs`
- Modify: `crates/rssh-ssh/src/russh_client.rs`
- Modify: `crates/rssh-app/src/ssh.rs`
- Test: the same modules

**Step 1: Write RED tests**

Add:

```text
shell_runner_streams_output_before_input_eof
remote_exit_status_is_preserved
remote_exit_signal_is_preserved
native_ssh_runner_returns_remote_exit_status
```

Use bounded channels/duplex streams and prove output arrives while input remains
open.

**Step 2: Verify RED**

Run the four focused tests and confirm the sequencing/status assertions fail.

**Step 3: Implement concurrent pumps and result contract**

Read local input and remote events concurrently. Define a session result that
preserves exit status/signal and maps it intentionally at the app boundary.

**Step 4: Verify GREEN**

Run all SSH/app focused tests with no sleeps or unbounded waits.

**Step 5: Commit**

```powershell
git add crates/rssh-ssh crates/rssh-app/src/ssh.rs
git commit -m "fix: make native SSH sessions full duplex"
```

### Task 20: Give all forwarding modes cancellation-aware lifecycles

**Files:**

- Modify: `crates/rssh-ssh/src/russh_client.rs`
- Modify: `crates/rssh-app/src/ssh.rs`
- Test: both modules

**Step 1: Write RED tests**

Verify dropping/cancelling local, dynamic, and remote forward handles releases
listeners, stops accepting, cancels remote forwarding, and joins within the
deadline.

**Step 2: Verify RED**

Expected: current permanent listeners/tasks fail to stop.

**Step 3: Implement handles**

Return explicit cancellation-aware handles. Drop is a fallback; normal shutdown
must cancel and await completion.

**Step 4: Verify GREEN**

Run focused forwarding lifecycle tests repeatedly.

**Step 5: Commit**

```powershell
git add crates/rssh-ssh/src/russh_client.rs crates/rssh-app/src/ssh.rs
git commit -m "fix: bound SSH forwarding lifecycles"
```

### Task 21: Build the hermetic loopback SSH fixture

**Files:**

- Modify: `crates/rssh-test-support/Cargo.toml`
- Create: `crates/rssh-test-support/src/ssh/mod.rs`
- Create: `crates/rssh-test-support/src/ssh/server.rs`
- Create: `crates/rssh-test-support/src/ssh/agent.rs`
- Create: `crates/rssh-test-support/src/ssh/sftp.rs`
- Create: `crates/rssh-test-support/src/ssh/forward.rs`
- Test: these modules

**Step 1: Write lifecycle/security RED tests**

Cover start/ready/stop, port release, host key generation, temp known-hosts,
path traversal/symlink rejection, loopback-only forwarding, agent identity
injection, and bounded teardown.

**Step 2: Verify RED**

Expected: fixture APIs absent.

**Step 3: Implement the fixture**

Use real TCP and runtime-generated keys. The server executes only white-listed
test commands and records PTY/session/resize/forward events.

**Step 4: Verify GREEN**

Run the fixture tests with `--test-threads=1`, then repeat them ten times without
port leaks.

**Step 5: Commit**

```powershell
git add crates/rssh-test-support
git commit -m "test: add hermetic SSH server fixtures"
```

### Task 22: Add native and system OpenSSH interoperability

**Files:**

- Create: `crates/rssh-ssh/tests/loopback_native.rs`
- Create: `crates/rssh-app/tests/openssh_loopback.rs`
- Create: `crates/rssh-app/tests/transfer_loopback.rs`
- Create: `scripts/ci/openssh-sshd.sh`
- Test: files above

**Step 1: Write RED matrices**

Cover authentication, host-key policy, shell/exec/PTY/resize/status, forwarding,
SFTP, SCP, recursive transfer, and SHA-256 content verification. Linux tests
must also use an isolated real OpenSSH `sshd`.

**Step 2: Verify RED**

Expected: tests fail before server and lifecycle wiring is complete.

**Step 3: Implement only required adapters/config**

Use temporary HOME/config/known-hosts. Do not read user SSH state. Do not use an
external container image for the required sshd gate.

**Step 4: Verify GREEN**

Run:

```powershell
cargo test --locked -p rssh-ssh --test loopback_native -- --test-threads=1
cargo test --locked -p rssh-app --test openssh_loopback -- --test-threads=1
cargo test --locked -p rssh-app --test transfer_loopback -- --test-threads=1
```

**Step 5: Commit**

```powershell
git add crates/rssh-ssh/tests crates/rssh-app/tests scripts/ci
git commit -m "test: verify SSH and OpenSSH interoperability"
```

### Task 23: Make real PTY tests deterministic and required

**Files:**

- Modify: `crates/rssh-pty/src/lib.rs`
- Modify: `crates/rssh-app/tests/local_pty.rs`
- Modify: `crates/rssh-app/src/window_restart_pane_tests.rs`
- Test: all formerly ignored tests

**Step 1: Replace sleeps with RED readiness assertions**

Add explicit ready/marker/exit deadlines and RAII cleanup. Keep one default-shell
smoke; use deterministic non-interactive platform commands elsewhere.

**Step 2: Verify RED**

Run all ignored tests and capture current timeout/hang behavior.

**Step 3: Fix lifecycle and remove `#[ignore]`**

No read, wait, kill, or join may be unbounded.

**Step 4: Verify GREEN**

Run the PTY group ten times and the quick-exit stress 100 times locally.

**Step 5: Commit**

```powershell
git add crates/rssh-pty/src/lib.rs crates/rssh-app/tests/local_pty.rs crates/rssh-app/src/window_restart_pane_tests.rs
git commit -m "test: make platform PTY coverage required"
```

### Task 24: Add six-target native GUI/PTY/SSH CI

**Files:**

- Modify: `.github/workflows/ci.yml`
- Create: `.github/workflows/nightly.yml`
- Create: `crates/rssh-app/tests/native_window_e2e.rs`
- Create: `scripts/ci/run-native-window.ps1`
- Create: `scripts/ci/run-native-window.sh`
- Test: workflow and subprocess tests

**Step 1: Write native-window RED**

Run ten frames with a deterministic marker. Assert true present count, PTY bytes,
first byte/cell timings, adapter/backend/surface metrics, clean exit, and no
stack overflow.

**Step 2: Verify RED**

Run locally on Windows debug and release. Expected: missing metrics/test wiring
fails before implementation is complete.

**Step 3: Add the matrix**

PR required:

```text
windows-2025
ubuntu-24.04
macos-15
```

Nightly/release required:

```text
windows-11-arm
ubuntu-24.04-arm
macos-15-intel
```

Linux runs Xvfb/X11 and Weston/Wayland separately. Every job validates
`version --json` target identity and PTY backend.

**Step 4: Verify locally and statically**

Run PowerShell scripts locally. Validate workflow YAML and ensure all jobs use
locked/all-target commands and read-only permissions.

**Step 5: Commit**

```powershell
git add .github/workflows crates/rssh-app/tests/native_window_e2e.rs scripts/ci
git commit -m "ci: run native terminal E2E on six targets"
```

## Milestone 6: Packaging and Completion

### Task 25: Build and certify six native release artifacts

**Files:**

- Modify: `.github/workflows/release.yml`
- Create/modify: `packaging/` platform manifests and launchers
- Create: `scripts/ci/package-smoke.ps1`
- Create: `scripts/ci/package-smoke.sh`
- Modify: `docs/release-console.md`
- Test: packaged artifact smokes

**Step 1: Add RED package assertions**

Require each artifact to run version, doctor, self-test, benchmark gates,
loopback SSH, font fixture specimen, and ten-frame GUI present.

**Step 2: Verify RED**

Expected: current release creates only one Windows x64 console ZIP.

**Step 3: Add six artifacts and protected publication**

Produce Windows x64/ARM64, Linux x64/ARM64, and macOS x64/ARM64 packages. Build
and test jobs use read-only permissions. Signing/notarization/SBOM/provenance
run only in protected release environments.

**Step 4: Verify local package path**

Build and smoke the current Windows artifact locally. Validate all target jobs
and artifact names statically.

**Step 5: Commit**

```powershell
git add .github/workflows/release.yml packaging scripts/ci docs/release-console.md
git commit -m "release: certify six native terminal artifacts"
```

### Task 26: Final documentation, conformance, and full verification

**Files:**

- Modify: `README.md`
- Modify: `docs/architecture.md`
- Modify: `docs/research/wezterm-parity-gap.md`
- Modify: `docs/performance-baseline.md`
- Modify: the design/implementation plan status sections

**Step 1: Re-read the approved design**

Create a requirement checklist mapping every approved requirement to a commit,
test, metric, or native CI job.

**Step 2: Run fresh verification**

Run:

```powershell
cargo +1.89.0 fmt --all -- --check
cargo +1.89.0 clippy --locked --workspace --all-targets -- -D warnings
cargo +1.89.0 test --locked --workspace --all-targets
cargo +1.89.0 build --locked --release -p rssh-app
cargo +1.89.0 run --locked --release -p rssh-app -- bench --json --workload plain-scroll --bytes 1048576 --chunk-size 8192
cargo +1.89.0 run --locked --release -p rssh-app -- bench --json --workload ansi-scroll-query --bytes 1048576 --chunk-size 8192
git diff --check
```

Run debug/release native-window, PTY, loopback SSH, and Windows packaged smokes.

**Step 3: Update documentation with observed evidence**

Do not claim Linux/macOS/ARM64 runtime support until the corresponding native CI
jobs have executed successfully. Distinguish locally verified, hosted verified,
and self-hosted certified results.

**Step 4: Request final independent review**

Review the entire range from the design commit to HEAD for spec compliance,
correctness, security, performance, and production readiness. Resolve all
Critical and Important findings.

**Step 5: Commit**

```powershell
git add README.md docs
git commit -m "docs: record production parity verification"
```

## Completion Criteria

- Windows debug GUI subprocess exits cleanly without stack overflow.
- Query scanning is linear and approved throughput/latency budgets pass.
- History eviction and full-screen scroll do not relocate/clone survivors.
- Terminal cells preserve complete grapheme clusters across chunk boundaries,
  reflow, history, selection, and copy.
- CJK, Arabic, Indic, bidi, ligatures, fallback, and color emoji are rendered
  through the configured font stack.
- Native GUI uses direct `wgpu` with a bounded GPU glyph atlas and reports real
  adapter/surface metrics.
- Native SSH is full duplex, preserves remote status, and cancels forwarding.
- Real PTY tests are no longer ignored.
- Hermetic SSH and independent OpenSSH interoperability pass.
- PR and nightly/release matrices cover all six native OS/architecture targets.
- Full workspace tests, Clippy, formatting, release build, performance gates,
  native-window smoke, and package smoke are fresh and green.
