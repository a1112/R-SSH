# WezTerm Stable Selection and Dirty Invalidation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give R-SSH stable terminal row coordinates, WezTerm-style sequence/line-change tracking, stable pane viewports, and ordinary selection invalidation that matches pinned WezTerm `093bf6bf2b82b929ed80c04fd54ebc80464f715e`.

**Architecture:** `rssh-terminal` becomes the source of truth for stable row identity and per-row last-change sequence metadata. `rssh-app` stores pane viewports and long-lived selection coordinates in that stable space, projects them into viewport-local presentation types only while rendering, and invalidates ordinary selection at pane presentation boundaries using visible changed stable rows. Ordinary and transient selection provenance remain separate.

**Tech Stack:** Rust 2021, existing `rssh-terminal`/`rssh-app` crates, in-module unit tests, Cargo, pinned WezTerm source under `E:\project\R-SSH\refs\wezterm`.

---

## Execution rules

- Work only in `E:\project\R-SSH\.worktrees\wezterm-stable-selection`.
- Branch: `codex/wezterm-stable-selection`.
- Base implementation commit: `dfd4c7d4`.
- Use `@superpowers:test-driven-development` for every production change.
- For every RED step, capture the failing command and confirm that it fails
  because the required behavior is absent, not because of a typo.
- A missing-API RED may stop at compile time before any test executes. In that
  case, record the expected missing type/method diagnostic. After the API
  compiles, every named GREEN filter must execute at least one test.
- Do not use `--exact` with an unqualified Rust test name. Use the named
  substring filters below and confirm that at least one test ran.
- After each task: self-review, commit, then return the commit SHA and the
  witnessed RED/GREEN commands to the controller.
- The controller dispatches a separate specification reviewer, then a separate
  code-quality reviewer. Do not start the next task until both approve.
- Do not create a junction under this disposable worktree. The ignored pinned
  palette fixture already exists at
  `refs/wezterm/docs/colorschemes/data.json`.
- Do not modify or repair the known corrupt
  `refs/codex/turn-diffs/.../base` ref; automatic-GC warnings are expected and
  do not invalidate a successfully created commit.

## Authoritative references

- Design:
  `docs/plans/2026-07-18-wezterm-stable-selection-design.md`
- Pinned upstream stable rows:
  `E:\project\R-SSH\refs\wezterm\term\src\screen.rs`
- Pinned upstream sequence behavior:
  `E:\project\R-SSH\refs\wezterm\term\src\terminal.rs`
  and
  `E:\project\R-SSH\refs\wezterm\term\src\terminalstate\mod.rs`
- Pinned upstream line change tracking:
  `E:\project\R-SSH\refs\wezterm\wezterm-surface\src\line\line.rs`
- Pinned upstream selection:
  `E:\project\R-SSH\refs\wezterm\wezterm-gui\src\selection.rs`
- Pinned upstream invalidation:
  `E:\project\R-SSH\refs\wezterm\wezterm-gui\src\termwindow\mod.rs`
  (`check_for_dirty_lines_and_invalidate_selection`)
- Pinned upstream viewport:
  `E:\project\R-SSH\refs\wezterm\wezterm-gui\src\termwindow\mod.rs`
  (`get_viewport`/`set_viewport`)

### Task 1: Add terminal stable-row and sequence primitives

**Files:**

- Modify: `crates/rssh-terminal/src/lib.rs:5-9`
- Modify: `crates/rssh-terminal/src/lib.rs:213-349`
- Modify: `crates/rssh-terminal/src/parser.rs:424-590`
- Modify: `crates/rssh-terminal/src/parser.rs:639-675`
- Modify: `crates/rssh-terminal/src/parser.rs:2475-2520`
- Modify: `crates/rssh-terminal/src/parser.rs:2845-2865`
- Test: `crates/rssh-terminal/src/lib.rs`

**Required public model:**

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

`TerminalGrid` owns one `last_change_seqno` entry per row.
`ScrollbackLine` owns the sequence of the row captured into history.
`Terminal` owns:

```rust
seqno: SequenceNo,
main_stable_row_offset: StableRowIndex,
```

Use a non-zero initial sequence. Increment with checked arithmetic at each
public input batch, each resize call, and public scrollback erase call.

**Step 1: Write the failing stable-dimensions tests**

Add tests named:

```text
terminal_stable_dimensions_start_on_main_screen
terminal_stable_row_conversion_is_strict
terminal_alternate_dimensions_have_no_main_scrollback
terminal_stable_bottom_and_viewport_range_are_checked
terminal_stable_range_retention_is_strict
```

They must assert:

- a fresh main terminal has `scrollback_top == 0`,
  `physical_top == 0`, and retained rows equal viewport rows;
- history index zero maps to stable row zero and round-trips;
- a stable row outside the retained range returns `None`;
- alternate dimensions use `domain == Alternate`,
  `scrollback_top == 0`, `physical_top == 0`, and no main history.
- `stable_bottom_exclusive` returns `None` on checked overflow;
- `viewport_stable_range(None)` is the physical viewport ending at the checked
  stable bottom, while an explicit retained top preserves that stable top;
- `is_stable_range_fully_retained` is true only when the entire half-open
  range is retained and returns false for partial, outside, reversed, or
  overflowed ranges.

**Step 2: Run the stable-dimensions tests and verify RED**

Run:

```powershell
cargo test -p rssh-terminal terminal_stable_dimensions -- --nocapture
cargo test -p rssh-terminal terminal_stable_row_conversion_is_strict -- --nocapture
cargo test -p rssh-terminal terminal_alternate_dimensions_have_no_main_scrollback -- --nocapture
cargo test -p rssh-terminal terminal_stable_bottom_and_viewport_range_ -- --nocapture
cargo test -p rssh-terminal terminal_stable_range_retention_ -- --nocapture
```

Expected: compile failure because the stable-row types/methods do not exist.

**Step 3: Implement the minimal stable model**

Add:

```rust
pub const fn current_seqno(&self) -> SequenceNo;
pub fn stable_dimensions(&self) -> TerminalStableDimensions;
pub fn retained_stable_range(&self) -> Range<StableRowIndex>;
pub fn stable_bottom_exclusive(&self) -> Option<StableRowIndex>;
pub fn viewport_stable_range(
    &self,
    top: Option<StableRowIndex>,
) -> Range<StableRowIndex>;
pub fn is_stable_range_fully_retained(
    &self,
    rows: Range<StableRowIndex>,
) -> bool;
pub fn history_index_to_stable_row(&self, row: usize) -> Option<StableRowIndex>;
pub fn stable_row_to_history_index(&self, row: StableRowIndex) -> Option<usize>;
```

