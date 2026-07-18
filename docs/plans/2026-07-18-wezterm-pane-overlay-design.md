# WezTerm Pane-Local Overlay Ownership Design

## Status

Approved on 2026-07-18.

This design is based on pinned WezTerm commit
`093bf6bf2b82b929ed80c04fd54ebc80464f715e`.

## Goal

Replace R-SSH's window-global Search, Copy Mode, and Quick Select state with the
pinned WezTerm ownership model: every pane owns at most one current transient
overlay slot.

The completed slice must:

- keep each pane's current overlay across pane, tab, and workspace focus
  changes within one native window;
- model Search and Copy Mode as states of one shared `CopySearch` controller;
- let Quick Select replace the pane's current `CopySearch` overlay rather than
  coexist with it or restore it after Quick Select exits;
- route keyboard input, copy actions, and titles through the active pane's
  overlay only, while deriving presentation from every visible pane's own
  overlay;
- render each visible split pane's own overlay without projecting it into
  another pane;
- reconcile active and inactive pane overlays against terminal output,
  stable-row pruning, screen-domain changes, and viewport-height changes;
- retain ordinary-selection dirty invalidation exemption with the overlay that
  owns it;
- destroy only the overlay whose pane or tab is closed;
- preserve the overlay when a pane moves to a new tab in the same native
  window, and clear GUI overlay state when it moves to a different native
  window;
- correct the existing milestone documents that describe three independent
  per-pane controllers.

## Corrected Upstream Contract

Pinned WezTerm stores a `HashMap<PaneId, PaneState>` in `TermWindow`.
`PaneState` owns viewport, selection, and one optional `OverlayState`.
Search and Copy Mode both use `CopyOverlay`; activating either mode while a
`CopyOverlay` already exists updates that same controller in place. Quick
Select uses `QuickSelectOverlay` and assignment replaces the pane's existing
overlay slot.

Consequently, the old R-SSH milestone wording is incorrect:

> Each pane owns independent Search, Copy Mode, and Quick Select state.

The accurate contract is:

> Each pane owns at most one transient overlay slot. The slot is either
> `CopySearch` or `QuickSelect`. Search and Copy Mode are modes of the shared
> `CopySearch` controller; Quick Select replaces the current slot.

The acceptance suite still covers Search, Copy Mode, and Quick Select as three
observable behavior classes, but it must not assert that all three controllers
coexist or are restored after replacement.

Pinned WezTerm also keeps remembered search patterns per tab after the overlay
exits. That recall cache is not pane overlay ownership and must not be stored
inside `PaneUiState`. If R-SSH changes recall in this slice, it must use tab
ownership; otherwise exact pattern-history parity remains an explicit follow-up
rather than being misrepresented as pane-local state.

## Non-goals

This slice does not:

- port WezTerm's `Arc<dyn Pane>` wrapper hierarchy or complete mux;
- create a second general pane-state registry beside the existing
  `PaneRuntime` registry;
- route arbitrary actions addressed to inactive panes through their saved
  overlays; pinned WezTerm does not provide that general guarantee;
- implement full search-pattern history, regex/search-result refresh parity,
  width reflow, inactive-pane hover-wheel routing, or general App Shell v2;
- preserve a pane overlay across a native-window boundary;
- copy pinned WezTerm's stale `PaneState` retention after a real pane closes.

Immediate close cleanup, explicit inactive stable-row reconciliation,
domain/height retirement, deterministic Quick Select pruning, and explicit
native-window-boundary cleanup are intentional R-SSH safety contracts layered
on top of the upstream ownership model. They preserve the same observable
ownership contract but are not claimed as line-for-line upstream behavior.

## Data Model

### One pane-local slot

The public ownership invariant is represented as a sum type:

```rust
enum PaneTransientOverlay {
    CopySearch(WindowCopySearchController),
    QuickSelect(WindowQuickSelect),
}

enum WindowCopySearchMode {
    Search,
    Copy,
}

struct WindowCopySearchController {
    mode: WindowCopySearchMode,
    copy_mode: WindowCopyMode,
    search: Option<WindowSearch>,
}

struct PaneUiState {
    stable_viewport: PaneStableViewport,
    ordinary_selection: Option<StableOrdinarySelection>,
    overlay: Option<PaneTransientOverlay>,
}
```

`WindowCopySearchController` owns the copy cursor, anchor, selection mode, jump
state, search direction, query, match type, editing state, and current search
match. Search mode requires initialized search state. Copy mode may retain that
state, and entering Search from a controller without it initializes empty
search state. Mode-only Search and Copy transitions preserve controller
identity and shared state. Supplying a different Search pattern preserves the
copy cursor and selection but invalidates and recomputes search results.

Quick Select is a separate variant. Installing it drops the previous
`CopySearch` controller. Installing `CopySearch` drops Quick Select. Explicitly
exiting Quick Select leaves the slot empty; it does not resurrect the replaced
controller.

### Runtime ownership

`PaneRuntime` owns `PaneUiState` together with the pane's terminal runtime,
session, base snapshot, and writer. `NativeWindowApp` keeps the active pane's
same `PaneUiState` as a front buffer. `take_active_runtime` and
`install_active_runtime` move the complete UI state atomically.

This reuses the existing one-runtime-per-pane lifecycle. A second
`HashMap<PaneId, PaneUiState>` would duplicate close, move, pending-window, and
domain-retirement paths and could drift from `pane_runtimes`.

The viewport-local `WindowSelection` remains a derived presentation value. It
is rebuilt from stable ordinary selection or the current overlay and is never
stored in `PaneRuntime`.

`WindowCopyMode.source_cursor` and `source_anchor` are authoritative.
Viewport-local copy cursor and anchor fields are caches only. They are
reprojected after pane restoration, viewport movement, output, pruning, and
resize instead of being trusted across inactive periods.

## Lifecycle and State Transitions

### Creating and replacing overlays

- Starting Search in an empty slot creates `CopySearch` in Search mode.
- Starting Copy Mode in an empty slot creates `CopySearch` in Copy mode.
- Starting Search while `CopySearch` exists changes it to Search mode in
  place. A mode-only transition preserves shared controller state; a different
  pattern invalidates and recomputes search results.
- Starting Copy Mode while `CopySearch` exists changes it to Copy mode in
  place, preserving shared controller state.
- Starting Quick Select replaces any current pane overlay.
- Starting Search or Copy Mode while Quick Select exists replaces Quick
  Select.
- Explicit exit clears only the active pane's current overlay or changes the
  shared `CopySearch` mode as required by the command.

No transition may create a state in which Quick Select coexists with Search or
Copy Mode.

### Pane, tab, and workspace focus

A focus change performs these steps:

1. identify the previous active pane;
2. end window-level pointer capture, drag, and click-count state;
3. move the previous pane's complete `PaneUiState` into its `PaneRuntime`;
4. apply or observe the App Shell focus change;
5. install the new active pane's complete `PaneUiState`;
6. reconcile stable coordinates and rebuild the active projection;
7. update the title from the newly active overlay.

Focus changes do not clear pane overlays. They also never promote transient
overlay highlighting into ordinary selection.

Copy Mode fallback actions that switch pane or tab must dispatch without first
exiting the source pane's overlay. Mouse focus handling must resolve the target
pane before deciding whether any overlay receives or swallows the event.

### Window-level and tab-level overlays

Higher-level tab overlays and modal interfaces such as Command Palette,
Launcher, confirmations, input selectors, pane selection, and tab navigation
keep their existing input and presentation precedence without destroying pane
overlay slots. Whether they hide a whole tab, compose over pane presentation,
or only intercept input remains specific to the higher-level UI. When that UI
exits, the active pane overlay is reprojected and remains available.

