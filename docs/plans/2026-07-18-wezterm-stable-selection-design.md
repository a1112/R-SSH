# WezTerm Stable Selection and Dirty Invalidation Design

## Status

Approved on 2026-07-18.

This design is based on pinned WezTerm commit
`093bf6bf2b82b929ed80c04fd54ebc80464f715e`.

## Goal

Make selection ownership use stable terminal rows rather than viewport-local or
retained-buffer ordinal rows, and invalidate ordinary selections with
WezTerm-style terminal sequence numbers and per-line dirty tracking.

The completed slice must:

- preserve a selection when the viewport moves;
- preserve a selected line when normal full-screen scrolling moves that same
  line into scrollback without changing its contents;
- avoid retargeting a selection when scrollback pruning removes older rows;
- copy selected text directly from stable terminal rows, including offscreen
  rows;
- clear an ordinary selection when a changed visible stable row intersects it;
- apply the same rules to active and inactive panes;
- keep ordinary selection state separate from Search, Copy Mode, and Quick
  Select transient highlights.

## Non-goals

This slice does not:

- port WezTerm's complete `Screen`, mux pane, reflow, or renderer;
- promise that a selection retains identity across width reflow;
- make Search, Copy Mode, or Quick Select pane-local controllers;
- implement inactive-pane hover-wheel routing;
- add per-cell sequence numbers;
- complete selection, App Shell v2, or general WezTerm parity.

Search, Copy Mode, and Quick Select coordinates that survive terminal updates
must use stable rows in this slice, but their controller ownership remains a
separate pane-local overlay milestone.

## Upstream contract

### Stable rows

WezTerm uses `StableRowIndex = isize`. The main screen maps retained physical
row `i` to `stable_row_offset + i`.

- `scrollback_top` is the stable ID of the oldest retained row.
- `physical_top` is the stable ID of the first live viewport row.
- Retaining a row while it moves into scrollback preserves its stable ID.
- Removing oldest history advances `stable_row_offset`; surviving rows keep
  their IDs.
- A removed stable row must not resolve to the new oldest row.
- The alternate screen is a separate no-scrollback coordinate domain.

### Sequence numbers

Terminal sequence numbers begin non-zero. They advance unconditionally once
for every pinned-WezTerm batch boundary: each input/action batch, each resize
call (including a same-size resize), and each public erase-scrollback call.
A cursor-only or control-only input batch can therefore advance the terminal
sequence without dirtying a line. Sequence numbers do not advance once per
cell.

Each retained line stores its last-change sequence number. A line is changed
since `s` exactly when `line_seqno == 0 || line_seqno > s`.

- Normal main-screen full-width, full-height upward scrolling preserves the
  identity and sequence of existing rows. Only the new bottom line is dirty.
- An upward main-screen row scroll may record scrollback only when scrollback
  is allowed, the effective top is row zero, and the left/right margins span
  the full width. This covers LF/IND, SU/CSI `S`, and delete-line at row zero,
  including a short vertical region. The shifted stable suffix below a short
  region is dirty at the current sequence.
- Scroll regions whose effective top is below row zero, top-anchored regions
  with narrow horizontal margins, alternate-screen scrolling, and other
  slot-replacing operations mark affected destination rows dirty at the
  current sequence without recording main-screen history.
- Rendering damage and row dirty state remain separate. A full-screen visual
  damage region does not imply that every stable row changed identity.

### Ordinary selection invalidation

An ordinary selection stores stable inclusive endpoints, rectangular state,
and the terminal sequence at which it was established or extended.

Before painting each pane, WezTerm:

1. determines the pane's current visible stable row range;
2. asks which visible rows changed after the selection sequence;
3. clears the ordinary selection only when a changed visible row intersects
   the selected row range.

It does not scan every offscreen selected row on every PTY update. Search and
Copy Mode use `CopyOverlay`; Quick Select uses `QuickSelectOverlay`. While
either overlay family is active, WezTerm skips ordinary-selection dirty
invalidation for that pane.

### Pruning and ED3

Pruning and ED3 do not eagerly clear a selection. Instead:

- removal advances the main-screen stable top;
- strict stable lookup refuses removed rows;
- render and extraction consider only surviving rows with their original
  stable IDs;
- a fully removed selection renders and copies nothing;
- a partially retained selection can return only the surviving portion;
- no removed endpoint is clamped or saturated to the new oldest row.

ED3 removes history without dirtying surviving visible rows, so an ordinary
selection wholly within unchanged visible rows remains valid.

