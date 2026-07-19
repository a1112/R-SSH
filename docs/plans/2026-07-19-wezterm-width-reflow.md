# WezTerm-Compatible Width Reflow Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Match pinned WezTerm width-resize behavior by reflowing main-screen
logical lines and scrollback, preserving alternate-screen non-reflow semantics,
and rebuilding affected UI presentations without stale physical coordinates.

**Architecture:** Teach `rssh-terminal` to rebuild the main physical stream
from soft-wrapped logical lines on a width change while retaining the alternate
screen's existing physical-row resize behavior. Return a narrow resize outcome
to `rssh-app`; use it to clear ordinary selection and rebuild the owner-local
Copy/Search/Quick derived state. Do not introduce cross-reflow text-identity
selection mapping.

**Tech Stack:** Rust 2024, `rssh-terminal`, `rssh-app`, pinned WezTerm at
`refs/wezterm` commit `093bf6bf2b82b929ed80c04fd54ebc80464f715e`, Cargo test.

---

## Ground rules

- Match the fixed upstream behavior, not the prior R-SSH width-only source
  coordinate preservation tests.
- Main screen includes scrollback and reflows even while saved behind the
  alternate screen. The alternate screen never reflows.
- A normal selection is invalid after main reflow. Copy/Search/Quick modes
  survive but all physical-coordinate-derived presentation is rebuilt.
- Use one implementation subagent at a time. After every task: root checks
  evidence, then fresh spec review, then fresh quality review. The implementer
  fixes every Critical/Important issue and the same reviewer rechecks it.
- Run `cargo fmt --all -- --check` and the stated diff check before each commit.

### Task 1: Model main-screen physical streams and reflow logical lines

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`
- Test: `crates/rssh-terminal/src/parser.rs` test module

**Step 1: Write failing terminal tests**

Add tests that emit soft-wrapped main text at width 4, resize through widths
5 and 6 and back to 4, then assert the complete main scrollback-plus-grid
physical stream, `wrapped` boundaries, and textual content. Add a second test
that includes a hard newline and proves it remains a logical-line boundary.

**Step 2: Run tests to verify RED**

Run:

```text
cargo test -p rssh-terminal terminal_main_width_resize_reflows_soft_wrapped_lines -- --exact
cargo test -p rssh-terminal terminal_main_width_resize_preserves_hard_breaks -- --exact
```

Expected: FAIL because the existing rectangular grid resize clips the old rows
and never changes scrollback rows.

**Step 3: Add internal stream helpers**

In `parser.rs`, add private helpers to:

1. collect main scrollback then main grid into a physical stream preserving
   cell/style/sequence/wrapped metadata;
2. join only physical rows connected by `wrapped = true`;
3. pack each resulting logical line to a target width, respecting wide glyph
   lead and continuation cells; and
4. partition the output back into bounded scrollback plus an exact-height main
   grid padded with blank rows.

All rebuilt rows must carry the resize `seqno` and be dirty.

**Step 4: Make RED tests green**

Change `Terminal::resize` so a nonzero column change reflows the main physical
stream. Preserve current behavior for a same-width size change.

**Step 5: Add edge RED/GREEN tests**

Add and pass tests for width zero, shrink/expand with a two-cell Unicode glyph,
and a custom width override. A continuation blank must never be detached or
copied as an independent glyph.

**Step 6: Verify and commit**

```text
cargo test -p rssh-terminal
cargo fmt --all -- --check
git diff --check
git add crates/rssh-terminal/src/parser.rs
git commit -m "feat: reflow main terminal lines on width resize"
```

### Task 2: Map terminal state and preserve alternate-screen semantics

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`
- Test: `crates/rssh-terminal/src/parser.rs` test module

**Step 1: Write failing tests**

Add focused tests proving:

- a cursor inside a soft-wrapped main logical line, including trailing default
  padding, retains its logical x offset after shrink and expand; and a cursor
  reflowed above the resized viewport clamps to visible row zero, matching
  WezTerm's post-resize `set_cursor_pos` behavior;
- saved main state reflows while the alternate screen is active, and restoring
  main shows reflowed content;
- alternate rows narrow by physical truncation and widen without joining rows;
- pending wrap and saved cursor do not point at unrelated cells afterward.

**Step 2: Run RED tests**

```text
cargo test -p rssh-terminal terminal_width_resize_ -- --nocapture
```

Expected: failures in cursor placement and alternate behavior before mapping is
implemented.

**Step 3: Implement terminal-state mapping**

Extend the internal reflow operation with the logical cursor mapping required
by active and saved main state. Retain cursor x through trailing padding, then
convert the mapped physical row back to a visible row and clamp it to the
resized viewport exactly as upstream does when reflow pushes it into
scrollback. Update saved cursor, pending-wrap/NFC position, scrollback offset,
and semantic row metadata through the map, or retire an item when it has no
valid retained mapping. Apply the operation to saved main state even when
alternate is active. Keep alternate-grid behavior as the existing
per-physical-row resize path.

**Step 4: Make image/kitty coordinate handling safe**

For each main-screen inline-image/kitty coordinate container, map it only when
the underlying logical cell maps; otherwise remove the placement. Add a test
that proves a stale placement is retired rather than rendered at an unrelated
cell. Do not alter alternate-screen placement identity on a width-only resize.

**Step 5: Verify and commit**

```text
cargo test -p rssh-terminal
cargo fmt --all -- --check
git diff --check
git add crates/rssh-terminal/src/parser.rs
git commit -m "fix: preserve terminal state across main reflow"
```

