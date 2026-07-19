# WezTerm Cell-Granular Graphics Mapping Design

## Goal

Make graphics follow WezTerm's cell-level behavior during `DECLRMM`/`DECSLRM`
bounded movement. A narrow `SU`/`SD`, line edit, or character edit must move
the graphics cells inside its transformed rectangle while leaving graphics
cells outside it in place. A graphic may therefore be visibly split at a
horizontal-margin boundary.

## Evidence and boundary

At pinned WezTerm `093bf6bf2b82b929ed80c04fd54ebc80464f715e`, narrow-margin
`Screen::scroll_up_within_margins` and `scroll_down_within_margins` copy and
clear only `Cell`s. Image attachment is cell state, so there is no whole-image
translation or intersecting-image retirement. The existing R-SSH rectangle
placement model cannot express this with a single moved origin.

This slice introduces cell-granular render fragments for bounded operations;
it does not redesign image upload/storage, explicit Kitty deletion, or normal
full-width scroll behavior.

## Architecture

1. Persist a geometry-independent `CellAttachment` for every terminal cell
   covered by a physical inline-image placement. An attachment records its
   destination terminal cell, protocol parent identity, and logical source-cell
   identity/crop. Existing `ItermInlineImage` remains the protocol-facing
   placement and source-data record; it is never used to reconstruct which
   attachment cells survived a transform.
2. Use a shared `CellTransform` for narrow `SU`/`SD`, bounded line movement,
   and single-row `ICH`/`DCH`. The transform maps an attachment's source
   terminal cell to a destination cell or deletion. The resulting attachment
   set is persisted in terminal/screen state.
3. The renderer consumes that persisted attachment set as authoritative input.
   It resolves only pixel rectangles and sampling at the active
   `RenderGeometry`; it does not infer logical attachment identities from
   pixel offsets, a default 8x16 cell, or a parent rectangle. Damage includes
   both old and new attachment pixel extents.
4. Keep Kitty stored payloads, image-number mappings, and virtual placement
   data out of coordinate transforms. Placeholder caches, pending placeholders,
   and `last_kitty_placeholder` are screen-coordinate state and must receive
   the same transform or be invalidated when their source cell is deleted.
5. Preserve relative-Kitty semantics conservatively: a physical fragment
   generated from a relative placement transforms by cell, while protocol
   parent/child bookkeeping remains valid. Do not move a descendant a second
   time through the old whole-placement helper.

## Correctness invariants

- For every transformed cell, its graphics fragment set after the operation is
  exactly the pre-operation fragment set at the source cell; blanked cells have
  no fragments.
- For every cell outside the transform rectangle, graphics fragments are
  unchanged, including target-offset fragments whose pixels overlap the
  rectangle.
- A graphic crossing a LR boundary is not retired solely because it intersects
  the boundary; it is split according to the moved cells.
- Stored Kitty data is unaffected by terminal cell movement. Explicit graphics
  delete behavior is unchanged.
- The renderer consumes fragments without double-rendering their parent
  placement, and old/new pixel extents are damaged.
- Screen switching, resize, reset, history pruning, and full-width scrolling
  retain their existing metadata lifecycle unless a fragment is explicitly
  involved.

## Error handling and rollout

Attachment construction validates declared cell geometry and source bounds at
placement time, independently of renderer cell pixels. If a protocol placement
cannot provide a valid logical cell footprint, the operation uses the existing
safe retirement path for that placement and emits full-viewport damage; it
never leaves stale coordinates. This fallback is observable in focused tests
and is not advertised as upstream-equivalent.

The rollout is intentionally staged: first fragment data and renderer support,
then vertical bounded transforms, then single-row character transforms and
Kitty placeholder state, followed by documentation and exhaustive verification.