### Resize and alternate screen

Pinned WezTerm increments the terminal sequence for resize. Width changes may
rewrap and change stable assignments, and affected lines are dirty. This slice
does not promise selection persistence across width reflow.

The main and alternate screens are distinct coordinate domains. Pinned
WezTerm's exact dirty stamping differs between alternate-screen mode variants
and does not by itself provide a safe pre-paint guard against same-number rows
from the other screen. R-SSH therefore makes screen identity explicit in
stored selection and viewport state. A selection is never projected or
extracted against a different screen identity, and a buffer switch retires
ordinary and transient GUI selection state synchronously. This is an
intentional local safety mechanism for the same observable contract: old
coordinates never target unrelated rows in another buffer.

Vertical resize is another explicit identity boundary in this slice. The
current grid model destroys rows when shrinking and would reuse their slots
when growing. Until full WezTerm resize/reflow storage is ported, a height
change synchronously retires ordinary and transient selections. Width changes
advance the sequence and dirty all reflowed or replaced rows; persistence
across width reflow remains a non-goal.

## Terminal architecture

### Public types

`rssh-terminal` will expose:

```rust
pub type StableRowIndex = isize;
pub type SequenceNo = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalScreenDomain {
    Main,
    Alternate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalStableDimensions {
    pub domain: TerminalScreenDomain,
    pub viewport_rows: usize,
    pub scrollback_rows: usize,
    pub scrollback_top: StableRowIndex,
    pub physical_top: StableRowIndex,
}
```

`scrollback_rows` is the total retained row count for the active domain,
including the live viewport. For the alternate screen it equals
`viewport_rows`, `scrollback_top == 0`, and `physical_top == 0`. The terminal
API also exposes:

- current terminal sequence number;
- active screen identity;
- checked `stable_bottom_exclusive = physical_top + viewport_rows`;
- the retained half-open stable range;
- active stable viewport range for a stable viewport top;
- strict retained-index to stable-row conversion;
- strict stable-row to retained-index conversion;
- changed retained stable rows for a half-open range and base sequence;
- whether a requested stable range is fully retained;
- text extraction over stable inclusive selection endpoints.

Single-row conversion is always strict. Stable selection extraction intersects
the original stable range with the retained range and preserves the original
row IDs, matching upstream observable behavior: a fully pruned selection
returns no text, while a partially retained selection returns only its
surviving portion. It never clamps an endpoint onto another row. Any clamping
helper used for viewport rendering is named and kept separate.

### Stored metadata

The existing terminal model gains:

- a current non-zero sequence number;
- a monotonically advancing main-screen stable-row offset;
- last-change sequence metadata for every grid row;
- last-change sequence metadata for every scrollback row.

The implementation must distinguish:

- moving a row while preserving identity and last-change sequence; and
- replacing a stable slot with different contents and marking it changed.

These are represented by two unique internal mutation families, such as
`move_row_preserving_identity` and `replace_row_at_seqno`. Cell/attribute
writes, wrapped-line state, row-width/reflow changes, image/placeholder cell
assignment, and any pinned-upstream operation that calls
`make_all_lines_dirty` update the corresponding row sequences. Palette and
other upstream whole-line presentation changes therefore dirty all retained
lines in the currently active screen domain. Window-only decoration changes
that do not dirty upstream terminal lines remain render damage only.

The parser must not rely on scattered raw `grid.set` calls to infer row
dirtiness afterward.

### Pruning

All oldest-row removal flows use one internal pruning operation. It must update
the stable offset atomically with existing physical-index metadata such as:

- semantic prompt rows;
- semantic command exits;
- inline image rows;
- Kitty placeholder rows.

The operation covers capacity trimming, runtime scrollback-limit reduction,
ED3, and limit-zero top-anchored scrolling.
`erase_scrollback_and_viewport` is a compound operation: it first prunes
history through this helper, then replaces and dirties affected viewport rows
at the current sequence.

### Scroll paths

All upward row-scroll entry points share one eligibility rule: a main-screen
operation with scrollback allowed, effective top row zero, and full-width
horizontal margins records history. This includes LF/IND, SU/CSI `S`, and
delete-line at row zero, even for a short vertical region. Existing retained
rows moved into history preserve content identity; stable slots in the suffix
below a short region shift and are dirty. A top-anchored operation with narrow
horizontal margins, a region
beginning below row zero, or an alternate-screen scroll does not create
main-screen history and marks affected stable slots dirty.

## Window selection architecture

### Three coordinate layers

The GUI uses distinct types for:

1. stable terminal state, stored long term;
2. transient stable Search/Copy/Quick Select positions;
3. viewport-local presentation coordinates used only for hit testing and
   rendering.

The ordinary selection type stores:

```text
screen domain: main or alternate
anchor: stable row + column
focus: stable row + column
rectangular: bool
seqno: terminal sequence
```

The existing viewport selection shape remains a rendering projection and must
not be stored in `PaneRuntime`.

### Pane ownership

`PaneRuntime` continues to own each pane's ordinary selection. Take/install
operations transfer only ordinary selection state.

Search, Copy Mode, and Quick Select may temporarily drive a presentation
selection, but their provenance stays explicit. A pane switch:

- ends mouse dragging;
- never promotes a transient highlight to ordinary selection;
- follows the current controller-exit boundary until the later pane-local
  overlay slice.

The stable-coordinate migration covers these concrete fields:

- mouse drag anchor/focus and `WindowClick` multi-click state;
- `SelectionSourceCell` and `WindowSourceSelection`;
- `WindowCopyMode.source_cursor` and `source_anchor`;
- `WindowSearchMatch.source_row` and `end_source_row`;
- Quick Select's stored `WindowSearchMatch` values and their viewport
  projections.

Search query/editing state and controller ownership do not move into
`PaneRuntime`. Match sets may be recomputed when their source rows are no
longer retained.

### Viewport

The pane viewport is concretely stored as:

```text
PaneStableViewport {
    main_top: Option<StableRowIndex>
}
```

It follows these rules:

- `None` follows the terminal bottom;
- on the main screen, `Some(row)` clamps upward to `scrollback_top`; a value
  whose requested top reaches or enters the live viewport
  (`row >= physical_top`) normalizes to `None`;
- entering the alternate screen preserves the main-screen top as dormant pane
  state, while the alternate screen always renders at bottom with no
  scrollback;
- returning to the main screen restores and clamps its dormant top against the
  retained main range;
- for a zero-sized viewport, every requested scrolled-back position
  normalizes to `None`;
- wheel, page, prompt navigation, and scrollbar input update the viewport;
- new PTY output does not make a scrolled-back viewport drift merely because
  the retained history length changed;
- pruning may clamp the viewport to retained history but never clamps a
  selection endpoint.

Scrollbar offsets remain derived values for rendering and pointer math.

### Rendering

Before composing a pane snapshot:

1. apply upstream-compatible ordinary-selection invalidation unless a
   Copy/Search/Quick overlay exemption is active;
2. project the surviving ordinary or transient stable selection into the
   current viewport;
3. apply the selection palette;
4. apply existing foreground HSB, background opacity, inactive-pane HSB, and
   minimum-contrast processing in the established order.

Inactive pane PTY output uses the same invalidation and projection helper as
the active pane before rebuilding its presentation snapshot.

While a Search/Copy/Quick overlay is active, real PTY changes as well as
synthetic highlight changes are exempt. Exiting the overlay does not refresh
the ordinary selection sequence. The next underlying pane paint therefore
checks accumulated row changes since the ordinary selection was established.

### Text extraction

Ordinary selected text comes from stable selection-range extraction, not from
the current render snapshot. Single-row conversion remains strict; range
extraction intersects retained rows while retaining their original stable
coordinates.

Extraction preserves the current behavior for:

- inclusive endpoints;
- rectangular selections;
- soft-wrapped logical lines;
- trimming trailing blanks at logical line boundaries;
- reversed anchor/focus ordering.

Removed rows are filtered by their stable IDs and are never replaced with
unrelated retained rows. For a non-rectangular partial-prefix prune, the first
surviving row was an original middle row and therefore starts at column zero;
for a partial-suffix prune, the last surviving original middle row extends to
the end of that row. Rectangular selections keep their original column range
on every surviving row. Soft-wrapped extraction continues to join surviving
physical spans according to their original stable rows.

The existing WezTerm-facing Lua pane dimensions and cursor fields migrate at
the same boundary: `scrollback_top`, `physical_top`, cursor stable Y, and all
Lua/action coordinates derived from the viewport no longer expose
retained-buffer ordinals.

Search `CurrentSelectionOrEmptyString` reads ordinary selection text before it
creates transient state. Copy Mode and Quick Select actions read their own
transient stable selections.

## Lifecycle boundaries

- Focus and tab changes preserve the pane's ordinary stable selection and
  stable viewport.
