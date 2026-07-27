# WezTerm App Shell Salvage Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task.

**Goal:** Salvage only App Shell behavior still missing from the parity baseline, without replaying the abandoned branch.

**Architecture:** Treat the parity commit as the new source of truth. Reuse current typed app-shell, pane-runtime, rendering, and window-manager paths; port behavior in small tested slices only where the coverage ledger below identifies a gap.

**Tech Stack:** Rust, Cargo workspace tests, the existing `rssh-core` and `rssh-app` test modules.

---

## Archive and starting point

- Annotated tag `archive/wezterm-app-shell-v1-wip-20260727` points to WIP commit `de763f1ec342aaa8d1a57c7e5aad639a20a36afe`. It preserves 99 old unique commits plus the WIP snapshot.
- Migration starts from parity commit `f686180136d73d9e5d97bfc99bc735a5afc3bec8`, where the full baseline passed.
- This is a local-only salvage. Do not merge or rebase the old branch; consult its archive only as implementation evidence.

## Covered: do not port

### SpawnCommand fields

Current `crates/rssh-app/src/window.rs` has `WindowSpawnCommandQuery` and typed domain/options paths. Evidence includes:

- `window_app_dispatches_palette_new_tab_spawn_command_cwd_and_env_query`
- `window_app_dispatches_palette_new_tab_spawn_command_set_environment_variables_query`
- the supplied `...domain_assignment_query` evidence, currently named `window_app_dispatches_palette_spawn_command_in_new_tab_domain_assignment_query`
- the supplied `...label_table_query` evidence, currently named `window_app_dispatches_palette_spawn_command_label_table_query`
- `window_app_dispatches_native_spawn_command_local_domain_payload`

Boundary: do not add the old `PaneLaunch.domain` or `PaneLaunch.label`; labels remain launcher/UI metadata.

### PaneSelect MoveToNewWindow

Current `crates/rssh-core/src/app_shell.rs` has `apply_move_pane_to_new_window`. The supplied `action_move_pane_to_new_window_closes_pane` evidence is represented by the current test `action_move_pane_to_new_window_detaches_selected_pane_into_pending_window`. App-level coverage includes:

- `window_pane_select_move_to_new_window_detaches_selected_pane_and_requests_window`
- `window_manager_collects_detached_app_after_move_to_new_window`

Boundary: do not restore the old process-launch transfer.

### Command palette frecency and metadata

Current coverage uses `NativeCommandPaletteEntry`, static augment parsing, and persisted frecency. Evidence includes:

- `window_app_augments_command_palette_with_native_entries`
- `window_app_parses_static_wezterm_augment_command_palette_return`
- `window_app_command_palette_renders_augmented_entry_doc`
- `window_app_command_palette_renders_augmented_entry_icon`
- `window_app_command_palette_uses_recently_executed_command_as_fuzzy_tiebreaker`
- `window_app_command_palette_persists_frecency_between_app_instances`

Boundary: do not restore the old CLI-only model.

## Ordered salvage backlog

1. **Pane close button — partial.** Reuse `request_close_confirmation_or_close` and `WindowCloseTarget::Pane`.
2. **Tab overflow — partial.** Create one shared visible-segment ledger.
3. **Tab drag — partial.** Build drag targeting on that ledger and the existing `MoveTab` path.
4. **Split ratio — missing.** Recalibrate the current fixed `source_size_delta` when the window resizes.
5. **Pane restart — missing.** Reuse `PaneRuntime`, synchronization, and spawn paths. Do not bind `Ctrl+Shift+R`; config reload owns it.
6. **Pane inspect — missing.** Reuse `pane_render_layout` and the `PaneRuntime`/PTY pid.
7. **Window state report — missing.** Add CLI `state` and `state-json`, mutually exclusive with metrics, and snapshot the current configured startup app.

## Validation

For each backlog slice, add and run focused tests for that behavior before moving to the next slice. After all slices, run:

```powershell
cargo test --locked --workspace --all-targets
cargo fmt --check
git diff --check
```

Keep each change local to the parity-based salvage branch and preserve all covered boundaries above.
