# WezTerm Pane-Local Overlay Ownership Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give every pane one WezTerm-compatible `CopySearch | QuickSelect`
overlay slot, preserve it across same-window focus and tab changes, and isolate
its input, rendering, terminal reconciliation, and lifecycle to its owner.

**Architecture:** `PaneTransientOverlay` is a sum type. Search and Copy Mode
share `WindowCopySearchController`; Quick Select replaces that variant.
`PaneUiState` travels atomically with `PaneRuntime`, while viewport-local
selection remains derived. Active and inactive terminal mutation and
presentation use shared owner-local helpers.

**Tech Stack:** Rust, `rssh-app`, existing `rssh-core` App Shell and
`rssh-terminal` stable-row APIs, built-in unit tests in
`crates/rssh-app/src/window.rs`.

**Pinned Reference:** WezTerm
`093bf6bf2b82b929ed80c04fd54ebc80464f715e`.

**Design:** `docs/plans/2026-07-18-wezterm-pane-overlay-design.md`.

---

## Execution Rules

- Execute tasks in order with one fresh implementation subagent per task.
- Every production behavior starts with a focused failing test. Record the
  expected failure before writing production code.
- The implementer commits only after focused tests, formatting, and
  `git diff --check` pass.
- After every task, run a fresh spec-compliance review. Only after that passes,
  run a fresh code-quality review. The same implementer fixes review findings;
  the same reviewer then re-reviews.
- Do not run implementation agents in parallel because all tasks modify
  `crates/rssh-app/src/window.rs`.
- Do not preserve the old three-`Option` model as a compatibility source of
  truth. Temporary accessors may exist during a task, but the committed result
  for Task 2 must have one active overlay slot.
- Do not claim arbitrary inactive-pane addressed dispatch. Saved inactive
  overlays render and reconcile, but only the active overlay consumes normal
  input and copy actions.
- Commit this plan as its own documentation commit before Task 1. No
  implementation task may leave this plan untracked.

### Task 0: Commit the reviewed implementation plan

**Files:**

- Create: `docs/plans/2026-07-18-wezterm-pane-overlay.md`

**Step 1: Verify the reviewed plan diff**

Run:

```text
git diff --check
git status --short
```

Expected: `git diff --check` exits zero and the only worktree change is the
untracked implementation plan.

**Step 2: Commit the plan**

Run:

```text
git add docs/plans/2026-07-18-wezterm-pane-overlay.md
git commit -m "docs: plan pane-local overlay ownership"
```

Expected: commit succeeds and contains only the implementation plan.

**Step 3: Verify a clean implementation baseline**

Run:

```text
git status --short --branch
git show --stat --oneline HEAD
```

Expected: the feature worktree is clean, and `HEAD` is the plan-only commit.
Do not dispatch the Task 1 implementer until this gate passes.

### Task 1: Define the single-slot controller and transition invariants

**Files:**

- Modify: `crates/rssh-app/src/window.rs:101141-101306`
- Test: `crates/rssh-app/src/window.rs` unit-test module

**Step 1: Write the failing shape and transition tests**

Add focused tests:

```rust
#[test]
fn pane_transient_overlay_search_and_copy_share_one_slot() { /* ... */ }

#[test]
fn pane_transient_overlay_new_search_pattern_invalidates_results() { /* ... */ }

#[test]
fn pane_transient_overlay_quick_select_replaces_copy_search_without_restore() {
    /* ... */
}

#[test]
fn pane_transient_overlay_search_mode_always_has_search_state() { /* ... */ }

#[test]
fn pane_transient_overlay_empty_slot_enters_copy_mode() { /* ... */ }

#[test]
fn pane_transient_overlay_search_or_copy_replaces_quick_select() { /* ... */ }
```

The tests must construct real `WindowCopyMode`, `WindowSearch`, and
`WindowQuickSelect` values. They must prove:

- empty -> Search creates `CopySearch(Search)`;
- Search -> Copy mutates one controller in place and retains its cursor,
  selection mode, and stored Search state;
- Copy -> Search with the same pattern retains existing results;
- Copy -> Search with a different query or match type clears `current` before
  recomputation;
- Quick Select replaces `CopySearch`;
- an empty slot can enter Copy mode;
- Search and Copy each replace a current Quick Select slot;
- exiting Quick Select leaves the slot empty;
- a Search-mode controller cannot expose `search == None`.

**Step 2: Run the tests and witness RED**

Run:

```text
cargo test -p rssh-app pane_transient_overlay_ -- --nocapture
```

Expected: compile failure or assertion failure because
`PaneTransientOverlay`, `PaneUiState`, and transition methods do not exist.
The failure must be caused by the missing model, not a malformed fixture.

**Step 3: Add the model and transition API**

Implement this shape near the existing Copy/Quick types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowCopySearchMode {
    Search,
    Copy,
}

#[derive(Debug)]
struct WindowCopySearchController {
    mode: WindowCopySearchMode,
    copy_mode: WindowCopyMode,
    search: Option<WindowSearch>,
}

#[derive(Debug)]
enum PaneTransientOverlay {
    CopySearch(WindowCopySearchController),
    QuickSelect(WindowQuickSelect),
}

