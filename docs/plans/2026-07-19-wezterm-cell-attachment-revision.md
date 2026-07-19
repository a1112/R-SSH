# CellAttachment Revision Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace transient default-geometry image fragments with persistent,
geometry-independent CellAttachment state that can exactly survive bounded cell
transforms.

**Status (2026-07-20):** Completed for the bounded vertical/line and
single-row `ICH`/`DCH` operations in this plan. The remaining scope of the
repository is not implied by this status.

**Architecture:** A physical placement creates one attachment per declared
terminal cell, carrying parent identity and logical source-cell coordinates.
`CellTransform` moves or deletes attachments alongside text cells. Renderer
snapshots preserve attachment mappings and resolve their pixel rectangles only
at render time from the actual geometry; they never recreate attachment
identity from parent origins or target offsets.

For virtual Kitty placements, a narrow per-placement marker selects the live
attachment origin only after a bounded character edit creates a conflict with a
residual cache origin. Its resize/rebase, alternate-screen, and delete
lifecycle is coupled to the placement; it does not change stored payload or
explicit-delete behavior.

**Tech Stack:** Rust, `rssh-terminal`, `rssh-renderer`, terminal/renderer unit
tests.

---

### Task 0: Replace the transient fragment contract

**Files:**
- Modify: `crates/rssh-terminal/src/lib.rs`
- Modify: `crates/rssh-terminal/src/parser.rs`
- Modify: `crates/rssh-renderer/src/lib.rs`
- Test: existing graphics-fragment terminal and renderer tests

**Step 1: Write failing tests**

Create a placement whose logical 2x2 cells are stable across 8x16 and 10x20
render geometries. Mutate/delete one attachment in a snapshot and assert full
and damage renders use precisely the surviving attachment set at both
geometries.

**Step 2: Verify RED**

Run: `cargo test -p rssh-renderer cell_attachment -- --nocapture`

Expected: current transient runtime reconstruction cannot preserve mutation or
deletion across geometry changes.

**Step 3: Implement the persistent model**

Add a public attachment type with stable parent/source logical cell identity
and destination row/column. Store it in `Terminal` and `ScreenState`; construct
it on physical placement creation and remove/rebase it with existing placement
lifecycle operations. Replace snapshot fragment authority with attachments.

**Step 4: Verify GREEN**

Run focused terminal and renderer attachment tests, then terminal and renderer
full suites.

**Step 5: Commit**

```powershell
git add crates/rssh-terminal/src/lib.rs crates/rssh-terminal/src/parser.rs crates/rssh-renderer/src/lib.rs
git commit -m "feat: persist inline image cell attachments"
```

### Task 1: Render attachments at runtime geometry

**Files:**
- Modify: `crates/rssh-renderer/src/lib.rs`
- Test: `crates/rssh-renderer/src/lib.rs`

**Step 1: Write failing tests**

Cover target offsets, source crops, parent origin outside viewport, and three
sorted overlay parents using the attachment set as the only logical source.

**Step 2: Verify RED**

Run: `cargo test -p rssh-renderer cell_attachment_geometry -- --nocapture`

**Step 3: Implement runtime resolution**

Resolve each attachment's destination pixel rectangle and sample crop from the
parent source using actual `RenderGeometry`. Keep backing parents whenever an
attachment references them; do not use default terminal fragments to filter
viewport candidates. Ensure full and damage passes share this path.

**Step 4: Verify GREEN and regression**

Run focused command, then `cargo test -p rssh-renderer`.

**Step 5: Commit**

```powershell
git add crates/rssh-renderer/src/lib.rs
git commit -m "feat: render persistent image cell attachments"
```

### Task 2: Apply CellTransform to bounded vertical and line movement

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`
- Test: `crates/rssh-terminal/src/parser.rs`, `crates/rssh-terminal/src/lib.rs`

**Step 1: Write failing tests**

For narrow `SU`/`SD`, `IL`/`DL`, LF/IND/NEL/RI, place a 2x2 image wholly
inside and crossing LR. Assert attachment cells follow copied text cells,
blanked cells disappear, exterior cells remain, and stored Kitty data remains.

**Step 2: Verify RED**

Run: `cargo test -p rssh-terminal horizontal_margin_cell_attachment_vertical -- --nocapture`

**Step 3: Implement transform application**

Build transforms from existing cell-copy loops and apply them to persistent
attachments. Remove whole-placement retirement for covered operations; retain
only malformed-placement fallback.

**Step 4: Verify GREEN and regression**

Run focused command, then `cargo test -p rssh-terminal`.

**Step 5: Commit**

```powershell
git add crates/rssh-terminal/src/parser.rs crates/rssh-terminal/src/lib.rs
git commit -m "feat: transform image attachments with bounded rows"
```

### Task 3: Apply CellTransform to bounded character editing and Kitty state

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`
- Test: `crates/rssh-terminal/src/parser.rs`, `crates/rssh-terminal/src/lib.rs`

**Step 1: Write failing tests**

Cover `ICH`/`DCH`, cached/last/pending Kitty placeholders, high-byte image ids,
and relative placements. Assert attachment deletion/movement exactly matches
cells with no stale placeholder reference or double shift.

**Step 2: Verify RED**

Run: `cargo test -p rssh-terminal horizontal_margin_cell_attachment_edit -- --nocapture`

**Step 3: Implement state updates**

Apply character transforms to attachments and placeholder coordinates. Rebuild
physical renderer state from live attachments and preserve protocol payload and
explicit-delete behavior.

**Step 4: Verify GREEN and regression**

Run focused command, then `cargo test -p rssh-terminal`.

**Step 5: Commit**

```powershell
git add crates/rssh-terminal/src/parser.rs crates/rssh-terminal/src/lib.rs
git commit -m "fix: transform kitty attachment state with bounded edits"
```

### Task 4: Documentation, complete verification, and review

**Status (2026-07-20):** Completed after the independent verification recorded
with the documentation commit.

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/mvp-6-app-shell-v1.md`
- Modify: `docs/research/wezterm-parity-gap.md`
- Modify: `docs/plans/2026-07-19-wezterm-horizontal-margins-design.md`

**Step 1: Update parity claims**

Document persisted attachment semantics and any explicit malformed-protocol
fallback; remove the previous conservative-intersection claim only after all
bounded transforms pass.

**Step 2: Verify**

Run:

```powershell
cargo test -p rssh-terminal
cargo test -p rssh-renderer
cargo test -p rssh-app
cargo test --workspace --all-targets
cargo fmt --all -- --check
git diff --check
```

**Step 3: Commit**

```powershell
git add docs
git commit -m "docs: record cell attachment graphics parity"
```