Use checked conversions and checked addition. Do not clamp strict conversion.
Alternate-screen conversions must never expose main scrollback.

**Step 4: Run stable-dimensions tests and verify GREEN**

Run all five commands from Step 2.

Expected: all named tests run and pass.

**Step 5: Write failing sequence-boundary tests**

Add:

```text
terminal_sequence_starts_non_zero
terminal_feed_advances_sequence_once_per_batch
terminal_cursor_only_feed_advances_sequence_once
terminal_same_size_resize_advances_sequence_once
terminal_public_erase_advances_sequence_once
```

This task verifies only the terminal-sequence boundary. Task 2 verifies that
cursor-only feed and same-size resize do not change line sequences, while
ordinary feed and erase stamp only the rows they replace. Framebuffer
`DamageRegion` remains a separate concern.

**Step 6: Run sequence tests and verify RED**

Run:

```powershell
cargo test -p rssh-terminal terminal_sequence_ -- --nocapture
cargo test -p rssh-terminal terminal_public_erase_advances_sequence_once -- --nocapture
```

Expected: at least one assertion fails because sequence advancement is absent.

**Step 7: Implement sequence and row metadata**

- Advance the terminal sequence once at the start of `feed`, including an
  empty/cursor-only/control-only batch.
- Advance once at the start of `resize`, including same-size resize.
- Advance once at the public `erase_scrollback_and_viewport` boundary; internal
  prune/replacement helpers must reuse that sequence and must not increment it
  again.
- Initialize and resize `TerminalGrid` row-sequence storage.
- Preserve sequence metadata in `ScrollbackLine`.
- Provide crate-private row sequence getters/setters needed by later tasks.
- Keep `DamageRegion` behavior separate.

**Step 8: Run task tests and crate regression**

Run:

```powershell
cargo test -p rssh-terminal terminal_sequence_ -- --nocapture
cargo test -p rssh-terminal terminal_public_erase_advances_sequence_once -- --nocapture
cargo test -p rssh-terminal terminal_stable_ -- --nocapture
cargo test -p rssh-terminal
cargo fmt --all -- --check
git diff --check
```

Expected: all pass.

**Step 9: Commit**

```powershell
git add crates/rssh-terminal/src/lib.rs crates/rssh-terminal/src/parser.rs
git commit -m "feat: add terminal stable row primitives"
```

### Task 2: Make scrolling, pruning, and changed-row queries stable

**Files:**

- Modify: `crates/rssh-terminal/src/lib.rs:213-349`
- Modify: `crates/rssh-terminal/src/parser.rs:2792-2865`
- Modify: `crates/rssh-terminal/src/parser.rs:3116-3195`
- Modify: `crates/rssh-terminal/src/parser.rs:3801-3845`
- Modify: `crates/rssh-terminal/src/parser.rs:4309-4570`
- Modify: `crates/rssh-terminal/src/parser.rs:4765-4785`
- Test: `crates/rssh-terminal/src/lib.rs`

**Required internal split:**

Create one family that moves a row while preserving content identity and line
sequence, and one family that replaces/copies a stable slot and stamps the
current sequence. Names may differ, but the distinction must be explicit and
centralized.

**Step 1: Write failing full-scroll identity tests**

Add:

```text
terminal_full_screen_scroll_preserves_stable_row_identity
terminal_full_screen_scroll_marks_only_new_bottom_row_changed
terminal_top_anchored_short_su_records_history_and_dirties_suffix
terminal_row_zero_delete_line_records_history
terminal_top_zero_narrow_margin_scroll_does_not_record_history
```

The tests must compare stable IDs, text, row sequences, `physical_top`, and
`changed_stable_rows_since`.

**Step 2: Run scroll tests and verify RED**

Run:

```powershell
cargo test -p rssh-terminal terminal_full_screen_scroll_ -- --nocapture
cargo test -p rssh-terminal terminal_top_anchored_short_su_ -- --nocapture
cargo test -p rssh-terminal terminal_row_zero_delete_line_ -- --nocapture
cargo test -p rssh-terminal terminal_top_zero_narrow_margin_ -- --nocapture
```

Expected: failures because changed-row queries and unified scrollback
eligibility are absent.

**Step 3: Implement upward-scroll eligibility and row movement**

Use this exact eligibility rule for recording main-screen history:

```text
main screen
AND scrollback allowed
AND effective vertical top == 0
AND left/right margins span the full width
```

Apply it consistently to LF/IND, SU/CSI `S`, and DL at row zero. A short
top-anchored region must dirty the stable-slot suffix below the region.
Non-top-anchored, narrow-margin, and alternate-screen operations never add
main history and stamp every replaced destination row.

**Step 4: Implement changed-row query**

Add:

```rust
pub fn changed_stable_rows_since(
    &self,
    rows: Range<StableRowIndex>,
    seqno: SequenceNo,
) -> Vec<StableRowIndex>;
```

It must:

- intersect with the retained range without clamping identities;
- return sorted unique rows;
- use `line_seqno == 0 || line_seqno > seqno`;
- report only the active screen domain.

**Step 5: Run scroll tests and verify GREEN**

Run the commands from Step 2.

Expected: all named tests pass.

**Step 6: Write failing pruning and erase tests**

Add:

```text
terminal_scrollback_prune_advances_stable_top_without_retargeting
terminal_zero_scrollback_limit_keeps_ids_monotonic
terminal_runtime_limit_reduction_preserves_survivor_ids
terminal_ed3_removes_history_without_dirtying_visible_rows
terminal_erase_scrollback_and_viewport_prunes_then_dirties_replaced_rows
terminal_ris_prunes_history_without_stable_retargeting_and_dirties_new_grid
terminal_prune_rebases_semantic_metadata_without_retargeting
terminal_prune_rebases_inline_image_metadata
terminal_prune_rebases_kitty_placeholder_metadata
```

The old stable row must map to `None` after prune. A survivor must retain its
stable ID. ED3 must not dirty unchanged visible rows.