#[derive(Debug, Default)]
struct PaneUiState {
    stable_viewport: PaneStableViewport,
    ordinary_selection: Option<StableOrdinarySelection>,
    overlay: Option<PaneTransientOverlay>,
}
```

Add owner-local methods with these responsibilities:

```rust
impl PaneUiState {
    fn enter_search(
        &mut self,
        initial_copy_mode: WindowCopyMode,
        requested: WindowSearch,
    );
    fn enter_copy_mode(&mut self, initial_copy_mode: WindowCopyMode);
    fn enter_quick_select(&mut self, quick_select: WindowQuickSelect);
    fn exit_overlay(&mut self);

    fn search(&self) -> Option<&WindowSearch>;
    fn search_mut(&mut self) -> Option<&mut WindowSearch>;
    fn copy_search(&self) -> Option<&WindowCopySearchController>;
    fn copy_search_mut(&mut self) -> Option<&mut WindowCopySearchController>;
    fn quick_select(&self) -> Option<&WindowQuickSelect>;
    fn quick_select_mut(&mut self) -> Option<&mut WindowQuickSelect>;
    fn overlay_active(&self) -> bool;
}
```

`enter_search` must preserve controller identity. If `requested.query` or
`requested.match_type` differs from retained Search state, install the new
pattern with `current = None`; otherwise retain the existing current match.
Entering Search when `search` is absent initializes it. `enter_copy_mode`
changes mode without dropping retained Search state. `enter_quick_select`
always replaces the slot.

Keep `WindowCopySearchMode` and `WindowSearch.editing` atomic:

- `Search` mode requires `search = Some(...)` with `editing = true`;
- `Copy` mode permits no Search state or retained Search state with
  `editing = false`;
- `EditPattern` sets both to Search/true;
- `AcceptPattern` sets both to Copy/false and keeps results;
- Search Escape and Copy `Close` retire the whole slot;
- no public transition may expose Search/false or Copy/true.

Do not derive `Clone` merely to move pane state. Use ownership and
`Option::take`.

**Step 4: Run focused tests and witness GREEN**

Run:

```text
cargo test -p rssh-app pane_transient_overlay_ -- --nocapture
```

Expected: all four focused tests pass.

**Step 5: Run task gates**

Run:

```text
cargo test -p rssh-app window_copy_mode_search_keeps_copy_mode_and_steps_matches
cargo fmt --all -- --check
git diff --check
```

Expected: all commands exit zero.

**Step 6: Commit**

```text
git add crates/rssh-app/src/window.rs
git commit -m "refactor: model pane transient overlay slot"
```

### Task 2: Migrate the active pane to the single overlay slot

**Files:**

- Modify: `crates/rssh-app/src/window.rs:80685-80810`
- Modify: `crates/rssh-app/src/window.rs:82440-82480`
- Modify: `crates/rssh-app/src/window.rs:87900-88350`
- Modify: `crates/rssh-app/src/window.rs:89103-89700`
- Modify: `crates/rssh-app/src/window.rs:94917-97080`
- Test: `crates/rssh-app/src/window.rs` unit-test module

**Step 1: Add failing single-pane integration tests**

Add:

```rust
#[test]
fn window_app_search_and_copy_transition_in_one_active_overlay() { /* ... */ }

#[test]
fn window_app_quick_select_replaces_copy_search_and_exit_does_not_restore_it() {
    /* ... */
}

#[test]
fn window_app_title_uses_only_active_overlay_variant() { /* ... */ }

#[test]
fn window_app_search_exit_rebuilds_base_projection_immediately() { /* ... */ }

#[test]
fn window_app_new_search_pattern_recomputes_results_without_resetting_copy_cursor() {
    /* ... */
}

