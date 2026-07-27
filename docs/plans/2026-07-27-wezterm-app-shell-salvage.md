# WezTerm App Shell Salvage Coverage Ledger

This ledger is the final behavior-by-behavior accounting for the parity-based
App Shell implementation through `0df5f313`. Every behavior found in the
archived WIP is either already covered by the parity baseline or has a dedicated
parity-native commit range and committed tests below. The current parity
architecture remains the source of truth; the archived implementation was used
only as behavioral evidence.

## Archive and integration boundary

- The annotated tag `archive/wezterm-app-shell-v1-wip-20260727` peels to WIP
  commit `de763f1ec342aaa8d1a57c7e5aad639a20a36afe`.
- That commit contains the original 99 commits unique to the old App Shell
  history plus the WIP snapshot commit itself (100 commits ahead of the parity
  base in total). The snapshot preserves all nine tracked WIP files.
- Salvage started from parity commit
  `f686180136d73d9e5d97bfc99bc735a5afc3bec8`.
- The old `codex/wezterm-app-shell-v1` branch and its worktree are still
  retained. Keep them, the salvage branch, and the archive tag until the
  pre-merge and post-merge validation succeeds. Only then may the old and
  temporary branches/worktrees be removed; retain the archive tag as the
  historical recovery point.
- This is local-only integration. Do not merge or rebase the old App Shell
  branch, push these branches, merge parity into `main`, or create a PR.

## Covered by the parity baseline

### SpawnCommand fields

No WIP port was needed. Current `crates/rssh-app/src/window.rs` uses
`WindowSpawnCommandQuery` and the typed domain/options paths. Committed evidence:

- `window_app_dispatches_palette_new_tab_spawn_command_cwd_and_env_query`
- `window_app_dispatches_palette_new_tab_spawn_command_set_environment_variables_query`
- `window_app_dispatches_palette_spawn_command_in_new_tab_domain_assignment_query`
- `window_app_dispatches_palette_spawn_command_label_table_query`
- `window_app_dispatches_native_spawn_command_local_domain_payload`
- `window_app_show_launcher_key_assignments_use_spawn_command_label`

The first five tests cover query parsing and typed dispatch. The launcher test
proves that `label` is UI metadata. Do not restore the old
`PaneLaunch.domain` or `PaneLaunch.label` fields.

### PaneSelect MoveToNewWindow

No WIP port was needed. Current `crates/rssh-core/src/app_shell.rs` uses
`apply_move_pane_to_new_window`, and the window manager consumes the pending
detached window. Committed evidence:

- `action_move_pane_to_new_window_detaches_selected_pane_into_pending_window`
- `window_pane_select_move_to_new_window_detaches_selected_pane_and_requests_window`
- `window_manager_collects_detached_app_after_move_to_new_window`

These replace the archived close-only behavior. Do not restore the old
process-launch transfer.

### Command palette frecency and metadata

No WIP port was needed. Current coverage uses `NativeCommandPaletteEntry`,
static augment parsing, and persisted frecency. Committed evidence:

- `window_app_augments_command_palette_with_native_entries`
- `window_app_parses_static_wezterm_augment_command_palette_return`
- `window_app_command_palette_renders_augmented_entry_doc`
- `window_app_command_palette_renders_augmented_entry_icon`
- `window_app_command_palette_uses_recently_executed_command_as_fuzzy_tiebreaker`
- `window_app_command_palette_persists_frecency_between_app_instances`

Do not restore the old CLI-only palette model.

## Salvaged onto parity

### Pane close button

Commit range: `548f8861^..952e2f75`.

The parity implementation renders a close button for each pane only when a tab
has multiple panes, targets inactive panes exactly, and reuses
`request_close_confirmation_or_close` with `WindowCloseTarget::Pane`. Pane-local
badges and higher-level overlays retain render and hit priority. A consumed
press also consumes its paired release, including focus-loss and pane-selection
paths; snapshot reuse avoids a second deep clone.

Representative committed tests:

- `window_app_renders_and_hits_close_buttons_for_each_visible_pane`
- `window_app_omits_pane_close_button_for_single_pane`
- `window_app_pane_close_button_targets_non_active_pane_confirmation_without_forwarding`
- `window_app_pane_badge_takes_render_and_hit_priority_over_close_button`
- `window_app_command_palette_blocks_pane_close_button_full_click`
- `window_app_pane_close_button_consumes_release_after_window_loses_focus`

### Tab overflow

Commit range: `aa30f3b9^..286feaeb`.

The Fancy renderer now produces and caches the visible tab-bar layout used by
the same rendered frame's hit-testing. Clipped tabs produce a non-interactive
`…`; hidden or truncated cells cannot target tabs. At extreme widths overflow
yields correctly against the new-tab control and never overwrites reserved
right status. Hit-testing does not rerun a stateful formatter, and stale
independent layout helpers were removed.

Representative committed tests:

- `tab_bar_renders_overflow_indicator_for_clipped_tabs`
- `tab_bar_overflow_indicator_does_not_target_tabs`
- `tab_bar_render_and_hit_testing_share_formatted_segment_layout`
- `tab_bar_hit_testing_reuses_and_refreshes_the_rendered_layout`
- `tab_bar_extreme_narrow_width_prioritizes_overflow_over_new_tab`
- `tab_bar_overflow_never_overwrites_reserved_right_status`

### Tab drag reorder

Commit range: `3e166a18^..e85a23eb`.

Drag starts only from a real rendered tab ledger and records the stable source
tab identity. Release resolves the current rendered target identity, then
re-resolves both workspace indexes before using the existing move-tab path.
Blank, new-tab, close, right-status, overflow, deleted, and stale targets cancel
safely. The full mouse sequence is consumed, focus loss is latched safely, and
runtime ownership stays attached to tab identity.

Representative committed tests:

- `dragging_tab_bar_reorders_tabs`
- `tab_drag_does_not_start_before_the_first_rendered_ledger`
- `tab_drag_revalidates_source_and_target_ids_after_tab_order_changes`
- `dragging_tab_bar_reorder_ignores_hidden_overflow_target`
- `dragging_active_tab_preserves_tab_identity_and_runtime_owners`
- `tab_drag_release_cancels_on_blank_new_close_and_right_status_cells`
- `tab_drag_revalidates_ids_when_a_real_render_replaces_the_pressed_ledger`

### Split ratio persistence

Commit range: `57e49543^..e9b3cdd7`.

Before resizing terminal runtimes, `AppShell` replays the current implicit split
tree using each split's local usable span (excluding its one-cell separator).
It applies nearest-cell rounding and a one-cell-per-child clamp, recurses
through mixed axes, and uses a mouse-dragged ratio as the next resize baseline.
Same-size resize is an exact no-op. A cross-axis-only resize preserves the raw
`source_size_delta`, including an out-of-render-range value, rather than
normalizing it.

Representative committed tests:

- `preserve_split_layout_scales_down_split_with_height_only`
- `preserve_split_layout_keeps_vertical_delta_on_width_only_resize`
- `preserve_split_layout_keeps_horizontal_delta_on_height_only_resize`
- `preserve_split_layout_recurses_through_mixed_local_splits`
- `preserve_split_layout_uses_dragged_ratio_as_resize_baseline`
- `preserve_split_layout_clamps_tiny_sizes_and_uses_clamp_as_next_baseline`
- `window_app_preserves_split_ratio_with_percentage_padding`
- `window_app_preserves_down_split_ratio_when_rows_change`

### Pane restart

Commit range: `e14b1704^..65d078af`.

The typed `RestartPane` action is available to palette/config dispatch without
stealing `Ctrl+Shift+R` from configuration reload. Restart preserves pane
identity and its full launch definition, uses explicit runtime CWD evidence
when available, retires and joins the old PTY lifecycle, resets only the target
pane's terminal/UI projections, and installs a fresh runtime. Process-unique
runtime generations prevent late events from a retired or transferred owner
from reaching a new runtime. Active and inactive pane targeting, spawn failure,
title refresh, and target terminal dimensions are covered.