**Step 7: Run pruning tests and verify RED**

Run:

```powershell
cargo test -p rssh-terminal terminal_scrollback_prune_ -- --nocapture
cargo test -p rssh-terminal terminal_zero_scrollback_limit_ -- --nocapture
cargo test -p rssh-terminal terminal_runtime_limit_reduction_ -- --nocapture
cargo test -p rssh-terminal terminal_ed3_ -- --nocapture
cargo test -p rssh-terminal terminal_erase_scrollback_and_viewport_ -- --nocapture
cargo test -p rssh-terminal terminal_ris_prunes_history_ -- --nocapture
cargo test -p rssh-terminal terminal_prune_rebases_semantic_metadata_ -- --nocapture
cargo test -p rssh-terminal terminal_prune_rebases_inline_image_ -- --nocapture
cargo test -p rssh-terminal terminal_prune_rebases_kitty_placeholder_ -- --nocapture
```

Expected: assertions fail because stable top does not advance and strict old
row lookup can retarget.

**Step 8: Implement one pruning path**

Route capacity trim, limit reduction, ED3, RIS/reset, and limit-zero scroll
through one helper that:

- advances `main_stable_row_offset`;
- removes oldest `ScrollbackLine` values;
- rebases semantic prompt rows and command exits;
- rebases inline image and Kitty placeholder physical metadata;
- leaves surviving stable IDs and line sequences unchanged.

Treat `erase_scrollback_and_viewport` as prune followed by viewport replacement
and dirty stamping. RIS/reset must also prune through this helper so old stable
rows cannot retarget into the reset grid.

**Step 9: Add direct dirty-metadata tests**

Add and first run failing tests for:

```text
terminal_cursor_only_batch_does_not_mark_lines_changed
terminal_same_size_resize_does_not_mark_lines_changed
terminal_placeholder_cell_assignment_marks_row_changed
terminal_mark_all_lines_changed_marks_active_domain_rows_changed
terminal_public_mark_all_lines_changed_advances_sequence_once
terminal_feed_with_all_lines_changed_advances_sequence_once
terminal_ris_with_whole_line_dirty_advances_sequence_once
terminal_partial_region_scroll_marks_only_affected_slots_changed
terminal_alternate_scroll_marks_alt_slots_changed_without_main_history
```

The partial-region test must be table-driven across non-top SU, SD, IL, and
DL, plus top-zero narrow horizontal margins. It must assert both that every
replaced destination stable slot is stamped and every unaffected slot retains
its prior line sequence.

Run every named test before implementation:

```powershell
cargo test -p rssh-terminal terminal_cursor_only_batch_ -- --nocapture
cargo test -p rssh-terminal terminal_same_size_resize_does_not_ -- --nocapture
cargo test -p rssh-terminal terminal_placeholder_cell_assignment_ -- --nocapture
cargo test -p rssh-terminal terminal_mark_all_lines_changed_ -- --nocapture
cargo test -p rssh-terminal terminal_public_mark_all_lines_changed_ -- --nocapture
cargo test -p rssh-terminal terminal_feed_with_all_lines_changed_ -- --nocapture
cargo test -p rssh-terminal terminal_ris_with_whole_line_dirty_ -- --nocapture
cargo test -p rssh-terminal terminal_partial_region_scroll_ -- --nocapture
cargo test -p rssh-terminal terminal_alternate_scroll_ -- --nocapture
```

Expected: the missing query/whole-line boundary fails to compile or the dirty
row assertions fail.

**Step 10: Implement missing row dirty stamping**

Ensure cell/attribute writes, wrapped flags, placeholder cells, row
width/reflow changes, and existing `make_all_lines_dirty`-equivalent calls
stamp row sequences. Image metadata that does not touch cells remains render
damage only. Split whole-line stamping into:

- a private `mark_all_lines_changed_at_current_seqno()` helper that only stamps
  every row in the active screen domain;
- a public `Terminal::mark_all_lines_changed()` wrapper that
  checked-increments the sequence once, then calls the private helper.
- a public `Terminal::feed_with_all_lines_changed(bytes)` input boundary that
  checked-increments once, processes the bytes without another increment, then
  calls the private helper at that same sequence. `feed(bytes)` delegates to
  the same internal batch path without whole-line stamping.

All feed-batch internal paths, including RIS/reset and OSC palette/style
changes, must call the private helper so the public feed boundary still
advances exactly once. The standalone app palette/config boundary calls the
public wrapper. Do not add a terminal palette/configuration API.

Run every command from Step 9 again and verify GREEN before the broader
regression.

**Step 11: Run task tests and crate regression**

Run:

```powershell
cargo test -p rssh-terminal terminal_full_screen_scroll_ -- --nocapture
cargo test -p rssh-terminal terminal_top_anchored_ -- --nocapture
cargo test -p rssh-terminal terminal_row_zero_delete_line_ -- --nocapture
cargo test -p rssh-terminal terminal_scrollback_prune_ -- --nocapture
cargo test -p rssh-terminal terminal_ed3_ -- --nocapture
cargo test -p rssh-terminal terminal_ris_prunes_history_ -- --nocapture
cargo test -p rssh-terminal terminal_prune_rebases_semantic_metadata_ -- --nocapture
cargo test -p rssh-terminal terminal_prune_rebases_inline_image_ -- --nocapture
cargo test -p rssh-terminal terminal_prune_rebases_kitty_placeholder_ -- --nocapture
cargo test -p rssh-terminal terminal_placeholder_cell_assignment_ -- --nocapture
cargo test -p rssh-terminal terminal_mark_all_lines_changed_ -- --nocapture
cargo test -p rssh-terminal terminal_public_mark_all_lines_changed_ -- --nocapture
cargo test -p rssh-terminal terminal_feed_with_all_lines_changed_ -- --nocapture
cargo test -p rssh-terminal terminal_ris_with_whole_line_dirty_ -- --nocapture
cargo test -p rssh-terminal terminal_partial_region_scroll_ -- --nocapture
cargo test -p rssh-terminal terminal_alternate_scroll_ -- --nocapture
cargo test -p rssh-terminal
cargo fmt --all -- --check
git diff --check
```

Expected: all pass.

**Step 12: Commit**

```powershell
git add crates/rssh-terminal/src/lib.rs crates/rssh-terminal/src/parser.rs
git commit -m "feat: track stable terminal row changes"
```