#[test]
fn window_app_copy_search_mode_and_editing_state_change_atomically() { /* ... */ }
```

Use public-in-module app entry points (`enter_search_mode`,
`enter_copy_mode`, `enter_quick_select_mode`, key handling, and
`effective_window_title`). Do not test only the pure Task 1 helpers. The last
test must remove Search and verify stale transient projection is gone without
an extra manual `refresh_snapshot`. The new-pattern test must begin with a
real terminal search result, change query or match type, and prove the old
result is invalidated and a new result is computed while copy cursor, anchor,
and selection mode remain unchanged.

**Step 2: Run and witness RED**

Run:

```text
cargo test -p rssh-app window_app_search_and_copy_transition_in_one_active_overlay -- --nocapture
cargo test -p rssh-app window_app_quick_select_replaces_copy_search_and_exit_does_not_restore_it -- --nocapture
cargo test -p rssh-app window_app_title_uses_only_active_overlay_variant -- --nocapture
cargo test -p rssh-app window_app_search_exit_rebuilds_base_projection_immediately -- --nocapture
cargo test -p rssh-app window_app_new_search_pattern_recomputes_results_without_resetting_copy_cursor -- --nocapture
cargo test -p rssh-app window_app_copy_search_mode_and_editing_state_change_atomically -- --nocapture
```

Expected: assertions fail because the app still stores three independent
window-global `Option`s and Search exit does not rebuild projection.

**Step 3: Replace the active three-`Option` source of truth**

Replace:

```rust
search: Option<WindowSearch>,
copy_mode: Option<WindowCopyMode>,
quick_select: Option<WindowQuickSelect>,
```

with one active `PaneUiState` field. Move the active
`stable_viewport` and `ordinary_selection` into the same `PaneUiState` in this
task so there is one atomic active UI owner:

```rust
active_ui: PaneUiState,
selection: Option<WindowSelection>, // derived only
```

Add narrow `NativeWindowApp` accessors such as:

```rust
fn active_search(&self) -> Option<&WindowSearch>;
fn active_search_mut(&mut self) -> Option<&mut WindowSearch>;
fn active_copy_search(&self) -> Option<&WindowCopySearchController>;
fn active_copy_search_mut(&mut self) -> Option<&mut WindowCopySearchController>;
fn active_quick_select(&self) -> Option<&WindowQuickSelect>;
fn active_quick_select_mut(&mut self) -> Option<&mut WindowQuickSelect>;
```

Migrate every production read/write of the removed fields. Do not add mirror
fields or synchronize two representations. In this same task, migrate every
unit test and fixture that directly accesses the removed active
`stable_viewport`, `ordinary_selection`, `search`, `copy_mode`, or
`quick_select` fields. The Task 2 commit must compile the complete `rssh-app`
test target.

**Step 4: Convert mode entry, exit, title, copy, and projection**

- Search and Copy entry call `PaneUiState` transition methods.
- Quick Select replaces the active slot.
- Search Escape exits Search editing according to the shared-controller
  command by retiring the slot, matching current standalone and Copy-search
  Escape behavior. `EditPattern` and `AcceptPattern` atomically update both
  `WindowCopySearchMode` and `WindowSearch.editing`; full CopySearch exit
  clears the slot.
- Quick Select exit clears the slot and cannot restore a replaced controller.
- Every exit immediately rebuilds derived projection and base snapshot.
- `effective_window_title`, `selected_text`, key routing, copy assignments,
  search stepping, and Quick actions read the active overlay variant.
- Update existing tests that directly inspect `app.search`,
  `app.copy_mode`, or `app.quick_select` to inspect the new owner-local API.
  Do not weaken their behavior assertions.

**Step 5: Run focused and existing controller suites**

Run:

```text
cargo test -p rssh-app window_app_search_and_copy_transition_in_one_active_overlay -- --nocapture
cargo test -p rssh-app window_app_quick_select_replaces_copy_search_and_exit_does_not_restore_it -- --nocapture
cargo test -p rssh-app window_app_search_exit_rebuilds_base_projection_immediately -- --nocapture
cargo test -p rssh-app window_app_new_search_pattern_recomputes_results_without_resetting_copy_cursor -- --nocapture
cargo test -p rssh-app window_app_copy_search_mode_and_editing_state_change_atomically -- --nocapture
cargo test -p rssh-app copy_mode
cargo test -p rssh-app quick_select
cargo test -p rssh-app search
```

Expected: all exit zero. Confirm the new tests were observed failing before
production changes.

**Step 6: Run task gates**

Run:

```text
cargo fmt --all -- --check
cargo test -p rssh-app --no-run
git diff --check
```

Expected: both exit zero.

**Step 7: Commit**

```text
git add crates/rssh-app/src/window.rs
git commit -m "refactor: route active modes through pane overlay"
```

### Task 3: Move complete pane UI state through runtime focus lifecycle

**Files:**

- Modify: `crates/rssh-app/src/window.rs:81390-81510`
- Modify: `crates/rssh-app/src/window.rs:83577-83630`
- Modify: `crates/rssh-app/src/window.rs:84306-84480`
- Modify: `crates/rssh-app/src/window.rs:95065-95180`
- Test: `crates/rssh-app/src/window.rs:183520-183824`
- Test: `crates/rssh-app/src/window.rs:187152-187306`

**Step 1: Reverse the old pane-switch contract with failing tests**

Replace the old assertions that pane switching clears all modes. Add parameter
fixtures for Search, Copy, and Quick and focused tests:

```rust
#[test]
fn window_app_pane_focus_saves_and_restores_each_overlay_class() { /* ... */ }

#[test]
fn window_app_tab_switch_saves_and_restores_each_overlay_class() { /* ... */ }

#[test]
fn window_app_workspace_switch_saves_and_restores_each_overlay_class() {
    /* ... */
}

#[test]
fn window_app_pane_switch_never_promotes_overlay_projection_to_ordinary_selection() {
    /* ... */
}

