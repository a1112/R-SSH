# WezTerm-Compatible Width Reflow Design

## Goal

Make a width resize match the pinned WezTerm behavior: reflow the main screen
and its scrollback by logical lines, resize but do not reflow the alternate
screen, and refresh selection-oriented UI state without preserving stale
physical coordinates.

## Evidence and compatibility target

The pinned upstream is `093bf6bf2b82b929ed80c04fd54ebc80464f715e`.

- `term/src/screen.rs` reflows only screens with scrollback.  It joins
  soft-wrapped physical lines, re-wraps them to the new width, and maps the
  terminal cursor through the logical line.  The alternate screen is not
  reflowed so full-screen programs do not receive conflicting screen changes.
- `wezterm-gui/src/selection.rs` stores physical cell plus stable row
  coordinates.  The GUI clears a normal selection when a changed reflow row
  intersects it.
- `overlay/copy.rs` and `overlay/quickselect.rs` detect width changes, clear
  their derived results, search again, and regenerate match presentation. They
  do not promise text-identity coordinate remapping.

This slice deliberately implements that behavior rather than a stronger,
R-SSH-specific cross-reflow selection migration.

## Current mismatch

`TerminalGrid::resize_with_seqno` copies the rectangular intersection of the
old and new dense grids. `Terminal::resize` applies that operation to the
active grid and saved main grid but never touches main scrollback. The window
layer treats only height changes as UI identity boundaries, so a width-only
resize leaves ordinary, Copy, Search, Quick Select, and viewport coordinates
pointing at old physical cells.

## Design

### Terminal reflow

Add a terminal-internal main-screen reflow operation used by `Terminal::resize`
when `columns` changes and the main screen is present, whether it is currently
active or saved behind the alternate screen.

1. Form the main-screen physical stream from scrollback followed by the main
   grid. Consecutive rows whose preceding row has `wrapped = true` form one
   logical line. A non-wrapped row terminates a logical line, including an
   explicit hard newline.
2. Remove only the soft-wrap boundary while joining; preserve cells, style,
   sequence metadata, and hard-break boundaries. Repack each logical line at
   the new column count, setting `wrapped` on every non-final output row.
3. Rebuild scrollback and the visible main grid from the repacked stream while
   honoring the current scrollback limit and padding a short viewport with
   blank rows. Mark rebuilt rows with the resize seqno and issue full damage.
4. Map the active or saved main cursor by its logical-cell offset. Carry that
   mapping through cursor, saved cursor, pending-wrap state, and the terminal
   fields whose row/column values are tied to physical main lines. Clamp only
   after a logical map is unavailable due to retention pruning.
5. Keep alternate-screen width handling distinct: narrow resize truncates each
   physical row; wider resize preserves rows and marks them dirty. It never
   joins or splits alternate rows. Both screens still receive dimensions and
   saved cursor updates.

The reflow implementation must be grapheme-width aware. A styled blank used as
a wide glyph continuation is emitted only with its leading glyph and must not
become an independently movable character. Custom/ambiguous-width terminal
settings remain authoritative.

### Resize outcome and app boundary

Expose a compact `TerminalResizeOutcome` from terminal/runtime resize. It must
say whether the main screen reflowed and whether the active screen was main or
alternate, without exposing a public map of every old physical cell. The app
uses it before owner reconciliation:

- a main reflow clears ordinary selection and restores the base snapshot;
- a Copy/Search controller keeps its mode, query, and editing state but drops
  stale source selection/current-match presentation, clears its match cache,
  and rebuilds results against the reflowed terminal;
- Quick Select keeps its configured patterns but drops matches/current label
  state, clears its cache, then recomputes matches and deterministic labels;
- the main viewport returns to the terminal's post-reflow physical range
  rather than retaining an unrelated old stable row;
- alternate-only resize retains existing pane UI coordinates and follows the
  existing normal resize/reconciliation path.

This is intentionally not a terminal identity retirement. Search/Copy/Quick
mode ownership survives a main reflow, but every presentation derived from old
physical coordinates is recomputed.

### Metadata and retention

The reflow task is incomplete unless it accounts for main-screen coordinate
metadata. The implementation must either map or deliberately invalidate,
according to upstream-compatible behavior:

- cursor/saved cursor/pending-wrap/NFC printable position;
- semantic prompt rows and command exit rows;
- scrollback offset and stable dimensions;
- inline-image, kitty placeholder, relative-parent, virtual-placement, and
  pending-placement coordinates.

For metadata with no safe logical-cell map, retire that item instead of leaving
it at a potentially unrelated physical location. Alternate-screen metadata is
not reflowed.

### Invariants

- Width changes cannot silently truncate soft-wrapped main content.
- Explicit hard breaks remain hard breaks after any resize sequence.
- Resize does not reflow alternate-screen rows.
- No ordinary or overlay projection can copy/highlight a stale physical source
  after main reflow.
- Main reflow is safe while the alternate screen is active; the saved main
  screen and scrollback are reflowed before it is restored.
- Every reflow result is deterministic for the same terminal state and target
  size.

## Test strategy

Terminal tests cover narrow/wide round trips, main scrollback, hard vs soft
breaks, cursor and pending wrap, Unicode/custom width, zero dimensions,
retention pruning, and main reflow while alternate is active. Alternate tests
prove truncation/dirtying without logical rewrap.

App tests cover active and inactive panes for ordinary-selection clearing;
Copy and Search mode/query retention with rebuilt matches; Quick candidate and
label regeneration; viewport reset; and unchanged alternate-only owner state.
Integration tests cover native window resize, PTY resize, renderer snapshot
rebuild, and no stale text copied after reflow.

## Non-goals

- Cross-reflow text-identity persistence for normal or rectangular selection.
- Reflowing alternate-screen content.
- General terminal reflow behavior beyond the pinned WezTerm semantics.
- Unbounded support for arbitrary image coordinate transformations; unsupported
  unsafe metadata is retired rather than mispositioned.
