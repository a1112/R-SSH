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

1. Represent the portion of an inline image that covers each terminal cell as
   an independently addressable render fragment. The fragment carries the
   source image/Kitty placement identity and crop information needed to render
   precisely that cell-sized part. Existing `ItermInlineImage` remains the
   protocol-facing placement and source-data record.
2. Derive fragments from a physical placement and use a shared `CellTransform`
   for narrow `SU`/`SD`, bounded line movement, and single-row `ICH`/`DCH`.
   The transform maps every source cell to a destination cell or deletion.
3. Apply a transform to graphics fragments exactly as cells are copied:
   fragments in moved cells relocate, fragments in blanked cells disappear,
   and fragments outside the transformed rectangle remain untouched. Damage
   includes both the old and new fragment extents.
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

Fragment derivation validates cell dimensions and source bounds. If an image
cannot be represented as cell fragments (zero geometry or malformed crop), the
operation uses the existing safe retirement path for that placement and emits
full-viewport damage; it never leaves stale coordinates. This fallback is
observable in focused tests and is not advertised as upstream-equivalent.

The rollout is intentionally staged: first fragment data and renderer support,
then vertical bounded transforms, then single-row character transforms and
Kitty placeholder state, followed by documentation and exhaustive verification.