#[test]
fn window_app_dirty_ordinary_selection_remains_deferred_in_saved_inactive_overlay() {
    /* ... */
}
```

For each observable class, give pane A and pane B distinct state:

- Search: query, match type, current stable match;
- Copy: stable cursor/anchor, selection mode, retained Search state;
- Quick: input, current stable match, labels, action.

Switch away and back at least twice. Assert exact owner state and active title,
and assert no transient projection enters ordinary storage.

**Step 2: Run and witness RED**

Run:

```text
cargo test -p rssh-app window_app_pane_focus_saves_and_restores_each_overlay_class -- --nocapture
cargo test -p rssh-app window_app_tab_switch_saves_and_restores_each_overlay_class -- --nocapture
cargo test -p rssh-app window_app_workspace_switch_saves_and_restores_each_overlay_class -- --nocapture
cargo test -p rssh-app window_app_pane_switch_never_promotes_overlay_projection_to_ordinary_selection -- --nocapture
cargo test -p rssh-app window_app_dirty_ordinary_selection_remains_deferred_in_saved_inactive_overlay -- --nocapture
```

Expected: the source overlay is missing after focus change because
`end_transient_selection_modes_for_pane_change` clears it and `PaneRuntime`
does not store it.

**Step 3: Make `PaneRuntime` own `PaneUiState`**

Change:

```rust
struct PaneRuntime {
    // terminal/session fields...
    snapshot: TerminalRenderSnapshot,
    ui: PaneUiState,
}
```

Remove separate runtime `stable_viewport` and `ordinary_selection` fields.
`new_inactive_pane_runtime`, spawning, pending-window materialization, and all
test fixtures must initialize `ui` explicitly.

**Step 4: Capture old runtime before App Shell focus mutation**

Refactor App Shell action dispatch to preserve the actual old owner:

1. capture `previous_active_pane`;
2. end pointer capture, drag, and click-count state before focus/layout
   mutation;
3. move the active terminal runtime and complete `active_ui` into one
   `PaneRuntime`;
4. apply the App Shell action;
5. if the action fails, reinstall the captured runtime and UI;
6. pass the captured runtime into `sync_pane_runtimes`;
7. if active pane is unchanged, reinstall it directly;
8. if active pane changed, store it under `previous_active_pane` and install
   the new active runtime.

Change `sync_pane_runtimes` to accept the already captured previous runtime. It
must never call `take_active_runtime` a second time or overwrite a captured
runtime with the replacement blank runtime.

This ordering is required for ordinary focus changes and is also the basis for
safe active-pane `MovePaneToNewWindow` in Task 6.

**Step 5: Move the active UI atomically**

- `take_active_runtime` uses `std::mem::take(&mut self.active_ui)`.
- `install_active_runtime` installs `runtime.ui` before rebuilding projection
  and title.
- `sync_pane_runtimes` no longer clears the transient overlay on focus change.
- Replace `end_transient_selection_modes_for_pane_change` with a helper that
  ends only pointer drag/capture/click-count state and clears the derived
  viewport projection before installing the new owner.
- Keep runtime retain/drop owner-local.
- Ensure a new pane gets `PaneUiState::default()`.

Migrate every production and test/fixture access to the removed
`PaneRuntime.stable_viewport` and `PaneRuntime.ordinary_selection` fields in
this same task. Update every direct `PaneRuntime` struct literal. Do not leave
a non-compiling intermediate commit.

**Step 6: Preserve overlay exemption by owner**

`PaneRuntime::reconcile_terminal_mutation` must skip ordinary-selection
dirty-row invalidation when `runtime.ui.overlay_active()`. This is an interim
owner-local rule; Task 4 adds complete overlay stable-coordinate reconciliation.
On explicit overlay exit, rebuild the base presentation without refreshing the
ordinary selection sequence so accumulated dirty rows are evaluated.

**Step 7: Run focused and lifecycle tests**

Run:

```text
cargo test -p rssh-app window_app_pane_focus_saves_and_restores_each_overlay_class -- --nocapture
cargo test -p rssh-app window_app_tab_switch_saves_and_restores_each_overlay_class -- --nocapture
cargo test -p rssh-app window_app_workspace_switch_saves_and_restores_each_overlay_class -- --nocapture
cargo test -p rssh-app window_app_pane_switch_never_promotes_overlay_projection_to_ordinary_selection -- --nocapture
cargo test -p rssh-app window_app_dirty_ordinary_selection_remains_deferred_in_saved_inactive_overlay -- --nocapture
cargo test -p rssh-app pane_switch
cargo test -p rssh-app stable_selection
cargo test -p rssh-app --no-run
```

Expected: all exit zero.

**Step 8: Run task gates and commit**

Run:

```text
cargo fmt --all -- --check
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "feat: preserve pane overlays across focus changes"
```

### Task 4: Share stable-coordinate reconciliation across active and inactive panes

**Files:**

- Modify: `crates/rssh-app/src/window.rs:81472-81483`
- Modify: `crates/rssh-app/src/window.rs:89052-89290`
- Modify: `crates/rssh-app/src/window.rs:94032-94110`
- Modify: `crates/rssh-app/src/window.rs:95654-95660`
- Test: `crates/rssh-app/src/window.rs` unit-test module

**Step 1: Write failing owner-local mutation tests**

Add:

```rust
#[test]
fn window_app_inactive_output_preserves_retained_copy_search_coordinates() {
    /* ... */
}

#[test]
fn window_app_inactive_prune_retires_only_unretained_copy_search_owner() {
    /* ... */
}

#[test]
fn window_app_inactive_prune_clears_search_current_without_dropping_query() {
    /* ... */
}

#[test]
fn window_app_inactive_quick_prune_keeps_match_identity_or_retires_overlay() {
    /* ... */
}

#[test]
fn window_app_inactive_screen_or_height_change_retires_only_owner_ui_state() {
    /* ... */
}

#[test]
fn window_app_clear_scrollback_and_viewport_reconciles_overlay_projection() {
    /* ... */
}

#[test]
fn window_app_active_prune_reconciles_copy_search_and_quick_overlay() {
    /* ... */
}

#[test]
fn window_app_active_screen_or_height_change_retires_owner_ui_state() {
    /* ... */
}

