# Production Parity Foundation Design

**Date:** 2026-07-28

**Status:** Approved

**Branch:** `codex/production-parity-foundation`

## Implementation and Verification Status (2026-08-02)

Tasks 1–25 are committed through `5744013e`. The forwarding-lifecycle,
hermetic SSH, OpenSSH interoperability, deterministic PTY, six-target workflow,
and packaging slices are recorded by `2d700785`, `afa3df11`, `93cc2ec2`,
`3bb2dd3d`, `7efb6886`, and `5744013e`; the shared process-harness evidence fix
is `83ade73a`; the Linux hosted OpenSSH dependency and service guard correction
is `74f78bab`, extended to both release package-smoke paths by `f02acff6`.
Task 26 local verification, documentation, static checks, and
independent reviews are complete in this documentation change. The
detailed requirement-to-evidence ledger is maintained in
[Production Parity Verification](../production-parity-verification.md).

| Evidence scope | Status | Boundary |
| --- | --- | --- |
| Windows x64 workspace, focused SSH/PTY/native-window checks, and local package smoke | verified locally on Windows x64 | Evidence is local to the 2026-08-02 verification session and does not establish another OS or architecture. |
| Linux x64, macOS ARM64, Windows ARM64, Linux ARM64, and macOS x64 native jobs | defined in hosted workflow but not run in this local session | Workflow definitions are not successful runtime results. Commits `74f78bab` and `f02acff6` guard the native and release Linux `openssh-server` installs, but the independent real `sshd` and package paths were not run on the Windows host and have no linked hosted result. |
| Protected performance comparison, signing, notarization, SBOM, provenance/attestation, and publication | requires protected/self-hosted environment | No protected artifact or performance result was produced in this local session. |
| Hardware IME, Intel/AMD/NVIDIA/WARP/Mesa/Metal, RDP, and multi-DPI/HiDPI/Retina certification | not yet evidenced | These remain separate certification obligations. |

This status does not claim that all six native targets passed or that R-SSH has
100% WezTerm parity. The bounded compatibility work in this design remains
subject to the explicit non-goals and the current
[WezTerm parity gap tracker](../research/wezterm-parity-gap.md).

## Goal

Raise R-SSH from a Windows-focused beta implementation to a production-oriented
terminal foundation with:

1. a reliable Windows debug GUI startup path;
2. scalable terminal query handling, scrolling, and bounded history;
3. real grapheme storage, font shaping, fallback, color emoji, and GPU glyph
   rendering;
4. hermetic SSH, PTY, font, and native-window end-to-end coverage across
   Windows, Linux, and macOS on x64 and ARM64.

This design preserves the existing terminal protocol, pane, selection, image,
and app-shell behavior while replacing the internal bottlenecks and placeholder
font path behind compatibility seams.

## Approved Decisions

- Use a staged migration rather than a big-bang rewrite or embedding WezTerm's
  internal crates.
- Raise the workspace MSRV from Rust 1.85 to Rust 1.89.
- Use `cosmic-text 0.19` for shaping, fallback, bidirectional layout, and color
  glyph rasterization.
- Use `glyphon 0.12` and `wgpu 30` for the production GPU glyph atlas and
  presentation path.
- Replace the native `pixels` path after the direct `wgpu` path passes
  deterministic and native E2E gates. Keep a CPU renderer only as a headless
  reference and diagnostic fallback.
- Run Windows x64, Linux x64, and macOS ARM64 as required PR targets. Run
  Windows ARM64, Linux ARM64, and macOS x64 as required nightly/release targets.
- Keep hardware/IME/GPU-vendor certification separate from deterministic hosted
  CI and run it on protected self-hosted machines.

## Pre-implementation Evidence and Root Causes

The following evidence and present-tense descriptions capture the
pre-implementation baseline investigated on 2026-07-28. They are retained as
the rationale for the approved design; they are not a statement of the
2026-08-02 implementation state. Current implementation evidence is recorded in
[Production Parity Verification](../production-parity-verification.md).

### Windows debug GUI stack overflow

The failure is not recursive. On Windows debug builds, three nested stack frames
consume almost the full default 1 MiB main-thread stack before the innermost
function body runs:

| Function | Debug stack frame |
| --- | ---: |
| `window::run` | 617,680 bytes |
| `configured_startup_app_with_constructor` | 365,440 bytes |
| `validate_cli_config_overrides` | 59,616 bytes |
| Total | 1,042,736 bytes |

The large frames are caused by deeply nested by-value startup state, including
`NativeConfigOverrides`, `NativeConfigLifecycle`, `NativeWindowApp`,
`ConfiguredStartupApp`, and `NativeWindowManager`. Release optimization removes
enough temporaries to hide the issue. The state-report path already avoids a
similar failure by using an 8 MiB worker, but the normal winit event loop must
remain on the platform main thread.

The fix must therefore reduce the actual type and frame sizes through grouped
heap-owned state. Increasing the executable stack may be retained only as a
defense-in-depth guard, not as the root fix.

### Long-output performance

There are three independent hotspots:

1. The query-heavy benchmark repeatedly rescans the remaining chunk for dozens
   of fixed and dynamic terminal queries and drains the buffer prefix after each
   match. Measured throughput collapses as chunk size increases, which is
   evidence of near-quadratic work per chunk.
2. The flat `TerminalGrid` clones almost every surviving cell for a one-line
   full-screen scroll.
3. Bounded history is a `Vec<ScrollbackLine>` and performs `drain(..1)` after
   reaching the 3,500-row limit, then repeatedly rebases selection, image,
   semantic-zone, placeholder, and attachment metadata.

The performance work must address all three rather than treating the history
container as the only bottleneck.

### Font and text correctness

`Cell` and `RenderCell` currently store one `char`. Zero-width characters are
discarded, and variation selectors only mutate width state. Consequently,
combining marks, emoji ZWJ sequences, skin tones, flags, and complex-script
clusters can be lost before a shaper sees them.

The native renderer uses `font8x8::BASIC_FONTS`; unsupported characters are
silently skipped. Font family, fallback, rule, feature, locator, and shaper
configuration is retained in app state but is not applied to rendering.

A font library swap alone is insufficient. The terminal cell model must first
preserve complete extended grapheme clusters.

### E2E and SSH correctness

The current native SSH path has three blockers that hermetic E2E would expose:

- local input is consumed to EOF before remote output is read, so an interactive
  shell is not truly full duplex;
- remote `ExitStatus` and `ExitSignal` are discarded and the app returns zero;
- local, dynamic, and remote forwarding tasks have no complete cancellation and
  bounded-join lifecycle.

The current real PTY tests are ignored and use fixed sleeps or potentially
unbounded waits. CI only runs on Windows and does not provide native runtime
evidence for Linux, macOS, or ARM64.

## Architecture

### `rssh-terminal`

Introduce a grapheme-aware cell model:

```text
CellContent
├─ Blank
├─ Text { grapheme, columns }
└─ Continuation { leader_delta }
```

- `grapheme` stores the complete extended grapheme cluster.
- `columns` is terminal logical width and is determined by Unicode version,
  variation selector, ambiguous-width policy, and configured width overrides.
- Shaping advances never change cursor movement, selection, reflow, or PTY
  rows/columns.
- Leader and continuation cells are indivisible for insert/delete, erase,
  selection, copy, resize, and reflow.

Replace the flat grid with row objects:

```text
GridRow
├─ cells
├─ reflow_overflow
├─ wrapped
└─ last_change_seqno
```

Use `VecDeque<ScrollbackLine>` for bounded history. External code observes only
logical history indexes, never physical deque positions. Full-screen scrolling
rotates or replaces rows instead of cloning surviving cells.

### `rssh-fonts`

Add a dedicated workspace crate:

- `config`: family lists, fallback, weight/style/stretch, features, font rules,
  and terminal-specific bidi options;
- `catalog`: `cosmic_text::FontSystem`, system and configured directory loading,
  generation tracking, and deterministic test fonts;
- `shape`: UTF-8 row creation, byte/cluster/cell maps, style runs, bidi visual
  order, terminal-span alignment, and shaped-line caching;
- `raster`: CPU masks/RGBA glyphs for headless tests and emergency diagnostics;
- `diagnostics`: deduplicated missing family, missing cluster, corrupt font, and
  fallback reporting.

Primary font metrics determine cell width, ascent, descent, and baseline.
`cell_width` and `line_height` remain multiplicative configuration controls.
Configured terminal width always wins over proportional glyph advance.

