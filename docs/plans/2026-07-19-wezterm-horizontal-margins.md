# WezTerm Horizontal Margin Semantics Implementation Record

**Goal:** Match pinned WezTerm `DECLRMM`/`DECSLRM` cell-level bounded
scrolling, editing, and right-edge writing semantics.

**Reference:** WezTerm `093bf6bf2b82b929ed80c04fd54ebc80464f715e`.

**Architecture:** Preserve the existing full-width, top-anchored main-screen
scrollback path. For an active non-full left/right margin, use bounded-cell
movement and bounded same-row shifts. The following records what the feature
branch implements; it is not a claim of general terminal or renderer parity.

## Ground rules retained by the implementation

- Bounded operations preserve cells outside the active LR interval exactly.
- Partial-width movement never writes main-screen scrollback or rebases stable
  row IDs; full-width behavior keeps its existing scrollback-aware path.
- `ICH` requires both TB and LR cursor membership. `DCH` requires LR membership
  only. `ECH` remains physical-right-edge erase.
- A printable character, insert mode, and auto-wrap use the LR right edge only
  while the cursor is within the LR interval.
- The original cursor column gates LF/IND/NEL/RI constrained scrolling; in
  particular NEL decides before its carriage-return behavior.

## Task 1: bounded CSI `SU`/`SD` — complete

Implemented bounded vertical copy-and-blank helpers and routed `CSI S`/`CSI T`
through them only for active, non-full LR margins. Regression coverage proves
the outside columns remain unchanged, zero/default count behaves as one, and a
narrow top-anchored main-region movement does not add history or churn stable
row identity. Text-only movement records a rectangular damage region and
updates affected row sequences.

Implementation commits: `27f9983e`, `3eaae63e`, `1a5c23b6`, `ccdffc00`.

## Task 2: line editing and control scrolling — complete

`IL`/`DL` now require both TB and LR membership. LF, IND, NEL, and RI perform
the constrained edge scroll only when the cursor was within LR; outside LR they
retain ordinary row/cursor behavior without moving the bounded rectangle. NEL
captures that membership before applying carriage-return behavior. Regression
coverage also proves active alternate-screen changes do not alter dormant main
screen state.

Implementation commit: `7915080b`.

## Task 3: character edits and writes — complete

`ICH`/`DCH` use the active LR right edge and leave exterior cells unchanged.
Normal and insert-mode output use the LR right edge only for an in-LR cursor;
outside LR writing and physical wraps retain their full-line behavior. The
coverage includes right-edge normal/insert output, wide glyphs, variation
selectors, gate differences, and physical-right `ECH`.

Implementation commit: `82a699c2`.

## Metadata and renderer safety — completed CellAttachment policy

The later CellAttachment slice replaces this plan's former
conservative-intersection policy for declared-cell inline-image and Kitty
placements. Bounded vertical, line, and character transforms now move/delete
one persistent attachment per covered terminal cell with the matching text
cell. A placement crossing LR can therefore split, while attachment cells
outside the transformed rectangle remain unchanged.

`kitty_placeholder_cells`, `last_kitty_placeholder`, and
`pending_kitty_placeholder` receive the same `ICH`/`DCH` coordinate transform,
including their rendered coordinates, high-byte image identities, and relative
placement behavior. Stored Kitty payloads and explicit-delete behavior remain
unchanged. A narrow per-placement marker resolves only live-attachment versus
residual virtual-cache origin conflicts and follows the placement lifecycle.

Only malformed or footprint-less placements use the conservative retirement
fallback; damage covers the old/new visual extents, including target-offset
overflow. This bounded result is not a claim of arbitrary graphics re-layout,
general renderer parity, or full protocol parity.

Safety follow-up commits: `3eaae63e`, `1a5c23b6`, `ccdffc00`.

## Acceptance matrix

Before integration, run and record fresh evidence for:

1. `git -C refs/wezterm rev-parse HEAD`, plus upstream searches for
   `scroll_up_within_margins`, `scroll_down_within_margins`,
   `set_left_and_right_margins`, `InsertBlank`, and `DeleteCharacter`.
2. `cargo test -p rssh-terminal`.
3. `cargo test -p rssh-app`.
4. `cargo test --workspace --all-targets`.
5. `cargo fmt --all -- --check` and
   `git diff --check codex/wezterm-parity-progress..HEAD`.
6. The documented colors-alias and full-data verification scripts, if present.

Fresh spec and quality review must clear Critical/Important findings before
local fast-forward merge to `codex/wezterm-parity-progress`; the same matrix is
then repeated on that target branch.