#[test]
fn window_app_runtime_scrollback_limit_reconciles_active_and_inactive_overlays() {
    /* ... */
}
```

The Quick test must cover both branches:

- current match survives: retain by stable `WindowSearchMatch` identity and
  recompute index;
- current match is pruned or no match survives: retire Quick Select;
- never retarget to another surviving match.

**Step 2: Run and witness RED**

Run:

```text
cargo test -p rssh-app window_app_inactive_prune_ -- --nocapture
cargo test -p rssh-app window_app_inactive_quick_prune_ -- --nocapture
cargo test -p rssh-app window_app_clear_scrollback_and_viewport_reconciles_overlay_projection -- --nocapture
cargo test -p rssh-app window_app_active_prune_reconciles_copy_search_and_quick_overlay -- --nocapture
cargo test -p rssh-app window_app_active_screen_or_height_change_retires_owner_ui_state -- --nocapture
cargo test -p rssh-app window_app_runtime_scrollback_limit_reconciles_active_and_inactive_overlays -- --nocapture
```

Expected: saved inactive overlay coordinates remain stale, or Quick current is
retargeted/left invalid, because inactive runtime reconciliation currently
handles only ordinary selection.

**Step 3: Extract owner-local reconciliation**

Add methods with no implicit active app state:

```rust
impl PaneUiState {
    fn retire_terminal_identity(&mut self);
    fn reconcile_stable_coordinates(&mut self, terminal: &Terminal);
    fn reconcile_terminal_mutation(&mut self, terminal: &Terminal);
}
```

Required behavior:

- clamp only stable viewport;
- perform ordinary dirty invalidation only when no owner overlay exists;
- `CopySearch`: clear Search `current` when its match is not retained; retire
  the entire overlay if stable copy cursor or anchor is not retained;
- recompute viewport-local Copy cursor/anchor from stable source coordinates;
- `QuickSelect`: filter matches and labels in parallel, preserve current by
  copied match identity, recompute index, and retire on current loss or empty
  results;
- never clamp a removed stable coordinate onto another row.

Use temporary local outcomes to avoid mutating `self.ui.overlay` through two
simultaneous borrows. Do not duplicate active and inactive implementations.

**Step 4: Route every mutation path through the shared contract**

Use the shared methods from:

- active PTY output;
- inactive PTY output;
- runtime scrollback-limit changes;
- `ClearScrollback(ScrollbackOnly)`;
- `ClearScrollback(ScrollbackAndViewport)`;
- terminal reset/destructive erase;
- window resize loops;
- screen-domain and viewport-height identity changes.

Identity retirement clears ordinary selection and owner overlay before any
render/copy/callback can consume the new domain. It must not clear another
pane's UI state.

**Step 5: Run focused and existing stable suites**

Run:

```text
cargo test -p rssh-app window_app_inactive_output_preserves_retained_copy_search_coordinates -- --nocapture
cargo test -p rssh-app window_app_inactive_prune_ -- --nocapture
cargo test -p rssh-app window_app_inactive_quick_prune_ -- --nocapture
cargo test -p rssh-app window_app_inactive_screen_or_height_change_retires_only_owner_ui_state -- --nocapture
cargo test -p rssh-app window_app_clear_scrollback_and_viewport_reconciles_overlay_projection -- --nocapture
cargo test -p rssh-app window_app_active_prune_reconciles_copy_search_and_quick_overlay -- --nocapture
cargo test -p rssh-app window_app_active_screen_or_height_change_retires_owner_ui_state -- --nocapture
cargo test -p rssh-app window_app_runtime_scrollback_limit_reconciles_active_and_inactive_overlays -- --nocapture
cargo test -p rssh-app scrollback_limit
cargo test -p rssh-app stable
```

Expected: all exit zero.

**Step 6: Run task gates and commit**

Run:

```text
cargo fmt --all -- --check
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "fix: reconcile pane overlays with terminal mutations"
```

### Task 5: Render every visible pane's own overlay and fix Quick label coordinates

**Files:**

- Modify: `crates/rssh-app/src/window.rs:89229-89540`
- Modify: `crates/rssh-app/src/window.rs:90420-90610`
- Modify: `crates/rssh-app/src/window.rs:90977-91065`
- Test: `crates/rssh-app/src/window.rs` unit-test module

**Step 1: Write failing presentation tests**

Add:

```rust
#[test]
fn window_app_visible_split_panes_render_distinct_search_overlays() { /* ... */ }

#[test]
fn window_app_visible_split_panes_render_distinct_copy_overlays() { /* ... */ }

#[test]
fn window_app_visible_split_panes_render_distinct_quick_overlays() { /* ... */ }

#[test]
fn window_app_inactive_quick_labels_use_owner_pane_row_and_column_offsets() {
    /* ... */
}

#[test]
fn window_app_quick_labels_are_clipped_to_owner_pane_rect() { /* ... */ }
```

Use two visible split panes with distinct text and distinct stable matches.
Inspect the composed `TerminalRenderSnapshot` by pane rect. Assert owner
highlights and label cells exist only inside the owner rect. Include a
right-side pane and a bottom pane so both column and row offsets are proven.

**Step 2: Run and witness RED**

Run:

```text
cargo test -p rssh-app window_app_visible_split_panes_render_distinct_ -- --nocapture
cargo test -p rssh-app window_app_inactive_quick_labels_use_owner_pane_row_and_column_offsets -- --nocapture
cargo test -p rssh-app window_app_quick_labels_are_clipped_to_owner_pane_rect -- --nocapture
```

Expected: inactive overlays are absent and/or Quick labels appear at
window-origin coordinates because rendering currently reads only active
window-global state.

**Step 3: Extract pane-parameterized projection and presentation**

Create helpers whose inputs explicitly include terminal/runtime, UI state, and
pane rect:

```rust
fn pane_overlay_source_selection(
    terminal: &Terminal,
    ui: &PaneUiState,
    word_boundary: &str,
) -> Option<WindowSourceSelection>;