Representative committed tests:

- `restart_pane_does_not_replace_reload_configuration_shortcut`
- `window_app_restart_pane_retires_active_runtime_and_owner_state`
- `restart_inactive_pane_replaces_only_target_runtime`
- `restart_without_new_cwd_evidence_preserves_full_pane_launch_on_success_and_failure`
- `window_manager_ignores_events_from_retired_pane_runtime_generation`
- `wheel_restart_targets_inactive_pane_and_keeps_active_runtime_unchanged_on_spawn_failure`
- `pane_spawn_pty_and_terminal_runtime_use_same_target_dimensions`
- `active_restart_applies_title_after_target_projection_reset`

### Pane inspection

Commit range: `5ab851eb^..8c05fec3`.

The typed `InspectPane` action opens a pane-local overlay backed by the current
pane render rectangle and live runtime/PTY metadata. It targets inactive panes
without focusing them, clips by grapheme display width, and never exposes
environment values. The modal input barrier closes on Escape or Enter with
paired-release protection across focus changes. Ownership changes, target
deletion/read failure, and competing pane/global overlays cancel safely;
restart refreshes live metadata while retaining the stable pane target.

Representative committed tests:

- `pane_inspection_metadata_is_live_for_an_inactive_pane_and_hides_environment_values`
- `pane_inspection_overlay_targets_inactive_rect_and_clips_tiny_panes`
- `pane_inspection_overlay_uses_grapheme_width_and_never_partially_draws_a_wide_cell`
- `inspect_swallows_terminal_input_and_the_paired_close_key_release`
- `fresh_matching_keypress_after_refocus_supersedes_the_stale_release_pending_state`
- `inspect_keeps_a_visible_stable_target_and_cancels_when_target_leaves_the_active_tab`
- `nested_multiple_ownership_move_cancels_the_inspected_pane`
- `inspect_reads_restarted_runtime_metadata_without_replacing_the_stable_target`

### Configured window state report

Commit range: `a2e57cef^..0df5f313`.

The window CLI accepts mutually exclusive `--state` and `--state-json` formats
alongside the existing mutually exclusive metrics formats. Reporting runs
before window/PTY creation and snapshots the same configured startup path used
by a real window: configuration discovery, strict default-workspace overrides,
startup command, workspace/tab/pane tree, launch metadata, split directions,
and configured dimensions. Text and JSON share one deterministic snapshot,
environment values are redacted, and output/spawn/panic/error paths are
explicitly propagated.

Representative committed tests:

- `parses_window_state_flags`
- `window_report_formats_are_strictly_mutually_exclusive`
- `state_snapshot_covers_all_shell_entries_in_stable_order`
- `state_json_round_trips_and_never_exposes_environment_values`
- `state_text_is_readable_deterministic_and_uses_the_same_snapshot`
- `configured_state_uses_cli_default_workspace_prog_cwd_and_redacts_values`
- `configured_state_uses_file_default_workspace_prog_and_dimensions`
- `configured_initial_grid_drives_json_text_and_nested_split_dimensions`
- `state_stdout_writes_once_flushes_and_propagates_broken_pipe`
- `state_report_thread_seam_maps_spawn_panic_and_inner_errors`

## Final validation gate

The ledger accounts for every archived behavior, but cleanup is intentionally
not claimed here. Before merging salvage into parity, and again after the local
merge, run:

```powershell
cargo test --locked --workspace --all-targets
cargo fmt --check
git diff --check
```

After both validation passes, verify that the archive tag still peels to
`de763f1ec342aaa8d1a57c7e5aad639a20a36afe`, remove only the old and temporary
worktrees/branches, prune worktrees, and confirm that local branches are limited
to `main` and `codex/wezterm-parity-progress`. Keep the annotated archive tag.
