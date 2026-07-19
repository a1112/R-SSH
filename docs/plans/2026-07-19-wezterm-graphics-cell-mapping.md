# WezTerm Cell-Granular Graphics Mapping Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make graphics follow WezTerm's per-cell bounded-movement semantics.

**Architecture:** Keep protocol placements and stored Kitty payloads intact, but derive renderable cell fragments from every physical placement. A shared transform maps source terminal cells to a destination or deletion for bounded vertical scroll, line edit, and character edit. Renderer input becomes the fragment list, so a source placement may appear as separated visual pieces when only some of its cells move.

**Tech Stack:** Rust, `rssh-terminal`, `rssh-renderer`, existing terminal and renderer unit tests.

---

### Task 1: Fragment model and renderer contract

**Files:**
- Modify: `crates/rssh-terminal/src/lib.rs`
- Modify: `crates/rssh-terminal/src/parser.rs`
- Modify: `crates/rssh-renderer/src/lib.rs`
- Test: terminal and renderer tests beside existing inline-image tests

**Step 1: Write the failing test**

Create a 2x2 physical image and assert that the terminal exposes four render fragments with one-cell destination coordinates and correct source crops. Add a renderer test proving fragments do not also draw their parent rectangle.

**Step 2: Run test to verify RED**

Run: `cargo test -p rssh-terminal graphics_fragment -- --nocapture`

Run: `cargo test -p rssh-renderer graphics_fragment -- --nocapture`

Expected: fail because no fragment API exists.

**Step 3: Write minimal implementation**

Introduce a public render-fragment type containing destination row/column, source crop, image identity, format, and placement metadata. Derive fragments from `ItermInlineImage`, preserve target offsets in the crop, expose them from `Terminal`, and make the renderer draw fragments when present.

**Step 4: Run focused tests**

Run both focused commands. Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-terminal/src/lib.rs crates/rssh-terminal/src/parser.rs crates/rssh-renderer/src/lib.rs
git commit -m "feat: expose cell-granular image fragments"
```

### Task 2: Shared bounded transform and vertical movement

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`
- Test: `crates/rssh-terminal/src/parser.rs`

**Step 1: Write the failing test**

Cover narrow `CSI S` and `CSI T` with a 2x2 image wholly inside LR and one crossing LR. Assert each source fragment moves or clears exactly like the text cell; exterior fragments remain fixed and source payload remains stored.

**Step 2: Verify RED**

Run: `cargo test -p rssh-terminal horizontal_margin_graphics_vertical -- --nocapture`

**Step 3: Write minimal implementation**

Add a private `CellTransform` mapping `(history_row, column)` to a destination or deletion for bounded up/down movement. Replace narrow-scroll retirement with fragment transform application. Damage both old and new fragment extents; leave full-width scroll unchanged.

**Step 4: Verify GREEN and regression**

Run the focused command, then `cargo test -p rssh-terminal`.

**Step 5: Commit**

```powershell
git add crates/rssh-terminal/src/parser.rs
git commit -m "feat: map graphics cells through bounded scrolls"
```

### Task 3: Line and character-edit transforms

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`
- Test: `crates/rssh-terminal/src/lib.rs`

**Step 1: Write the failing test**

Add `IL`/`DL` and `ICH`/`DCH` cases with images fully in, crossing, and outside the affected LR rectangle. Assert fragment destinations match cell copy and blank semantics, including count clipping and `ICH` TB gating versus `DCH` LR gating.

**Step 2: Verify RED**

Run: `cargo test -p rssh-terminal horizontal_margin_graphics_edit -- --nocapture`

**Step 3: Write minimal implementation**

Construct transforms from the exact loops that move cells and route fragment updates through the shared transform. Remove intersecting-image retirement only for operations now covered by fragments; retain safe fallback only for malformed geometry.

**Step 4: Verify GREEN and regression**

Run the focused command, then `cargo test -p rssh-terminal`.

**Step 5: Commit**

```powershell
git add crates/rssh-terminal/src/parser.rs crates/rssh-terminal/src/lib.rs
git commit -m "feat: map graphics cells through bounded edits"
```

### Task 4: Kitty placeholder and relative coordinate state

**Files:**
- Modify: `crates/rssh-terminal/src/parser.rs`
- Test: `crates/rssh-terminal/src/parser.rs`, `crates/rssh-terminal/src/lib.rs`

**Step 1: Write the failing test**

Cover cached, last, and pending Kitty placeholders across every transform, high-byte IDs, and a relative placement crossing LR. Assert no cache points at a deleted fragment and stored data remains placeable.

**Step 2: Verify RED**

Run: `cargo test -p rssh-terminal horizontal_margin_kitty_fragment -- --nocapture`

**Step 3: Write minimal implementation**

Apply the transform to placeholder coordinate state. Regenerate physical fragment output from live placeholders and avoid double-shifting relative descendants. Retire only unresolvable state and damage old pixel extents.

**Step 4: Verify GREEN and regression**

Run the focused command, then `cargo test -p rssh-terminal`.

**Step 5: Commit**

```powershell
git add crates/rssh-terminal/src/parser.rs crates/rssh-terminal/src/lib.rs
git commit -m "fix: map kitty placeholders through bounded cells"
```

### Task 5: Documentation and complete verification

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/mvp-6-app-shell-v1.md`
- Modify: `docs/research/wezterm-parity-gap.md`
- Modify: `docs/plans/2026-07-19-wezterm-horizontal-margins-design.md`

**Step 1: Update claims**

Replace conservative-retirement-only wording with the actual cell-fragment semantics. Keep malformed-geometry fallback and remaining non-graphics gaps explicit.

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
git commit -m "docs: record cell-granular graphics parity"
```