fn pane_presentation_snapshot(
    base: &TerminalRenderSnapshot,
    terminal: &Terminal,
    ui: &PaneUiState,
    rect: PaneRenderRect,
    palette: &ResolvedPalette,
    /* existing appearance inputs */
) -> TerminalRenderSnapshot;

fn quick_select_cells_for_pane(
    terminal: &Terminal,
    viewport: PaneStableViewport,
    quick: &WindowQuickSelect,
    rect: PaneRenderRect,
) -> Vec<RenderCell>;
```

Use the repository's actual palette type name rather than inventing a duplicate
type. Keep base snapshots selection-free. Apply overlay projection when
composing each visible pane, for active and inactive panes alike.

**Step 4: Correct Quick label placement**

Quick cell window coordinates are:

```text
window_row    = rect.row    + pane_local_row
window_column = rect.column + pane_local_column
```

Clip each generated cell to the half-open pane rect. Keep matches and labels
paired. Do not allow a long label to overwrite a split separator or neighbor.

Only the active pane controls the title and input key table; rendering all
visible overlays does not make inactive overlays active input owners.

**Step 5: Run presentation and snapshot suites**

Run:

```text
cargo test -p rssh-app window_app_visible_split_panes_render_distinct_ -- --nocapture
cargo test -p rssh-app window_app_inactive_quick_labels_use_owner_pane_row_and_column_offsets -- --nocapture
cargo test -p rssh-app window_app_quick_labels_are_clipped_to_owner_pane_rect -- --nocapture
cargo test -p rssh-app render_snapshot
cargo test -p rssh-app quick_select
cargo test -p rssh-app inactive_pane
```

Expected: all exit zero.

**Step 6: Run task gates and commit**

Run:

```text
cargo fmt --all -- --check
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "feat: render owner-local pane overlays"
```

### Task 6: Preserve owner identity through input, modal precedence, move, and close

**Files:**

- Modify: `crates/rssh-app/src/window.rs:83260-83280`
- Modify: `crates/rssh-app/src/window.rs:84347-84356`
- Modify: `crates/rssh-app/src/window.rs:84803-84845`
- Modify: `crates/rssh-app/src/window.rs:86600-87695`
- Modify: `crates/rssh-app/src/window.rs:87900-88350`
- Modify: `crates/rssh-app/src/window.rs:89798-89920`
- Modify: `crates/rssh-app/src/window.rs:96121-96135`
- Modify: `crates/rssh-app/src/window.rs:83577-83630`
- Test: `crates/rssh-app/src/window.rs` unit-test module

**Step 1: Write failing input and focus-owner tests**

Add:

```rust
#[test]
fn window_app_copy_mode_focus_fallback_preserves_source_overlay() { /* ... */ }

#[test]
fn window_app_click_focus_does_not_clear_source_or_target_overlay() { /* ... */ }

#[test]
fn window_app_active_input_mutates_only_active_overlay() { /* ... */ }

#[test]
fn window_app_copy_and_selection_actions_read_only_active_pane_overlay() {
    /* ... */
}

#[test]
fn window_app_quick_nested_focus_action_clears_only_source_owner() { /* ... */ }

#[test]
fn window_app_each_higher_level_ui_preserves_pane_overlay_slots() { /* ... */ }
```

The active-copy test gives the inactive and active panes different controller
selections and records the clipboard/PTY side effect. It must prove copy,
send-selected-text, paste-selected-text, and clear-selection resolve only the
active pane and never read a saved inactive controller.

The Quick nested-action test must use a real `Multiple` sequence whose first
action changes pane or tab. Assert later source cleanup never clears the newly
active pane's saved overlay.

The higher-level UI test is a parameterized matrix covering Command Palette,
Launcher, close confirmation, generic confirmation, input selector, prompt
input, pane select, tab navigator, and character select. Exercise existing
entry and exit paths for both tab-wide and window/modal presentation classes.
Each case must preserve the pane slot and retain that UI's existing
input/presentation precedence.

**Step 2: Write failing move and close tests**

Add:

```rust
#[test]
fn window_app_move_active_or_inactive_pane_to_new_tab_preserves_overlay() {
    /* ... */
}

#[test]
fn window_app_move_active_pane_to_new_window_transfers_runtime_then_clears_gui_ui() {
    /* ... */
}

#[test]
fn window_app_move_inactive_pane_to_new_window_transfers_runtime_then_clears_gui_ui() {
    /* ... */
}

#[test]
fn window_app_close_inactive_pane_or_tab_drops_only_target_overlays() {
    /* ... */
}

#[test]
fn window_app_close_active_pane_or_tab_restores_survivor_overlay() {
    /* ... */
}