### Task 3: Add stable text, semantic, and screen-domain APIs

**Files:**

- Modify: `crates/rssh-terminal/src/lib.rs:81-126`
- Modify: `crates/rssh-terminal/src/parser.rs:2490-2725`
- Modify: `crates/rssh-terminal/src/parser.rs:2792-2865`
- Modify: `crates/rssh-terminal/src/parser.rs:3801-3845`
- Test: `crates/rssh-terminal/src/lib.rs`

**Step 1: Write failing stable text tests**

Add:

```text
terminal_stable_text_reads_offscreen_rows
terminal_stable_text_returns_surviving_partial_prefix_prune
terminal_stable_text_returns_surviving_partial_suffix_prune
terminal_stable_text_reverse_anchor_focus_survives_partial_prune
terminal_stable_text_rejects_mixed_or_inactive_domains
terminal_stable_rectangular_text_keeps_original_columns_after_prune
terminal_stable_soft_wrapped_text_joins_surviving_spans
terminal_stable_text_fully_pruned_returns_none
```

Use inclusive stable endpoints and verify:

- non-rectangular surviving prefix starts at column zero when the original
  first endpoint was pruned;
- surviving suffix reaches line end when the original final endpoint was
  pruned;
- rectangular selection always uses the original columns;
- removed rows never map to the new oldest row.
- mixed start/end domains and a range outside the active domain return `None`.

**Step 2: Run stable text tests and verify RED**

Run:

```powershell
cargo test -p rssh-terminal terminal_stable_text_ -- --nocapture
cargo test -p rssh-terminal terminal_stable_rectangular_text_ -- --nocapture
cargo test -p rssh-terminal terminal_stable_soft_wrapped_text_ -- --nocapture
```

Expected: compile failure because stable selection extraction does not exist.

**Step 3: Implement stable text extraction**