- A new split starts without selection and follows the bottom viewport.
- Closing a pane removes only that pane's GUI state.
- `MovePaneToNewTab` preserves terminal runtime, viewport, ordinary selection,
  and selection sequence within the same GUI window.
- `MovePaneToNewWindow` keeps terminal runtime and viewport but clears GUI
  ordinary selection at the GUI-window boundary, matching the existing pinned
  ownership contract.
- Runtime scrollback-limit changes update active and inactive panes.
- A screen-domain switch or viewport-height change synchronously retires
  ordinary and transient GUI selections, multi-click caches, and drag state
  before any projection, extraction, or callback can consume coordinates in
  the new identity domain.
- Width reflow, destructive reset, and erase operations use row dirty metadata
  and strict coordinate conversion rather than endpoint clamping.

## Error handling and invariants

- Sequence numbers use checked increment in all builds and fail fast on the
  practically unreachable overflow boundary. They never saturate.
- Stable-row arithmetic uses checked or saturating operations only for range
  construction, never to retarget an invalid selection.
- Conversion failure means "row not retained", not "use nearest row".
- Changed-row results are sorted, deduplicated, half-open, and limited to the
  requested retained intersection.
- Empty screens and zero-sized grids return empty ranges and no selection.
- Alternate-screen snapshots never sample main-screen scrollback.

## Testing strategy

### Terminal tests

- initial stable dimensions and conversion;
- non-zero sequence and one unconditional increment per mutation batch,
  including cursor-only input and same-size resize with no dirty lines;
- full-screen LF/IND scroll preserves row identity and sequence;
- eligible top-anchored full-screen/short-region SU and row-zero DL match
  LF/IND history behavior, including dirty suffix rows for the short-region
  case;
- top-zero SU/DL with narrow horizontal margins does not record history;
- only the new bottom row is dirty after normal full-screen scrolling;
- non-top-anchored SU/SD, IL/DL, margins, and alternate scrolling dirty
  affected slots without recording main-screen history;
- capacity trim, limit zero, runtime limit reduction, ED3, and public erase
  advance stable top without retargeting removed rows;
- strict row lookup rejects removed rows, while selection-range extraction
  returns only surviving original stable rows;
- partial-prefix and partial-suffix prune preserve non-rectangular column
  semantics, while rectangular and soft-wrapped extraction keep their own
  documented ranges;
- width resize dirties replaced rows; height resize and alternate-screen
  entry/exit expose the documented domain/dimension transition;
- wrapped rows, semantic zones, inline images, and Kitty placeholder row
  metadata remain coherent after pruning.
- image/placeholder cell assignment dirties the affected rows, and palette or
  other pinned `make_all_lines_dirty` operations dirty every retained row in
  the active screen domain.

### Window tests

- wheel, page, prompt, and scrollbar viewport movement preserve ordinary
  selection;
- scrolled-back viewport remains anchored while PTY output arrives;
- selection scrolls out of view and back to the same highlighted text;
- offscreen and cross-scrollback selection copies original text;
- soft-wrapped and rectangular stable selection extraction;
- non-intersecting visible dirty row preserves ordinary selection;
- intersecting visible dirty row clears it at presentation time;
- normal full-screen scrolling preserves a selected unchanged row;
- pruning never retargets a removed selection;
- ED3 preserves an unchanged visible selection and removes access to deleted
  history;
- active and inactive panes use identical invalidation rules without affecting
  each other;
- Search-only, Copy-Mode-only, and Quick-Select-only overlay exemptions;
- each overlay exemption covers real PTY modification of a selected visible
  row, and exiting the overlay triggers accumulated dirty invalidation without
  refreshing the ordinary selection sequence;
- transient highlights do not become ordinary selections;
- active and inactive pane height changes and main/alternate screen switches
  synchronously retire ordinary and transient selections before projection,
  extraction, or callbacks; multi-click and drag caches retire at the same
  boundary, while a dormant main-screen viewport restores after returning from
  the alternate screen;
- resize, close, split, `MovePaneToNewTab`, and `MovePaneToNewWindow`
  lifecycle behavior;
- active and inactive selection rendering keeps the established alpha and HSB
  ordering.

### Verification gates

Run:

```text
cargo test -p rssh-terminal
cargo test -p rssh-app selection
cargo test -p rssh-app copy_mode
cargo test -p rssh-app search
cargo test -p rssh-app quick_select
cargo test -p rssh-app scrollback
cargo test -p rssh-app
cargo test --workspace
cargo fmt --all -- --check
git diff --check
```

All new production behavior follows a witnessed RED-GREEN-REFACTOR cycle.