### Task 3: Surface a reflow outcome through runtime and retire stale ordinary UI

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`
- Modify: `crates/rssh-app/src/terminal_runtime.rs`
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/window.rs` test module

**Step 1: Write failing app tests**

Create active and inactive-pane fixtures with main ordinary selection and a
main viewport. Resize width through `handle_window_resize`, then assert:

- main content reflowed;
- ordinary selection is cleared instead of highlighting/copying old cells;
- viewport is reset/clamped to the post-reflow main range;
- alternate-only resize leaves ordinary owner state on the existing path.

**Step 2: Run RED test**

```text
cargo test -p rssh-app window_app_main_reflow_ -- --nocapture
```

Expected: old source endpoints are retained and incorrectly project onto the
reflowed grid.

**Step 3: Add a narrow public resize outcome**

Expose `TerminalResizeOutcome` from `Terminal::resize` and pass it through
`TerminalRuntime::resize`. It should distinguish main reflow from alternate
physical resize without leaking an arbitrary old-cell mapping API.

**Step 4: Apply outcome before reconciliation**

In `NativeWindowApp::handle_window_resize`, obtain one outcome for active and
each inactive pane. On main reflow clear ordinary selection, reset the main
viewport, clear cached presentation, rebuild snapshots, then run normal
reconciliation. Do not use height-change identity retirement for this case;
screen switches/resets retain their existing retirement semantics.

**Step 5: Verify and commit**

```text
cargo test -p rssh-app window_app_main_reflow_ -- --nocapture
cargo test -p rssh-app window_app_width_only_ -- --nocapture
cargo fmt --all -- --check
git diff --check
git add crates/rssh-terminal/src/parser.rs crates/rssh-app/src/terminal_runtime.rs crates/rssh-app/src/window.rs
git commit -m "feat: reconcile ordinary pane UI after main reflow"
```

### Task 4: Rebuild Copy/Search/Quick state after main reflow

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/window.rs` test module

**Step 1: Write failing tests**

For active and inactive main panes, enter each of Copy, Search, and Quick
Select; create a match/selection where applicable; then resize width. Assert:

- the owner keeps the same overlay variant and Search query/editing state;
- no old Copy source endpoint or old Search/Quick match can be selected or
  copied;
- Search re-finds matching text in its new physical rows;
- Quick Select recomputes candidates and deterministic labels from the
  reflowed content; and
- alternate-only resize does not invoke this main-reflow reset path.

**Step 2: Run RED tests**

```text
cargo test -p rssh-app window_app_main_reflow_overlay_ -- --nocapture
```

Expected: the current width-only logic preserves old stable source endpoints
and cached matches.

**Step 3: Add owner-local reflow reconciliation**

Add a `PaneUiState` method called only for main reflow. It must preserve only
mode/configuration, clear every coordinate-derived Copy selection/current
match/Quick label and cache, and schedule/rebuild the existing search/quick
derivations against the reflowed terminal. Rebuild the selection-free base
snapshot and repaint each affected visible pane.

**Step 4: Make RED tests green and cover safety**

Add a copy action assertion that cannot return text from the old physical cell
after reflow. Cover a resize sequence while the pane is inactive, then focus
it and verify it presents only regenerated state.

**Step 5: Verify and commit**

```text
cargo test -p rssh-app window_app_main_reflow_overlay_ -- --nocapture
cargo test -p rssh-app window_app_quick_ -- --nocapture
cargo test -p rssh-app window_app_copy_ -- --nocapture
cargo fmt --all -- --check
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "fix: rebuild pane overlays after main reflow"
```

### Task 5: Record bounded parity and perform full verification

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/mvp-6-app-shell-v1.md`
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Update the three authoritative documents**

Move the exact main-screen reflow/alternate non-reflow slice from the next
gap to completed evidence. State normal selection invalidation and
Copy/Search/Quick recomputation precisely. Do not claim general selection
parity, general renderer parity, or arbitrary image reflow beyond the tested
safe-retirement policy. Name the next concrete remaining gap.

**Step 2: Verify upstream evidence**

```text
git -C refs/wezterm rev-parse HEAD
rg -n "rewrap_lines|allow_scrollback|alternate" refs/wezterm/term/src/screen.rs
rg -n "check_for_dirty_lines_and_invalidate_selection|CopyOverlay|QuickSelectOverlay" refs/wezterm/wezterm-gui/src/termwindow/mod.rs
rg -n "check_for_resize|update_search" refs/wezterm/wezterm-gui/src/overlay/copy.rs refs/wezterm/wezterm-gui/src/overlay/quickselect.rs
```

Expected: fixed commit `093bf6bf...` and every behavior symbol found.

**Step 3: Run the complete acceptance matrix**

```text
cargo test -p rssh-terminal
cargo test -p rssh-app
cargo test --workspace -q
cargo fmt --all -- --check
git diff --check codex/wezterm-parity-progress..HEAD
```

**Step 4: Commit documentation**

```text
git add docs/architecture.md docs/mvp-6-app-shell-v1.md docs/research/wezterm-parity-gap.md
git commit -m "docs: record WezTerm width reflow parity"
```

## Final review and integration

Run a fresh final spec review against this design and all five tasks, then a
fresh quality review against the complete target diff. Resolve all
Critical/Important findings and re-review them. Re-run the full acceptance
matrix on the feature branch, fast-forward merge it locally into
`codex/wezterm-parity-progress`, repeat the full matrix on the merged target,
then remove only the clean feature worktree and merged feature branch.