#[test]
fn window_app_new_split_starts_with_empty_overlay_slot() { /* ... */ }
```

Cover Search, Copy, and Quick observable classes through parameterized
fixtures. Test active and inactive source panes separately. For the new-window
boundary, inspect the materialized detached app: the exact source terminal
text, scrollback, and stable viewport survive; ordinary selection and overlay
are empty. Also prove the source runtime was not killed or replaced by the
blank active replacement.

**Step 3: Run and witness RED**

Run:

```text
cargo test -p rssh-app window_app_copy_mode_focus_fallback_preserves_source_overlay -- --nocapture
cargo test -p rssh-app window_app_click_focus_does_not_clear_source_or_target_overlay -- --nocapture
cargo test -p rssh-app window_app_active_input_mutates_only_active_overlay -- --nocapture
cargo test -p rssh-app window_app_copy_and_selection_actions_read_only_active_pane_overlay -- --nocapture
cargo test -p rssh-app window_app_quick_nested_focus_action_clears_only_source_owner -- --nocapture
cargo test -p rssh-app window_app_each_higher_level_ui_preserves_pane_overlay_slots -- --nocapture
cargo test -p rssh-app window_app_move_active_or_inactive_pane_to_new_tab_preserves_overlay -- --nocapture
cargo test -p rssh-app window_app_move_active_pane_to_new_window_transfers_runtime_then_clears_gui_ui -- --nocapture
cargo test -p rssh-app window_app_move_inactive_pane_to_new_window_transfers_runtime_then_clears_gui_ui -- --nocapture
cargo test -p rssh-app window_app_close_inactive_pane_or_tab_drops_only_target_overlays -- --nocapture
cargo test -p rssh-app window_app_close_active_pane_or_tab_restores_survivor_overlay -- --nocapture
cargo test -p rssh-app window_app_new_split_starts_with_empty_overlay_slot -- --nocapture
```

Expected: source modes are destroyed before focus dispatch, Quick cleanup
targets the newly active pane, higher-level UI clears pane state, or
new-window/close boundaries do not follow owner-local rules.

**Step 4: Fix input and completion ownership**

- Remove the unconditional `exit_copy_mode` before App Shell fallback.
- Resolve the pane hit target before overlay cleanup or focus transfer.
- Normal keyboard input reads only the active overlay.
- Quick acceptance captures `source_pane_id`, extracts/copies from that owner,
  and clears the source slot before a nested action can change focus.
- Do not implement general inactive-pane mode dispatch.

If an action needs side effects after focus change, capture the required source
text/action data before dispatch. Do not temporarily install an inactive
runtime as active.

**Step 5: Preserve pane slots under higher-level UI**

Remove Search/Copy/Quick destruction from Command Palette, Launcher,
confirmation, input-selector, prompt-input, pane-select, tab-navigator, and
character-select entry. Keep each higher-level UI's existing input and
presentation precedence. On exit, rebuild the active pane presentation.

Do not assert that every modal hides the whole tab; preserve its current
visibility semantics.

**Step 6: Enforce move and close boundaries**

- Same-window pane/tab/workspace operations move or retain `PaneRuntime.ui`.
- Pending new-window materialization calls a dedicated
  `prepare_for_new_window` method that clears ordinary selection and overlay
  but retains stable viewport.
- Active-pane move uses the pre-App-Shell `PaneRuntime` captured by Task 3;
  `sync_pane_runtimes` stores that exact runtime under the pending pane ID and
  never takes or overwrites it after App Shell focus changes.
- Inactive-pane move keeps its existing map-owned runtime under the pending
  pane ID.
- Pending pane IDs are retained until `take_next_pending_window_app` removes
  their runtime, sanitizes only GUI state, and installs it in the detached app.
- Runtime retain/drop removes only invalid pane IDs.
- Closing the active owner installs the survivor UI before title/projection
  rebuild.
- New runtime creation uses empty `PaneUiState`.

**Step 7: Run focused lifecycle and input suites**

Run:

```text
cargo test -p rssh-app window_app_copy_mode_focus_fallback_preserves_source_overlay -- --nocapture
cargo test -p rssh-app window_app_click_focus_does_not_clear_source_or_target_overlay -- --nocapture
cargo test -p rssh-app window_app_active_input_mutates_only_active_overlay -- --nocapture
cargo test -p rssh-app window_app_copy_and_selection_actions_read_only_active_pane_overlay -- --nocapture
cargo test -p rssh-app window_app_quick_nested_focus_action_clears_only_source_owner -- --nocapture
cargo test -p rssh-app window_app_each_higher_level_ui_preserves_pane_overlay_slots -- --nocapture
cargo test -p rssh-app window_app_move_active_or_inactive_pane_to_new_tab_preserves_overlay -- --nocapture
cargo test -p rssh-app window_app_move_active_pane_to_new_window_transfers_runtime_then_clears_gui_ui -- --nocapture
cargo test -p rssh-app window_app_move_inactive_pane_to_new_window_transfers_runtime_then_clears_gui_ui -- --nocapture
cargo test -p rssh-app window_app_close_
cargo test -p rssh-app move_to_new_
cargo test -p rssh-app pane_select
```

Expected: all exit zero.

**Step 8: Run task gates and commit**

Run:

```text
cargo fmt --all -- --check
git diff --check
git add crates/rssh-app/src/window.rs
git commit -m "fix: preserve pane overlay owner lifecycle"
```

### Task 7: Correct parity documentation and run completion gates

**Files:**

- Modify: `docs/architecture.md:1261-1269`
- Modify: `docs/mvp-6-app-shell-v1.md:2615-2640`
- Modify: `docs/research/wezterm-parity-gap.md:4391-4411`
- Verify: `docs/plans/2026-07-18-wezterm-pane-overlay-design.md`
- Verify: `crates/rssh-app/src/window.rs`

**Step 1: Update the three authoritative milestone summaries**

Replace every claim that each pane owns three independent controller states.
Document:

- one current `CopySearch | QuickSelect` pane-local slot;
- Search and Copy as shared-controller modes;
- Quick replacement with no implicit restoration;
- pane/tab/workspace save/restore within one native window;
- active-only input/title/copy routing;
- every-visible-pane owner-local rendering;
- stable mutation reconciliation and deterministic Quick prune as R-SSH
  safety contracts;
- same-window move preservation and native-window-boundary clearing;
- immediate close cleanup as an R-SSH robustness enhancement.

Move the slice from "Next Milestone" to completed evidence only after all
behavioral tasks and tests pass. Name the next bounded parity gap from actual
remaining evidence. Do not claim full App Shell v2 or general WezTerm parity.

**Step 2: Prove stale contract text is gone**

Run:

```text
rg -n "independent Search, Copy Mode|three independent|Search/Copy/Quick controller ownership" docs/architecture.md docs/mvp-6-app-shell-v1.md docs/research/wezterm-parity-gap.md
```

Expected: no output and exit code 1, meaning none of the three authoritative
summaries retains the stale contract. The approved design and implementation
plan deliberately quote the corrected historical wording and are excluded.

**Step 3: Run focused acceptance matrix**

Run:

```text
cargo test -p rssh-app pane_transient_overlay_
cargo test -p rssh-app window_app_pane_focus_saves_and_restores_each_overlay_class
cargo test -p rssh-app window_app_tab_switch_saves_and_restores_each_overlay_class
cargo test -p rssh-app window_app_inactive_prune_
cargo test -p rssh-app window_app_active_prune_reconciles_copy_search_and_quick_overlay
cargo test -p rssh-app window_app_runtime_scrollback_limit_reconciles_active_and_inactive_overlays
cargo test -p rssh-app window_app_visible_split_panes_render_distinct_
cargo test -p rssh-app window_app_inactive_quick_labels_use_owner_pane_row_and_column_offsets
cargo test -p rssh-app window_app_quick_nested_focus_action_clears_only_source_owner
cargo test -p rssh-app window_app_copy_and_selection_actions_read_only_active_pane_overlay
cargo test -p rssh-app window_app_move_active_pane_to_new_window_transfers_runtime_then_clears_gui_ui
cargo test -p rssh-app window_app_move_inactive_pane_to_new_window_transfers_runtime_then_clears_gui_ui
cargo test -p rssh-app window_app_close_
```

Expected: all exit zero.

**Step 4: Run full branch verification**

Run fresh:

```text
cargo test -p rssh-app
cargo test --workspace -q
cargo fmt --all -- --check
git diff --check 3b82fad408e6b1ae57f2bcd1daa9685384a6c887..HEAD
git status --short
```

Expected:

- `rssh-app` passes;
- workspace passes with 0 failed tests;
- format and diff checks exit zero;
- status contains only the intended documentation edits before commit.

**Step 5: Commit documentation**

```text
git add docs/architecture.md docs/mvp-6-app-shell-v1.md docs/research/wezterm-parity-gap.md
git commit -m "docs: record pane-local overlay parity"
```

**Step 6: Verify the committed branch again**

Run fresh:

```text
cargo test --workspace -q
cargo fmt --all -- --check
git diff --check 3b82fad408e6b1ae57f2bcd1daa9685384a6c887..HEAD
git status --short --branch
```

Expected: workspace has zero failures, format/diff checks pass, and the feature
worktree is clean.

**Step 7: Final independent review**

First verify the pinned upstream checkout and the exact ownership symbols:

```text
git -C refs/wezterm rev-parse HEAD
rg -n "pub struct PaneState|pane_state: HashMap|Search\\(pattern\\)|ActivateCopyMode|assign_overlay_for_pane" refs/wezterm/wezterm-gui/src/termwindow/mod.rs
rg -n "struct CopyOverlay|editing_search|impl Pane for CopyOverlay" refs/wezterm/wezterm-gui/src/overlay/copy.rs
rg -n "struct QuickSelectOverlay|impl Pane for QuickSelectOverlay" refs/wezterm/wezterm-gui/src/overlay/quickselect.rs
```

Expected: HEAD is exactly
`093bf6bf2b82b929ed80c04fd54ebc80464f715e`, and every symbol query returns
the fixed upstream ownership evidence.

Dispatch one final spec reviewer against:

- the approved design;
- all seven task requirements;
- pinned upstream evidence;
- the full diff from `3b82fad4` to feature `HEAD`.

Then dispatch one final code-quality reviewer. Fix and re-review all Critical
or Important findings before integration.

## Integration

The user has already selected local merge back to
`codex/wezterm-parity-progress`. After final review:

1. use `superpowers:finishing-a-development-branch`;
2. verify the feature branch one final time;
3. merge `codex/wezterm-pane-overlay` locally into
   `codex/wezterm-parity-progress`;
4. run `cargo test --workspace -q`, `cargo fmt --all -- --check`, and
   `git diff --check` on the merged target;
5. remove the clean feature worktree and delete the merged feature branch only
   after merged verification passes.