Add this public terminal-level model:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StableSelectionCoordinate {
    pub domain: TerminalScreenDomain,
    pub row: StableRowIndex,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StableSelectionRange {
    pub start: StableSelectionCoordinate,
    pub end: StableSelectionCoordinate,
    pub rectangular: bool,
}

pub fn text_from_stable_selection(
    &self,
    selection: StableSelectionRange,
) -> Option<String>;
```

Single-row conversion stays strict. Range extraction intersects the original
stable row range with retained rows and applies the original endpoint column
semantics, including when anchor and focus are reversed. Preserve existing
`text_from_region` soft-wrap/trailing-space behavior; do not duplicate its
line-formatting rules.

**Step 4: Run stable text tests and verify GREEN**

Run the commands from Step 2.

Expected: all pass.

**Step 5: Write failing screen-domain and resize tests**

Add:

```text
terminal_screen_domain_changes_on_alternate_switch
terminal_alternate_stable_text_never_reads_main_history
terminal_height_resize_reports_identity_boundary
terminal_width_resize_marks_replaced_rows_changed
```

The terminal need only expose a domain/dimension transition for height changes;
GUI selection retirement is tested later in `rssh-app`.

Run before implementation:

```powershell
cargo test -p rssh-terminal terminal_screen_domain_ -- --nocapture
cargo test -p rssh-terminal terminal_alternate_stable_text_ -- --nocapture
cargo test -p rssh-terminal terminal_height_resize_ -- --nocapture
cargo test -p rssh-terminal terminal_width_resize_ -- --nocapture
```

Expected: missing APIs fail to compile or current identity/dirty assertions
fail.

**Step 6: Implement domain and resize boundary APIs, then verify GREEN**

- Keep main and alternate stable dimensions separate.
- Never sample main history while alternate is active.
- Expose enough before/after state for `rssh-app` to detect screen-domain and
  height changes synchronously.
- Width changes stamp affected rows at the resize sequence.
- Same-size resize increments sequence but does not dirty unchanged rows.

Run the four Step 5 commands again. Expected: all named tests run and pass.

**Step 7: Write failing stable semantic API tests**

Add:

```text
terminal_stable_semantic_prompt_rows_survive_history_growth
terminal_stable_semantic_zones_survive_prune_without_retargeting
terminal_stable_semantic_zone_at_uses_stable_row
terminal_stable_semantic_command_exits_survive_prune
```

Run:

```powershell
cargo test -p rssh-terminal terminal_stable_semantic_ -- --nocapture
```

Expected: compile failure because the additive stable semantic model does not
exist.

**Step 8: Add stable semantic APIs without breaking app callers**

Keep the existing ordinal semantic API public through Task 3 so
`rssh-app` continues to compile. Keep physical internal storage if that
minimizes churn, and add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableSemanticZone {
    pub start_x: usize,
    pub start_y: StableRowIndex,
    pub end_x: usize,
    pub end_y: StableRowIndex,
    pub semantic_type: SemanticType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableSemanticCommandExit {
    pub row: StableRowIndex,
    pub exit_code: Option<i32>,
    pub aid: Option<String>,
}

pub fn stable_semantic_prompt_rows(&self) -> Vec<StableRowIndex>;
pub fn stable_semantic_zones(&self) -> Vec<StableSemanticZone>;
pub fn stable_semantic_zone_at(
    &self,
    column: usize,
    row: StableRowIndex,
) -> Option<StableSemanticZone>;
pub fn stable_semantic_command_exits(&self) -> Vec<StableSemanticCommandExit>;
```

Reuse the existing `SemanticType`, `Option<i32>` exit code, and `Option<String>`
aid payloads exactly; every row field must be stable. Task 4 migrates app
callers to these additive APIs; only then may ordinal wrappers be made
crate-private or removed.

Run:

```powershell
cargo test -p rssh-terminal terminal_stable_semantic_ -- --nocapture
cargo check -p rssh-app
```

Expected: stable semantic tests pass and the unchanged app still compiles.

**Step 9: Run task tests and crate regression**

Run:

```powershell
cargo test -p rssh-terminal terminal_stable_ -- --nocapture
cargo test -p rssh-terminal terminal_screen_domain_ -- --nocapture
cargo test -p rssh-terminal terminal_alternate_stable_text_ -- --nocapture
cargo test -p rssh-terminal terminal_height_resize_ -- --nocapture
cargo test -p rssh-terminal terminal_width_resize_ -- --nocapture
cargo test -p rssh-terminal semantic_zone -- --nocapture
cargo test -p rssh-terminal
cargo test -p rssh-app
cargo fmt --all -- --check
git diff --check
```

Expected: all pass.

**Step 10: Commit**

```powershell
git add crates/rssh-terminal/src/lib.rs crates/rssh-terminal/src/parser.rs
git commit -m "feat: expose stable terminal text ranges"
```

### Task 4: Migrate pane viewports and transient coordinates to stable rows

**Files:**

- Modify: `crates/rssh-app/src/window.rs:80663-80800`
- Modify: `crates/rssh-app/src/window.rs:81367-81420`
- Modify: `crates/rssh-app/src/window.rs:84249-84365`
- Modify: `crates/rssh-app/src/window.rs:87700-88230`
- Modify: `crates/rssh-app/src/window.rs:89221-89282`
- Modify: `crates/rssh-app/src/window.rs:93862-94320`
- Modify: `crates/rssh-app/src/window.rs:95320-96610`
- Modify: `crates/rssh-app/src/window.rs:99630-100335`
- Modify: `crates/rssh-app/src/window.rs:118780-119390`
- Test: `crates/rssh-app/src/window.rs`

**Required viewport model:**

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PaneStableViewport {
    main_top: Option<StableRowIndex>,
}
```

Alternate rendering always uses bottom/`None`. The dormant main top is
preserved and clamped when main becomes active again. Existing scrollbar
offsets become derived presentation values.

**Step 1: Write failing stable viewport tests**

Add:

```text
window_app_wheel_updates_stable_viewport_top
window_app_page_scroll_updates_stable_viewport_top
window_app_scrollbar_drag_updates_stable_viewport_top
window_app_scroll_to_prompt_updates_stable_viewport_top
window_app_scrolled_back_viewport_stays_on_same_stable_top_after_output
window_app_main_viewport_restores_after_alternate_screen
window_app_prune_clamps_stable_viewport
window_app_active_and_inactive_stable_viewports_are_independent
```

These tests cover viewport identity only. Ordinary selection remains
viewport-local until Task 5 and may continue to clear on viewport movement at
this intermediate commit.

**Step 2: Run viewport tests and verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_wheel_updates_stable_viewport_ -- --nocapture
cargo test -p rssh-app window_app_page_scroll_updates_stable_viewport_ -- --nocapture
cargo test -p rssh-app window_app_scrollbar_drag_updates_stable_viewport_ -- --nocapture
cargo test -p rssh-app window_app_scroll_to_prompt_updates_stable_viewport_ -- --nocapture
cargo test -p rssh-app window_app_scrolled_back_viewport_ -- --nocapture
cargo test -p rssh-app window_app_main_viewport_restores_ -- --nocapture
cargo test -p rssh-app window_app_prune_clamps_stable_viewport -- --nocapture
cargo test -p rssh-app window_app_active_and_inactive_stable_viewports_ -- --nocapture
```

Expected: assertions fail because the viewport is stored as a relative offset
and drifts or cannot restore by stable top.

**Step 3: Implement stable viewport storage**

- Replace long-lived `scrollback_offset` ownership in `NativeWindowApp` and
  `PaneRuntime` with `PaneStableViewport`.
- Add helpers that derive current offset and visible stable top from terminal
  dimensions.
- Normalize a requested main top:
  - below retained top → retained top;
  - `row >= physical_top` → bottom/`None`;
  - alternate or zero-sized viewport → bottom/`None`.
- Preserve dormant main top while alternate is active.
- Keep the single window-right scrollbar bound to the active pane and convert
  pointer offsets once at that boundary.
- At this intermediate commit, preserve the old ordinary viewport-local
  selection clearing boundary; Task 5 removes it after ordinary endpoints are
  stable. Migrated Search/Copy/Quick coordinates must not be discarded solely
  because the viewport moves.

**Step 4: Write failing stable transient-coordinate tests**

Convert:

- `SelectionSourceCell`/`WindowSourceSelection`;
- `WindowCopyMode.source_cursor`/`source_anchor`;
- `WindowSearchMatch.source_row`/`end_source_row`;
- Quick Select stored matches and viewport projections.

Add:

```text
window_app_copy_mode_cursor_survives_history_growth
window_app_search_matches_do_not_retarget_after_prune
window_app_quick_select_matches_do_not_retarget_after_prune
```

Run:

```powershell
cargo test -p rssh-app window_app_copy_mode_cursor_survives_ -- --nocapture
cargo test -p rssh-app window_app_search_matches_do_not_retarget_ -- --nocapture
cargo test -p rssh-app window_app_quick_select_matches_do_not_retarget_ -- --nocapture
```

Expected: current relative/physical coordinate storage drifts or retargets.

**Step 5: Migrate transient coordinates and stable semantic callers**

Long-lived fields carry `TerminalScreenDomain` where needed. Migrate all app
semantic prompt/zone/exit reads to Task 3's additive stable APIs. Recompute or
discard transient matches when their original stable rows are no longer
retained.

Run the three Step 4 commands again. Expected: all pass.

**Step 6: Update active/inactive runtime transfer**

`take_active_runtime` and `install_active_runtime` transfer stable viewport.
Ordinary selection remains under its pre-Task-5 ownership/clearing behavior.
Pane switching still ends drag and current window-global transient controller
state, and it must not promote a transient highlight to ordinary selection.

**Step 7: Run task tests and app regression**

Run:

```powershell
cargo test -p rssh-app stable_viewport -- --nocapture
cargo test -p rssh-app window_app_wheel_updates_stable_viewport_ -- --nocapture
cargo test -p rssh-app window_app_scroll_to_prompt_updates_stable_viewport_ -- --nocapture
cargo test -p rssh-app window_app_scrolled_back_viewport_ -- --nocapture
cargo test -p rssh-app window_app_main_viewport_restores_ -- --nocapture
cargo test -p rssh-app window_app_prune_clamps_stable_viewport -- --nocapture
cargo test -p rssh-app window_app_copy_mode_cursor_survives_ -- --nocapture
cargo test -p rssh-app window_app_search_matches_do_not_retarget_ -- --nocapture
cargo test -p rssh-app window_app_quick_select_matches_do_not_retarget_ -- --nocapture
cargo test -p rssh-app pane_focus -- --nocapture
cargo test -p rssh-app copy_mode -- --nocapture
cargo test -p rssh-app search -- --nocapture
cargo test -p rssh-app quick_select -- --nocapture
cargo test -p rssh-app
cargo fmt --all -- --check
git diff --check
```

Expected: all pass.

**Step 8: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: retain stable pane viewport coordinates"
```

### Task 5: Store and render ordinary selections in stable rows

**Files:**

- Modify: `crates/rssh-app/src/window.rs:80748-80770`
- Modify: `crates/rssh-app/src/window.rs:81367-81380`
- Modify: `crates/rssh-app/src/window.rs:84239-84365`
- Modify: `crates/rssh-app/src/window.rs:89094-89175`
- Modify: `crates/rssh-app/src/window.rs:90145-90170`
- Modify: `crates/rssh-app/src/window.rs:93862-94320`
- Modify: `crates/rssh-app/src/window.rs:98115-98135`
- Modify: `crates/rssh-app/src/window.rs:100175-100550`
- Modify: `crates/rssh-app/src/window.rs:119677-119705`
- Test: `crates/rssh-app/src/window.rs`

**Required state split:**

Create a stable ordinary type containing:

```text
screen domain
anchor stable row/column
focus stable row/column
rectangular
selection seqno
```

Keep the existing viewport-local `WindowSelection` (rename if useful) only for
presentation. Do not reuse ordinary storage for Search/Copy/Quick highlights.

**Step 1: Write failing offscreen and lifecycle selection tests**

Add:

```text
window_app_ordinary_selection_survives_scrolling_out_and_back
window_app_wheel_keeps_ordinary_selection_while_viewport_moves
window_app_page_scroll_keeps_ordinary_selection_while_viewport_moves
window_app_scrollbar_drag_keeps_ordinary_selection_while_viewport_moves
window_app_ordinary_selection_copies_offscreen_stable_text
window_app_ordinary_selection_survives_full_screen_scroll_into_history
window_app_ordinary_selection_partial_prune_copies_only_surviving_rows
window_app_ordinary_rectangular_selection_keeps_columns_after_prune
window_app_ordinary_soft_wrap_selection_uses_stable_rows
window_app_fully_pruned_selection_never_copies_new_oldest_row
window_app_multi_click_cache_uses_stable_rows
window_app_focus_switch_restores_each_stable_selection
window_app_new_split_starts_without_stable_selection
window_app_close_removes_only_closed_stable_selection
window_app_move_to_new_tab_preserves_stable_selection_and_viewport
window_app_move_to_new_window_clears_gui_selection_but_preserves_stable_viewport
window_app_transient_match_never_becomes_ordinary_stable_selection
```

**Step 2: Run selection tests and verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_ordinary_selection_ -- --nocapture
cargo test -p rssh-app window_app_wheel_keeps_ordinary_selection_ -- --nocapture
cargo test -p rssh-app window_app_page_scroll_keeps_ordinary_selection_ -- --nocapture
cargo test -p rssh-app window_app_scrollbar_drag_keeps_ordinary_selection_ -- --nocapture
cargo test -p rssh-app window_app_fully_pruned_selection_ -- --nocapture
cargo test -p rssh-app window_app_multi_click_cache_uses_stable_rows -- --nocapture
cargo test -p rssh-app window_app_focus_switch_restores_each_stable_selection -- --nocapture
cargo test -p rssh-app window_app_new_split_starts_without_stable_selection -- --nocapture
cargo test -p rssh-app window_app_close_removes_only_closed_stable_selection -- --nocapture
cargo test -p rssh-app window_app_move_to_new_tab_preserves_stable_selection_ -- --nocapture
cargo test -p rssh-app window_app_move_to_new_window_clears_gui_selection_ -- --nocapture
cargo test -p rssh-app window_app_transient_match_never_becomes_ordinary_ -- --nocapture
```

Expected: current viewport selection clears, copies only snapshot contents, or
retargets retained ordinals.

**Step 3: Implement stable ordinary input and projection**

- Convert mouse viewport rows to stable rows using the pane's visible stable
  top.
- Convert mouse selection cells and `WindowClick`/multi-click caches to stable
  coordinates; keep any viewport-local `WindowSelection` only as a derived
  paint-time projection.
- Record terminal sequence whenever an ordinary selection begins or extends.
- Convert word, line, semantic-zone, drag, Shift extension, and block selection
  helpers to stable endpoints.
- Project stable ordinary selection into the current viewport immediately
  before palette application.
- Preserve the established selection → foreground HSB → opacity →
  inactive-pane HSB → minimum-contrast order.
- Avoid active-pane double overlay.

**Step 4: Implement terminal-backed selected text**

`selected_text()` reads stable terminal rows, not `TerminalRenderSnapshot`.
Search `CurrentSelectionOrEmptyString` reads ordinary text before creating
transient state. Copy Mode and Quick Select actions continue to read their own
stable transient range.

**Step 5: Run selection tests and verify GREEN**

Run the commands from Step 2 plus:

```powershell
cargo test -p rssh-app selection -- --nocapture
```

Expected: all pass.

**Step 6: Verify pane ownership lifecycle GREEN**

Run the lifecycle commands from Step 2 again. Expected: all pass. Preserve the
existing GUI-window selection-clearing boundary while retaining pane runtime
and stable viewport as specified.

**Step 7: Run task regression**

Run:

```powershell
cargo test -p rssh-app selection -- --nocapture
cargo test -p rssh-app pane_focus -- --nocapture
cargo test -p rssh-app pane_select -- --nocapture
cargo test -p rssh-app copy_mode -- --nocapture
cargo test -p rssh-app search -- --nocapture
cargo test -p rssh-app quick_select -- --nocapture
cargo test -p rssh-app
cargo fmt --all -- --check
git diff --check
```

Expected: all pass.

**Step 8: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: store ordinary selection in stable rows"
```

### Task 6: Add paint-time dirty invalidation and identity-boundary retirement

**Files:**

- Modify: `crates/rssh-app/src/terminal_runtime.rs:65-150`
- Modify: `crates/rssh-app/src/terminal_runtime.rs:300-630`
- Modify: `crates/rssh-app/src/terminal_runtime.rs:2558-2675`
- Modify: `crates/rssh-app/src/window.rs:83450-83500`
- Modify: `crates/rssh-app/src/window.rs:84239-84365`
- Modify: `crates/rssh-app/src/window.rs:88913-89175`
- Modify: `crates/rssh-app/src/window.rs:90145-90170`
- Modify: `crates/rssh-app/src/window.rs:93660-93695`
- Modify: `crates/rssh-app/src/window.rs:97771-97820`
- Test: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/terminal_runtime.rs`

**Step 1: Write failing active/inactive invalidation tests**

Add:

```text
window_app_visible_dirty_selected_row_clears_ordinary_selection_on_paint
window_app_visible_dirty_unselected_row_preserves_ordinary_selection
window_app_offscreen_dirty_selected_row_waits_until_visible_paint
window_app_full_screen_scroll_preserves_unchanged_selected_row
window_app_inactive_visible_dirty_selected_row_clears_only_that_pane_selection
window_app_inactive_dirty_unselected_row_preserves_selection
window_app_ed3_preserves_unchanged_visible_selection
```

The inactive tests must rebuild that pane's base/presentation snapshot and
must not rely on focusing the pane first.

**Step 2: Run invalidation tests and verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_visible_dirty_ -- --nocapture
cargo test -p rssh-app window_app_offscreen_dirty_ -- --nocapture
cargo test -p rssh-app window_app_full_screen_scroll_preserves_ -- --nocapture
cargo test -p rssh-app window_app_inactive_visible_dirty_ -- --nocapture
cargo test -p rssh-app window_app_inactive_dirty_unselected_ -- --nocapture
cargo test -p rssh-app window_app_ed3_preserves_ -- --nocapture
```

Expected: stale selections remain, or current output handling clears too
broadly.

**Step 3: Implement one ordinary invalidation helper**

The helper receives:

```text
terminal
current visible stable range
ordinary selection
overlay exemption
```

It:

- returns early for Search/Copy/Quick overlay exemption;
- asks the terminal only for visible rows changed since selection seqno;
- clears only when a changed visible row intersects the selection row range;
- does not refresh selection seqno merely because an overlay exits;
- runs for active and inactive pane presentation before selection projection.

Do not derive invalidation from `DamageRegion`.

Before continuing, add:

```text
terminal_runtime_osc_palette_change_marks_active_domain_rows_changed
terminal_runtime_osc_palette_change_advances_sequence_once
terminal_runtime_palette_query_and_noop_do_not_mark_lines_changed
window_app_config_palette_change_marks_all_pane_active_domains_changed
```

Run:

```powershell
cargo test -p rssh-app terminal_runtime_osc_palette_change_ -- --nocapture
cargo test -p rssh-app terminal_runtime_palette_query_and_noop_ -- --nocapture
cargo test -p rssh-app window_app_config_palette_change_marks_all_pane_ -- --nocapture
```

Expected: runtime OSC palette mutations and config palette changes do not yet
stamp terminal rows.

Implement the real boundaries:

- Make `TerminalColorState::process` report whether an effective OSC 4/104 or
  dynamic color mutation/reset changed state. Queries and no-op assignments
  report false.
- Carry that flag with the corresponding filtered display batch and call
  `Terminal::feed_with_all_lines_changed` instead of `feed`, so the input
  sequence advances exactly once.
- Expose a `TerminalRuntime::mark_all_lines_changed` delegating to the
  standalone terminal public wrapper.
- When a config reload/replacement changes the resolved palette, call that
  runtime method for the active runtime and every inactive pane runtime before
  rebuilding presentation snapshots.

Run the three commands again and verify GREEN.

**Step 4: Write failing overlay accumulation tests**

Add separate tests:

```text
window_app_search_overlay_defers_real_dirty_selection_invalidation
window_app_copy_mode_overlay_defers_real_dirty_selection_invalidation
window_app_quick_select_overlay_defers_real_dirty_selection_invalidation
```

Each test performs:

1. ordinary selection;
2. enter exactly one overlay family;
3. real PTY modification of a selected visible row;
4. confirm no ordinary invalidation while overlay active;
5. exit overlay;
6. confirm next underlying paint invalidates using the original selection
   sequence.

**Step 5: Run overlay tests and verify RED**

Run:

```powershell
cargo test -p rssh-app window_app_search_overlay_defers_real_dirty_ -- --nocapture
cargo test -p rssh-app window_app_copy_mode_overlay_defers_real_dirty_ -- --nocapture
cargo test -p rssh-app window_app_quick_select_overlay_defers_real_dirty_ -- --nocapture
```

Expected: current transient/ordinary selection sharing or sequence refresh
violates at least one assertion.

**Step 6: Implement explicit ordinary/transient provenance**

If Task 5 did not already separate storage completely, do it now. Overlay
projection may visually replace ordinary selection, but it must not overwrite
ordinary endpoints or seqno. Exiting an overlay exposes the original ordinary
state for accumulated dirty checking.

Run the three Step 5 commands again. Expected: all pass.

**Step 7: Add identity-boundary tests**

Add:

```text
window_app_active_height_change_retires_selection_before_copy
window_app_inactive_height_change_retires_selection_before_focus
window_app_main_to_alt_retires_selection_before_projection
window_app_alt_to_main_does_not_revive_selection
window_app_screen_switch_retires_transient_and_multiclick_state
window_app_main_viewport_restores_after_alt_selection_retirement
```

The assertions must occur before a paint where noted, proving that projection,
extraction, or callback paths cannot consume a coordinate from the wrong
identity domain.

Run before implementation:

```powershell
cargo test -p rssh-app window_app_active_height_change_retires_ -- --nocapture
cargo test -p rssh-app window_app_inactive_height_change_retires_ -- --nocapture
cargo test -p rssh-app window_app_main_to_alt_retires_ -- --nocapture
cargo test -p rssh-app window_app_alt_to_main_does_not_revive_ -- --nocapture
cargo test -p rssh-app window_app_screen_switch_retires_ -- --nocapture
cargo test -p rssh-app window_app_main_viewport_restores_after_alt_selection_ -- --nocapture
```

Expected: at least one stale identity remains observable.

**Step 8: Implement synchronous retirement and verify GREEN**

Detect terminal height and `TerminalScreenDomain` transitions in active and
inactive output/resize paths. Retire:

- ordinary selection;
- Search/Copy/Quick stable transient coordinates/controller state as required
  by the current window-global boundary;
- drag/selecting state;
- `WindowClick` and related multi-click caches.

Preserve dormant main viewport across alternate mode and restore/clamp it on
return.

Run the six Step 7 commands again. Expected: all pass.

**Step 9: Update Lua pane dimensions and cursor coordinates**

Add failing tests for:

```text
window_app_lua_pane_dimensions_use_stable_scrollback_top
window_app_lua_pane_dimensions_use_stable_physical_top
window_app_lua_pane_cursor_y_uses_stable_row
```

Run:

```powershell
cargo test -p rssh-app window_app_lua_pane_dimensions_use_stable_ -- --nocapture
cargo test -p rssh-app window_app_lua_pane_cursor_y_uses_stable_row -- --nocapture
```

Expected: current ordinal values fail the stable-row assertions. Update
`lua_pane_dimensions_field_text` and any viewport-derived Lua/action
coordinate output. Do not invent a viewport-top Lua field that upstream does
not expose. Run both commands again and verify GREEN.

**Step 10: Run task regression**

Run:

```powershell
cargo test -p rssh-app window_app_visible_dirty_ -- --nocapture
cargo test -p rssh-app window_app_inactive_visible_dirty_ -- --nocapture
cargo test -p rssh-app window_app_inactive_dirty_unselected_ -- --nocapture
cargo test -p rssh-app overlay_defers_real_dirty -- --nocapture
cargo test -p rssh-app terminal_runtime_osc_palette_change_ -- --nocapture
cargo test -p rssh-app terminal_runtime_palette_query_and_noop_ -- --nocapture
cargo test -p rssh-app window_app_config_palette_change_marks_all_pane_ -- --nocapture
cargo test -p rssh-app height_change_retires -- --nocapture
cargo test -p rssh-app screen_switch_retires -- --nocapture
cargo test -p rssh-app lua_pane_dimensions_use_stable -- --nocapture
cargo test -p rssh-app selection -- --nocapture
cargo test -p rssh-app copy_mode -- --nocapture
cargo test -p rssh-app search -- --nocapture
cargo test -p rssh-app quick_select -- --nocapture
cargo test -p rssh-app scrollback -- --nocapture
cargo test -p rssh-app
cargo fmt --all -- --check
git diff --check
```

Expected: all pass.

**Step 11: Commit**

```powershell
git add crates/rssh-app/src/terminal_runtime.rs crates/rssh-app/src/window.rs
git commit -m "feat: invalidate stable selections on dirty rows"
```

### Task 7: Record parity boundaries and run final gates

**Files:**

- Modify: `docs/architecture.md`
- Modify: `docs/mvp-6-app-shell-v1.md`
- Modify: `docs/research/wezterm-parity-gap.md`
- Test: repository verification commands

**Step 1: Update documentation**

Record as complete:

- terminal stable row identity and strict conversion;
- per-row sequence tracking;
- stable ordinary selection and stable pane viewport;
- offscreen text extraction and prune non-retargeting;
- active/inactive visible dirty-row invalidation;
- overlay exemption and accumulated dirty behavior;
- stable Lua pane dimensions/cursor coordinates.

Keep explicitly open:

- full WezTerm width reflow/resize selection persistence;
- pane-local Search/Copy/Quick Select controller ownership;
- inactive-pane hover-wheel routing;
- richer pane focus visuals;
- arbitrary Lua callbacks;
- external CLI title control;
- real mux/window registry, domain, protocol, and renderer parity.

Do not claim full selection, App Shell v2, or general WezTerm parity.

**Step 2: Verify documentation scope**

Run:

```powershell
rg -n "stable|seqno|dirty|pane-local|hover-wheel|reflow|Next Milestone" docs/architecture.md docs/mvp-6-app-shell-v1.md docs/research/wezterm-parity-gap.md
git diff --check
```

Expected: completed and open boundaries are consistent across all three
documents.

**Step 3: Commit documentation**

```powershell
git add docs/architecture.md docs/mvp-6-app-shell-v1.md docs/research/wezterm-parity-gap.md
git commit -m "docs: record stable selection parity"
```

**Step 4: Run focused terminal gates**

```powershell
cargo test -p rssh-terminal terminal_stable_ -- --nocapture
cargo test -p rssh-terminal terminal_sequence_ -- --nocapture
cargo test -p rssh-terminal terminal_full_screen_scroll_ -- --nocapture
cargo test -p rssh-terminal terminal_top_anchored_ -- --nocapture
cargo test -p rssh-terminal terminal_scrollback_prune_ -- --nocapture
cargo test -p rssh-terminal terminal_ed3_ -- --nocapture
cargo test -p rssh-terminal
```

Expected: all pass.

**Step 5: Run focused app gates**

```powershell
cargo test -p rssh-app selection -- --nocapture
cargo test -p rssh-app stable_viewport -- --nocapture
cargo test -p rssh-app window_app_visible_dirty_ -- --nocapture
cargo test -p rssh-app window_app_inactive_visible_dirty_ -- --nocapture
cargo test -p rssh-app window_app_inactive_dirty_unselected_ -- --nocapture
cargo test -p rssh-app overlay_defers_real_dirty -- --nocapture
cargo test -p rssh-app copy_mode -- --nocapture
cargo test -p rssh-app search -- --nocapture
cargo test -p rssh-app quick_select -- --nocapture
cargo test -p rssh-app scrollback -- --nocapture
cargo test -p rssh-app pane_focus -- --nocapture
cargo test -p rssh-app pane_select -- --nocapture
```

Expected: all pass and every filter runs at least one test.

**Step 6: Run full repository gates**

```powershell
cargo fmt --all -- --check
git diff --check dfd4c7d4..HEAD
cargo test -p rssh-app
cargo test --workspace
git status --short
```

Expected:

- formatting and diff checks exit zero;
- `rssh-app` and workspace tests have zero failures;
- only the existing explicitly ignored real-PTY/platform tests remain ignored;
- worktree is clean.

**Step 7: Final independent review**

Dispatch a fresh reviewer for the complete range
`ddb74c56..HEAD`. Require:

- design/plan compliance;
- pinned upstream contract verification;
- Critical/Important/Minor findings;
- explicit Ready yes/no;
- fresh full test and clean-status evidence.

Do not integrate with any Critical or Important issue open.