This separates overlay visibility from overlay ownership and matches pinned
WezTerm's distinct tab/window overlay storage.

### Moving and closing

- A new split or tab begins with an empty `PaneUiState`.
- `MovePaneToNewTab` within the same native window moves the runtime and its
  overlay unchanged.
- `MovePaneToNewWindow` preserves terminal runtime, scrollback, and stable
  viewport, but clears ordinary selection and transient overlay before the
  pending runtime is materialized in the destination window.
- Closing an inactive pane drops only its runtime and overlay.
- Closing an active pane drops its overlay, then installs the surviving active
  pane's saved overlay.
- Closing a tab drops all pane overlays owned by that tab and leaves other
  tabs unchanged.
- Closing the native window drops all remaining overlays.

## Input and Action Ownership

Keyboard input and mode key tables resolve against the active pane's overlay.
Saved inactive overlays do not consume keys and do not change when another pane
receives input.

Copy and selection commands use the active pane's overlay-derived selection or
its ordinary selection. They never read a saved inactive controller merely
because it exists.

Quick Select acceptance captures the source pane and clears or extracts the
source overlay before executing a nested action that may change focus. A
`Multiple` action must not exit or mutate the overlay of whichever pane becomes
active during that action.

The design deliberately does not promise general dispatch into an arbitrarily
addressed inactive overlay. That would exceed pinned WezTerm's current action
routing contract.

## Rendering and Projection

Every pane snapshot begins from that pane's selection-free terminal snapshot.
A shared presentation helper receives:

- the pane terminal;
- its `PaneUiState`;
- effective palette and inactive-pane appearance;
- the pane's `PaneRenderRect`.

The helper:

1. reconciles ordinary-selection dirty invalidation unless that pane owns an
   active overlay;
2. projects the pane's ordinary or overlay stable selection into its viewport;
3. applies Search/Copy/Quick styling to that pane only;
4. returns pane-local overlay cells positioned in window coordinates;
5. applies existing color transformations in their established order.

Visible inactive splits render their saved overlays just as pinned WezTerm
substitutes each pane's own overlay during tab rendering.

Quick Select label cells must add both the owner pane rect's row and column
offset. They must be clipped to the owner rect and must not overwrite a
neighboring split. Search and Copy highlights are likewise owner-local.

Only the active pane overlay influences the window title and input key table.

## Terminal Mutation and Stable Coordinates

Active and inactive panes use one owner-local reconciliation contract.

For ordinary selection:

- an active pane overlay defers dirty-row invalidation for its owner;
- an inactive saved overlay provides the same owner-local exemption;
- leaving the overlay does not refresh the ordinary selection sequence;
- the next base presentation evaluates accumulated changed rows.

For `CopySearch`:

- retained stable cursor, anchor, and match coordinates survive output and
  history growth;
- a pruned current Search match becomes `None` while query and match type
  remain;
- if the copy cursor or anchor is no longer retained, the whole `CopySearch`
  overlay retires so no orphan Search state remains;
- viewport-local cursor, anchor, and selection are rebuilt from retained
  stable coordinates.

For Quick Select:

- matches and labels are filtered as parallel arrays by original index;
- surviving labels stay attached to their original matches;
- removed stable rows are never retargeted to the new oldest row;
- if the current match is pruned, the Quick Select overlay retires rather than
  targeting another match;
- if the current match survives, it is retained by stable match identity and
  its vector index is recomputed;
- if no matches survive, the Quick Select overlay retires.

A main/alternate screen-domain change or viewport-height identity change
retires that pane's ordinary selection and overlay synchronously. Width-only
resize continues to follow the stable-selection slice's existing documented
boundary until full reflow parity is implemented.

`ClearScrollback(ScrollbackAndViewport)`, runtime scrollback-limit changes, and
inactive PTY output must all invoke the same reconciliation rules. No mutation
path may leave a stale viewport projection merely because the pane is inactive.

