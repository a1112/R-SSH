# WezTerm Horizontal Margin Semantics Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Match pinned WezTerm `DECLRMM`/`DECSLRM` cell-level bounded scrolling,
editing, and right-edge writing semantics.

**Architecture:** Preserve the existing full-width top-anchored main-screen
scrollback path. Add private bounded-cell movement helpers for partial-width or
non-top regions. Character shifts and printing select an LR right edge only
when the cursor is inside a configured horizontal region.

**Tech Stack:** Rust 2024, `rssh-terminal`, pinned WezTerm
`093bf6bf2b82b929ed80c04fd54ebc80464f715e`, Cargo test.

## Ground rules

- Use upstream behavior, not the current whole-row result.
- Bounded operations never write scrollback or rebase stable row IDs.
- Preserve cells outside the LR interval exactly; do not copy whole-row wrap
  or reflow-overflow state for a cell-only movement.
- Retire graphics/Kitty placement metadata that intersects but is not wholly
  contained by a bounded movement rectangle.
- One implementation agent at a time; each task has fresh spec then quality
  review and every Critical/Important finding is fixed and re-reviewed.

## Task 1: Bounded vertical cell movement

Files: modify and test `crates/rssh-terminal/src/parser.rs`.

1. Add RED tests on an 8x4 distinctive grid with LR `3;6`, TB `2;4`, for
   `CSI S` and `CSI T`: only columns 2..6 move, outside columns remain exact,
   count zero means one, and narrow top-anchored movement writes no scrollback.
2. Run `cargo test -p rssh-terminal terminal_horizontal_margin_scroll_ -- --nocapture` and confirm current full-row failure.
3. Add a private bounded vertical copy-and-blank helper taking top, bottom,
   inclusive columns, direction, and count. It updates only affected cells,
   dirty row sequences, rectangular damage, and safe metadata.
4. Route `scroll_up_region_by` and `scroll_down_region` to it when normal
   full-width scrollback conditions do not apply; retain existing fast path.
5. Run focused plus `cargo test -p rssh-terminal`, format and diff checks;
   commit `feat: bound terminal vertical scroll to horizontal margins`.

## Task 2: LR gates for line/control scrolling

Files: modify and test `crates/rssh-terminal/src/parser.rs`.

1. Add RED cases for LR-inside and LR-outside `IL`/`DL`, LF, IND, NEL, and RI;
   outside LR edge events move rows but do not scroll the constrained region.
2. Run `cargo test -p rssh-terminal terminal_horizontal_margin_line_ -- --nocapture` and confirm RED.
3. Add a private cursor-in-LR predicate. Use it for `IL`/`DL` in addition to
   vertical bounds and for edge-triggered LF/IND/NEL/RI scrolling.
4. Verify focused and full terminal tests, format/diff, then commit
   `fix: constrain line editing to horizontal margins`.

## Task 3: Bounded character edits and writes

Files: modify `crates/rssh-terminal/src/parser.rs`; test parser and `lib.rs`.

1. Add RED cases for `ABCDEFGH`, LR `3;6`, cursor at column 3: `CSI 2@` gives
   `AB  CDGH`; `CSI 2P` gives `ABEF  GH`. Prove ICH needs both TB/LR gates,
   DCH needs LR only, and ECH remains physical-right-edge erase.
2. Add normal and insert-mode right-LR-edge tests plus a wide-glyph boundary
   test; run `cargo test -p rssh-terminal terminal_horizontal_margin_character_ -- --nocapture` and confirm RED.
3. Implement bounded row shifts and an effective write edge used only when the
   cursor is inside LR. Do not alter full-screen behavior outside LR.
4. Verify focused/full terminal, format/diff, then commit
   `fix: bound terminal character edits to horizontal margins`.

## Task 4: Metadata, stable state, and screen isolation

Files: modify and test `crates/rssh-terminal/src/parser.rs`.

1. Add RED tests: wholly-contained image/Kitty placement translates, a
   cross-LR-boundary placement retires, and an outside placement is unchanged.
   Also prove dirty sequences/damage update but stable IDs/history remain.
2. Run `cargo test -p rssh-terminal terminal_horizontal_margin_metadata_ -- --nocapture` and confirm RED.
3. Implement explicit translate-or-retire metadata behavior and preserve
   alternate/dormant-main isolation.
4. Verify focused/full terminal, format/diff, then commit
   `fix: preserve graphics safety in bounded margin scrolls`.

## Task 5: Documentation and acceptance

Files: update `docs/architecture.md`, `docs/mvp-6-app-shell-v1.md`, and
`docs/research/wezterm-parity-gap.md`.

1. Record exact bounded operation families, external-cell preservation,
   graphics retirement policy, and the next concrete gap; do not claim general
   terminal or renderer parity.
2. Verify upstream with `git -C refs/wezterm rev-parse HEAD`, and search
   `scroll_up_within_margins`, `scroll_down_within_margins`,
   `set_left_and_right_margins`, `InsertBlank`, and `DeleteCharacter`.
3. Run `cargo test -p rssh-terminal`, `cargo test -p rssh-app`,
   `cargo test --workspace -q`, `cargo fmt --all -- --check`, and
   `git diff --check codex/wezterm-parity-progress..HEAD`.
4. Commit `docs: record horizontal margin scrolling parity`.

## Final review and integration

Perform fresh full spec and quality review, repair/re-review all
Critical/Important findings, rerun the matrix on the feature branch,
fast-forward merge locally to `codex/wezterm-parity-progress`, repeat the
matrix there, and safely remove the merged worktree and branch.