### `rssh-renderer`

Keep two explicit implementations:

- a deterministic CPU reference renderer for headless tests, golden artifacts,
  and device-loss diagnostics;
- a production GPU renderer that owns the `wgpu` instance, surface, adapter,
  device, queue, surface configuration, render pipelines, and caches.

The GPU renderer uses:

- `glyphon` for shaped glyph preparation and the GPU glyph atlas;
- instanced quads for pane/cell backgrounds, selection, cursor, underlines,
  strikethrough, and custom block glyphs;
- textured quads for iTerm, Kitty, Sixel, background, and color-image layers;
- explicit render buckets that preserve the current image z-order relative to
  text and overlays.

Native rendering migrates away from `pixels`; the CPU framebuffer is no longer
uploaded every frame. Dirty rows update only affected instance and text data.

### `rssh-app`

- Break large startup configuration into grouped heap-owned structures.
- Store shared immutable effective configuration behind `Arc`.
- Keep the winit event loop and surface creation on the platform main thread.
- Expose real adapter, backend, device type, surface format, present mode, and
  software-adapter status in metrics.
- Recreate surface state after outdated/lost errors and rebuild GPU state from
  immutable terminal snapshots after device loss.

### `rssh-test-support`

Add a dev-only workspace crate providing:

- deadline-aware subprocess and child guards;
- deterministic PTY marker helpers;
- isolated temporary HOME, OpenSSH config, and known-hosts helpers;
- a real TCP loopback SSH server;
- sandboxed SFTP storage;
- an injectable in-memory SSH agent;
- loopback echo targets for forwarding;
- cancellation-aware server/listener handles.

All fixtures bind `127.0.0.1:0`, use per-test temporary directories and keys,
and cleanly kill/wait/join on failure or timeout.

## Data Flow and Invariants

```text
PTY bytes
→ single-pass streaming query scanner
→ grapheme terminal cells
→ row-oriented grid and deque history
→ immutable dirty-row snapshots
→ font/style/bidi runs
→ cosmic-text shaping and cluster/cell maps
→ GPU background/image/text/overlay passes
→ wgpu surface present
```

Invariants:

- Unknown or incomplete terminal sequences remain buffered or pass to the VT
  parser unchanged; chunk boundaries cannot alter query behavior.
- Stable row identity advances exactly once for each pruned row.
- `CellAttachment` parent/source identity is immutable; only destination
  coordinates rebase after pruning or scrolling.
- Active alternate-screen and dormant main-screen attachments are rebased
  consistently.
- Bidi changes visual order only. Cursor, selection, copy, and PTY byte order
  remain logical and use the cluster/cell map.
- A shaping run ends at font-rule, style, color, cursor, or selection boundaries.
- Damage covers the complete old and new shaped span. A ligature or complex
  cluster cannot leave pixels outside the dirty region.

## Error Handling and Fallback

Font fallback order:

```text
font-rule primary
→ configured fallback families
→ platform/system fallback
→ bundled licensed emergency font
→ visible tofu
```

- Missing families and clusters are warnings, not crashes.
- Diagnostics are deduplicated by font generation and cluster.
- Font reload advances the generation and invalidates shape/raster caches.
- Glyph atlas exhaustion evicts least-recently-used entries or repacks within a
  configured budget; it does not grow without bound or panic.
- Surface loss recreates surface configuration.
- Device loss recreates the adapter/device and restores render state from the
  terminal snapshot.
- Software adapters such as WARP or llvmpipe are permitted but explicitly
  reported.
- SSH, forwarding, PTY, and subprocess operations always have cancellation,
  operation deadlines, and bounded joins.

## Performance Budgets

### Deterministic algorithmic gates

- Query scanner inspected bytes are no more than four times input bytes.
- The query-heavy 16 KiB/512 B chunk throughput ratio is at least 0.70.
- Full-screen scrolling does not clone surviving cells.
- History eviction does not relocate surviving rows.
- One batch of history pruning performs at most one metadata rebase.

### Runtime budgets