## Error Handling and Invariants

- Every pane has zero or one transient overlay.
- A `CopySearch` controller has exactly one active mode.
- Quick matches and labels always have equal length.
- A stable coordinate conversion failure means "not retained"; it never
  clamps or redirects to another row.
- A screen-domain mismatch retires the affected GUI state before rendering,
  copying, or invoking callbacks.
- Missing or already-closed pane IDs cause owner-local state to be discarded,
  not transferred to the current active pane.
- Any overlay completion or nested action that may change focus captures its
  source pane before later clearing or mutating owner-local overlay state.
- Derived viewport selection and overlay cells are rebuilt rather than
  serialized.

## Documentation Changes

The implementation updates:

- `docs/architecture.md`;
- `docs/mvp-6-app-shell-v1.md`;
- `docs/research/wezterm-parity-gap.md`.

All three will replace the "independent Search, Copy Mode, and Quick Select
state" wording with the one-slot `CopySearch | QuickSelect` contract. They will
also distinguish pinned upstream behavior from R-SSH's immediate close cleanup
and explicit inactive stable-row reconciliation.

After the implementation lands, the milestone moves this slice from "Next" to
completed evidence and names the next bounded parity gap without claiming full
App Shell v2 or general WezTerm parity.

## Testing Strategy

### Controller shape and transitions

- Search in an empty slot creates `CopySearch(Search)`.
- Copy Mode in an empty slot creates `CopySearch(Copy)`.
- Mode-only Search to Copy and Copy to Search transitions retain shared cursor,
  selection, and search state in one controller; a new Search pattern
  invalidates and recomputes results without replacing the controller.
- Quick Select replaces `CopySearch`.
- Search or Copy replaces Quick Select.
- Exiting Quick Select does not restore the replaced overlay.

### Ownership and focus

For Search, Copy Mode, and Quick Select observable behavior:

- two panes save and restore distinct current overlays;
- two tabs save and restore distinct current overlays;
- workspace switching preserves owner state;
- input to the active pane does not mutate an inactive pane;
- a Copy Mode pane/tab focus fallback preserves the source overlay;
- click-to-focus does not clear the previous pane's overlay;
- window titles follow only the active overlay.

### Rendering

- two visible panes render distinct overlay highlighting;
- inactive visible overlays remain presented;
- overlay cells never project into another pane;
- Quick Select labels include owner pane row and column offsets and are clipped
  to the owner rect;
- hiding and revealing pane overlays with window/tab overlays preserves state.

### Mutation and retirement

- inactive ordinary selection retains its exemption while its owner overlay is
  active and evaluates accumulated dirty rows after exit;
- ordinary output and retained history growth preserve stable controller
  coordinates;
- active and inactive pruning reconcile only the owner overlay;
- Copy cursor/anchor pruning cannot leave an orphan Search;
- Quick matches and labels remain paired after pruning;
- screen-domain and height changes retire only the affected owner state;
- scrollback erase and runtime limit changes leave no stale projection.

### Move and close

- moving active and inactive panes to a new tab in the same window preserves
  their overlays;
- moving a pane to a new native window clears GUI selection and overlay while
  preserving terminal, scrollback, and viewport;
- closing inactive pane or tab removes only its overlays;
- closing active pane or tab restores the survivor's saved overlay;
- new panes start with an empty slot;
- Quick Select nested actions clear the source owner and never the new active
  pane.

### Gates

Each behavioral change follows a witnessed red-green-refactor cycle. Every
implementation task runs its focused tests, `cargo fmt --all -- --check`, and
`git diff --check`. The completed branch runs:

```text
cargo test -p rssh-app
cargo test --workspace -q
cargo fmt --all -- --check
git diff --check <base>..HEAD
```

Final review compares the delivered behavior and documentation against this
design and pinned upstream evidence before local merge.
