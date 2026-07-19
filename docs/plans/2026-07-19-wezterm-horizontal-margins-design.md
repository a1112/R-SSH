# WezTerm Horizontal Margin Semantics Design

## Objective and delivered scope

Close the documented `DECLRMM`/`DECSLRM` cell-level scrolling, editing, and
right-edge-writing gap against pinned WezTerm commit
`093bf6bf2b82b929ed80c04fd54ebc80464f715e`.

## Evidence and boundary

The existing terminal stores left/right margins and already applies them to
some cursor operations, but its vertical scroll and line-edit helpers copy
whole rows.  This changes columns outside the configured margins.  WezTerm's
`Screen::scroll_up_within_margins` and `scroll_down_within_margins` copy and
clear only the `[left, right)` cells and never records a partial-width scroll
in main-screen scrollback.

This slice now covers the complete core operation set sharing that state:

- `SU`/`SD`, line feed/index/next-line/reverse-index scrolling, and `IL`/`DL`
  move only the intersection of vertical and horizontal margins;
- `ICH` and `DCH` shift only from the cursor to the right margin, with the
  upstream gate conditions; `ECH` remains physical-line erase;
- printable output and insert mode use the right margin for wrapping and
  shifting when the cursor is within a left/right-margin region;
- active and saved main screens, alternate-screen state, dirty rows, damage,
  stable-row identity, wide cells, and image/Kitty placement metadata remain
  safe.

It deliberately does not claim arbitrary graphics re-layout or exact graphical
coordinate mapping. A bounded cell operation retires every inline-image or
Kitty placement intersecting its moved rectangle, including a wholly-contained
placement. It also retires intersecting placeholder caches, orphaned relative
children, and stale Kitty cache entries. This conservative policy prevents a
placement from pointing at unrelated cells while a later slice can add
coordinate-aware translation. It retains Kitty's uploaded image payload, so a
newly requested placement can still use that image; explicit Kitty delete
semantics are unchanged.

## Alternatives considered

1. **Whole-row operations plus a visual clip.** Smallest code change, but it
   corrupts boundary-external cells and is not WezTerm-compatible.
2. **Only patch `SU`/`SD`.** Fixes the obvious case but leaves `IL`/`DL`,
   character edits, and right-margin writing inconsistent under the same
   terminal mode.
3. **Recommended: shared bounded-cell primitives.** Add explicit horizontal
   range helpers used by all relevant edit paths.  It makes the invariants
   testable once, preserves columns outside the region, and keeps full-width
   scrolling on the existing optimized scrollback path.

## Architecture

`Terminal` selects one of two paths for a vertical cell movement:

```text
full-width top-anchored main region -> existing scrollback-aware row path
all other regions / horizontal margins -> bounded-cell copy-and-blank path
```

The bounded path receives inclusive terminal coordinates, derives a half-open
column interval, copies cells without copying whole-row `wrapped` or reflow
overflow state, clears only the destination/source cells using the current
blank style, retires cross-boundary graphics metadata, marks every affected
physical row at the current sequence, and records one rectangular damage
region.  It never changes main scrollback or stable row identity.

Character insert/delete use a separate same-row bounded shift primitive.
`ICH` is active only inside both vertical and horizontal bounds; `DCH` needs
only the horizontal bound, matching WezTerm; `ECH` deliberately retains its
physical-right-edge behavior. Print and insert-mode paths select an effective
right edge only when the cursor is inside the horizontal margin interval.
Existing full-screen behavior is retained outside it.

When a bounded operation retires graphics metadata, it records both the normal
cell rectangle and full-viewport damage. The latter is intentional: relative
Kitty children may have visual extents beyond the changed cells. Text-only
bounded operations retain precise rectangular damage.

## Verification

The regression matrix uses distinctive text in columns outside both margins
and asserts it remains byte-for-byte unchanged after every operation. It
covers `SU`/`SD`, LF/IND/NEL/RI, `IL`/`DL`, `ICH`/`DCH`, physical-edge `ECH`,
right-edge normal and insert-mode output, zero/default counts, wide-cell
cleanup, non-scrollback bounded main movement, alternate isolation, retained
Kitty payload with retired placements/caches, damage, and stable-row sequence
semantics. The final matrix includes terminal, app, workspace, formatting,
diff, and pinned WezTerm evidence checks.