- Query-heavy parser throughput: at least 1 MiB/s.
- Plain scrolling throughput: at least 5 MiB/s.
- 8 KiB parser chunk p95: at most 5 ms initially, then 2 ms after stabilization.
- GPU render/present p95: at most 16 ms; 8.3 ms is the 120 Hz target.
- Input-to-present p95: at most 25 ms.
- First visible cell: at most 500 ms.
- Idle CPU: at most 3 percent.
- Resident memory: at most 256 MiB for the default configuration.
- Shape, line, image, and glyph caches have explicit configurable capacities.

Hosted CI enforces algorithmic work and relative ratios. Fixed self-hosted
performance machines run two warmups and seven samples, compare medians on the
same machine class, and block regressions greater than 10 percent.

## E2E and CI

### Required PR targets

- Windows x64: `windows-2025`
- Linux x64: `ubuntu-24.04`
- macOS ARM64: `macos-15`

### Required nightly/release targets

- Windows ARM64: `windows-11-arm`
- Linux ARM64: `ubuntu-24.04-arm`
- macOS x64: `macos-15-intel`

Each native target verifies:

- exact target and PTY backend identity;
- formatting, Clippy, locked all-target tests, and release build;
- deterministic font/shaping/fallback fixtures;
- local PTY lifecycle;
- loopback native SSH;
- a real ten-frame GUI surface present with PTY marker and render metrics;
- packaged-binary version, doctor, self-test, and smoke behavior.

Linux runs X11 under Xvfb and Wayland under a headless Weston compositor.

### SSH matrix

Hermetic loopback tests cover:

- password, Ed25519, RSA, encrypted key, and agent authentication;
- unknown-host rejection, TOFU, known-host match, and host-key rotation;
- full-duplex shell, exec stdout/stderr, exit zero/non-zero/signal;
- initial PTY size, resize, keepalive, EOF, disconnect, and timeout;
- local, SOCKS5 dynamic, and remote forwarding plus cancellation;
- SFTP and SCP upload/download/recursive operations with digest verification.

Linux additionally starts an isolated local OpenSSH `sshd` to verify native
russh against an independent server implementation. System `ssh`, `sftp`, and
`scp` also connect to controlled fixtures.

### Hardware certification

Protected self-hosted workflows validate:

- Windows Pinyin, Linux IBus/Fcitx, and macOS Chinese/Japanese IME;
- Intel, AMD, NVIDIA, WARP, Mesa, and Metal paths;
- RDP, multi-DPI, HiDPI/Retina, and real input/focus behavior;
- signed and notarized packages.

Hosted jobs test synthetic IME preedit/commit and candidate rectangle behavior;
they do not claim real input-method certification.

## Security

- Build and test jobs have `contents: read`.
- `pull_request_target` is not used.
- Fork PRs never access self-hosted certification machines or signing secrets.
- Test SSH services bind loopback only and do not access real user SSH state.
- Runtime keys and passwords are not logged or uploaded.
- Actions are pinned to commit SHAs.
- Failure artifacts contain only redacted event logs, metrics, adapter data, and
  framebuffer output.
- Signing, notarization, SBOM, and provenance run only in a protected release
  environment.

## Release Gate

Release artifacts:

- Windows x64 and ARM64
- Linux x64 and ARM64
- macOS x64 and ARM64

All six artifacts must execute natively and pass package smoke, loopback SSH,
font fixtures, true performance gates, and ten-frame GUI present checks before
publishing. Cross-compilation alone is not evidence of runtime support.

## Migration Order

1. Test harness, CI foundation, and Windows debug GUI stack fix.
2. Single-pass query scanner.
3. Deque history and row-oriented grid.
4. Grapheme/continuation cell model.
5. `rssh-fonts` and deterministic CPU shaping/raster reference.
6. Direct `wgpu`/`glyphon` native renderer and `pixels` removal.
7. Native SSH duplex, exit status, and forwarding lifecycle fixes.
8. Loopback SSH, OpenSSH interoperability, PTY, and GUI E2E.
9. Six-target packaging, hardware certification, and performance baseline
   promotion.

Each migration slice uses test-first development and remains independently
reviewable. The compatibility path is removed only after the replacement passes
focused tests, the full workspace suite, and the applicable native E2E jobs.

## Non-goals

This program does not add a general Lua VM, WezTerm mux/domain runtime, or every
unrelated WezTerm action. It builds the production performance, text, rendering,
SSH correctness, and platform-verification foundation requested here.
