# WezTerm Parity App Shell v1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the first R-SSH application-shell layer needed to move toward WezTerm parity: workspaces, tabs, panes, and typed actions while preserving the current single-window local PTY startup path.

**Architecture:** Put pure app-shell state in `rssh-core` so it can be tested without a window or PTY. Integrate that state into `rssh-app::window` as one default workspace/tab/pane first, then route a small set of actions through the window without changing existing console behavior.

**Tech Stack:** Rust, existing `rssh-core`, existing `rssh-app` native `winit` window, existing `rssh-pty::PtyCommand`, existing unit tests and workspace cargo gates.

---

### Task 1: Add App Shell Identifiers

**Files:**
- Modify: `crates/rssh-core/src/lib.rs`

**Step 1: Write the failing tests**

Add tests beside the existing `SessionId` tests:

```rust
#[test]
fn app_shell_ids_expose_values() {
    assert_eq!(WindowId::new(1).get(), 1);
    assert_eq!(WorkspaceId::new(2).get(), 2);
    assert_eq!(TabId::new(3).get(), 3);
    assert_eq!(PaneId::new(4).get(), 4);
}

#[test]
fn app_shell_ids_are_hashable_and_comparable() {
    let mut ids = std::collections::HashSet::new();
    ids.insert(PaneId::new(7));

    assert!(ids.contains(&PaneId::new(7)));
    assert!(!ids.contains(&PaneId::new(8)));
}
```

**Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p rssh-core app_shell_ids
```

Expected: compile failure because the new ID types do not exist.

**Step 3: Implement minimal ID types**

Add `WindowId`, `WorkspaceId`, `TabId`, and `PaneId` next to `SessionId`. Use
the same `new` and `get` shape as `SessionId`; derive `Debug`, `Clone`, `Copy`,
`PartialEq`, `Eq`, and `Hash`.

**Step 4: Run test to verify it passes**

Run:

```powershell
cargo test -p rssh-core app_shell_ids
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-core/src/lib.rs
git commit -m "feat: add app shell identifiers"
```

### Task 2: Add Pure App Shell Model

**Files:**
- Create: `crates/rssh-core/src/app_shell.rs`
- Modify: `crates/rssh-core/src/lib.rs`

**Step 1: Write the failing tests**

Create `app_shell.rs` with tests first:

```rust
#[test]
fn app_shell_starts_with_default_workspace_tab_and_pane() {
    let shell = AppShell::new(PaneLaunch::local("pwsh"));

    assert_eq!(shell.active_workspace_id(), WorkspaceId::new(1));
    assert_eq!(shell.active_tab_id(), TabId::new(1));
    assert_eq!(shell.active_pane_id(), PaneId::new(1));
    assert_eq!(shell.workspaces().len(), 1);
    assert_eq!(shell.active_workspace().tabs().len(), 1);
}

#[test]
fn active_pane_exposes_local_launch_command() {
    let shell = AppShell::new(PaneLaunch::local("pwsh").with_args(["-NoLogo"]));

    assert_eq!(shell.active_pane().launch().program(), "pwsh");
    assert_eq!(shell.active_pane().launch().args(), ["-NoLogo"]);
}
```

**Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p rssh-core app_shell
```

Expected: compile failure until `AppShell`, `PaneLaunch`, and exports exist.

**Step 3: Implement minimal model**

Implement:

```rust
pub struct AppShell { ... }
pub struct Workspace { ... }
pub struct Tab { ... }
pub struct Pane { ... }
pub struct PaneLaunch { program: String, args: Vec<String> }
```

Use deterministic counters starting at `1`. Keep getters immutable. Export the
module from `lib.rs` with `pub mod app_shell;`.

**Step 4: Run test to verify it passes**

Run:

```powershell
cargo test -p rssh-core app_shell
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-core/src/lib.rs crates/rssh-core/src/app_shell.rs
git commit -m "feat: add app shell state model"
```

### Task 3: Add App Actions

**Files:**
- Modify: `crates/rssh-core/src/app_shell.rs`

**Step 1: Write the failing tests**

Add tests:

```rust
#[test]
fn action_new_tab_creates_and_selects_tab() {
    let mut shell = AppShell::new(PaneLaunch::local("pwsh"));

    shell.apply_action(AppAction::NewTab { launch: None }).unwrap();

    assert_eq!(shell.active_tab_id(), TabId::new(2));
    assert_eq!(shell.active_workspace().tabs().len(), 2);
    assert_eq!(shell.active_pane().launch().program(), "pwsh");
}

#[test]
fn action_close_tab_selects_neighbor() {
    let mut shell = AppShell::new(PaneLaunch::local("pwsh"));
    shell.apply_action(AppAction::NewTab { launch: None }).unwrap();

    shell.apply_action(AppAction::CloseTab { tab: TabId::new(2) }).unwrap();

    assert_eq!(shell.active_tab_id(), TabId::new(1));
    assert_eq!(shell.active_workspace().tabs().len(), 1);
}

#[test]
fn action_close_last_tab_is_rejected() {
    let mut shell = AppShell::new(PaneLaunch::local("pwsh"));

    let error = shell
        .apply_action(AppAction::CloseTab { tab: TabId::new(1) })
        .unwrap_err();

    assert_eq!(error, AppShellError::CannotCloseLastTab);
    assert_eq!(shell.active_tab_id(), TabId::new(1));
}
```

**Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p rssh-core action_
```

Expected: compile failure for missing `AppAction` and errors.

**Step 3: Implement action dispatch**

Add:

```rust
pub enum AppAction {
    NewTab { launch: Option<PaneLaunch> },
    CloseTab { tab: TabId },
    ActivateTab { tab: TabId },
    SplitPane { pane: PaneId, direction: SplitDirection, launch: Option<PaneLaunch> },
    ClosePane { pane: PaneId },
    FocusNextPane,
    FocusPreviousPane,
    SwitchWorkspace { workspace: WorkspaceId },
    RenameWorkspace { workspace: WorkspaceId, name: String },
}
```

Add `AppShellError` with typed cases for invalid workspace, tab, pane, and last
tab/pane guards. Implement only tab actions fully in this task; return a typed
`UnsupportedAction` for split/workspace actions until later tasks.

**Step 4: Run test to verify it passes**

Run:

```powershell
cargo test -p rssh-core action_
```

Expected: PASS for tab actions.

**Step 5: Commit**

```powershell
git add crates/rssh-core/src/app_shell.rs
git commit -m "feat: add app shell tab actions"
```

### Task 4: Add Pane Split State

**Files:**
- Modify: `crates/rssh-core/src/app_shell.rs`

**Step 1: Write the failing tests**

Add tests:

```rust
#[test]
fn action_split_pane_creates_and_focuses_new_pane() {
    let mut shell = AppShell::new(PaneLaunch::local("pwsh"));

    shell
        .apply_action(AppAction::SplitPane {
            pane: PaneId::new(1),
            direction: SplitDirection::Right,
            launch: None,
        })
        .unwrap();

    assert_eq!(shell.active_pane_id(), PaneId::new(2));
    assert_eq!(shell.active_tab().panes().len(), 2);
}

#[test]
fn focus_next_pane_cycles_within_active_tab() {
    let mut shell = AppShell::new(PaneLaunch::local("pwsh"));
    shell.apply_action(AppAction::SplitPane {
        pane: PaneId::new(1),
        direction: SplitDirection::Right,
        launch: None,
    }).unwrap();

    shell.apply_action(AppAction::FocusNextPane).unwrap();

    assert_eq!(shell.active_pane_id(), PaneId::new(1));
}
```

**Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p rssh-core pane
```

Expected: split/focus tests fail or return unsupported.

**Step 3: Implement minimal split state**

Keep pane layout simple for v1:

```rust
pub enum SplitDirection { Right, Down }
```

Store panes in tab order plus optional split metadata. Do not implement full
pixel layout yet. `SplitPane` creates a new pane, focuses it, and records the
requested direction. `FocusNextPane` and `FocusPreviousPane` cycle within the
active tab.

**Step 4: Run test to verify it passes**

Run:

```powershell
cargo test -p rssh-core pane
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-core/src/app_shell.rs
git commit -m "feat: add app shell pane actions"
```

### Task 5: Add Workspace Actions

**Files:**
- Modify: `crates/rssh-core/src/app_shell.rs`

**Step 1: Write the failing tests**

Add tests:

```rust
#[test]
fn action_new_workspace_creates_and_selects_workspace() {
    let mut shell = AppShell::new(PaneLaunch::local("pwsh"));

    shell
        .apply_action(AppAction::NewWorkspace {
            name: "ops".to_owned(),
            launch: None,
        })
        .unwrap();

    assert_eq!(shell.active_workspace_id(), WorkspaceId::new(2));
    assert_eq!(shell.active_workspace().name(), "ops");
    assert_eq!(shell.workspaces().len(), 2);
}

#[test]
fn action_switch_workspace_selects_existing_workspace() {
    let mut shell = AppShell::new(PaneLaunch::local("pwsh"));
    shell.apply_action(AppAction::NewWorkspace {
        name: "ops".to_owned(),
        launch: None,
    }).unwrap();

    shell
        .apply_action(AppAction::SwitchWorkspace {
            workspace: WorkspaceId::new(1),
        })
        .unwrap();

    assert_eq!(shell.active_workspace_id(), WorkspaceId::new(1));
}
```

**Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p rssh-core workspace
```

Expected: compile failure or unsupported action.

**Step 3: Implement workspace actions**

Add `NewWorkspace` to `AppAction`. Implement new, switch, and rename workspace
behavior. New workspaces should contain one tab and one pane using the supplied
launch or the default launch copied from the shell.

**Step 4: Run test to verify it passes**

Run:

```powershell
cargo test -p rssh-core workspace
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-core/src/app_shell.rs
git commit -m "feat: add app shell workspace actions"
```

### Task 6: Integrate Default App Shell Into Native Window

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Modify: `crates/rssh-app/Cargo.toml` only if new dependency is unavoidable

**Step 1: Write the failing tests**

Add tests in `window.rs`:

```rust
#[test]
fn window_app_starts_with_default_shell_state() {
    let app = NativeWindowApp::new_with_command(None, PtyCommand::new("pwsh"));

    assert_eq!(app.active_workspace_id(), rssh_core::WorkspaceId::new(1));
    assert_eq!(app.active_tab_id(), rssh_core::TabId::new(1));
    assert_eq!(app.active_pane_id(), rssh_core::PaneId::new(1));
    assert_eq!(app.startup_command().program(), "pwsh");
}
```

**Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p rssh-app window_app_starts_with_default_shell_state
```

Expected: compile failure until the window exposes app-shell state.

**Step 3: Implement integration**

Add an `AppShell` field to `NativeWindowApp`. Convert the existing
`PtyCommand` startup command into `PaneLaunch` when constructing the shell.
Keep the existing `startup_command` field for now to avoid broad PTY runtime
changes in the first integration step.

Expose test-only getters for active workspace, tab, and pane IDs.

**Step 4: Run test to verify it passes**

Run:

```powershell
cargo test -p rssh-app window_app_starts_with_default_shell_state
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: initialize app shell in native window"
```

### Task 7: Add Window Action Dispatch Tests

**Files:**
- Modify: `crates/rssh-app/src/window.rs`

**Step 1: Write the failing tests**

Add tests:

```rust
#[test]
fn window_app_dispatches_new_tab_action() {
    let mut app = NativeWindowApp::new_with_command(None, PtyCommand::new("pwsh"));

    app.dispatch_app_action(rssh_core::app_shell::AppAction::NewTab { launch: None })
        .unwrap();

    assert_eq!(app.active_tab_id(), rssh_core::TabId::new(2));
}

#[test]
fn window_title_reports_app_shell_state() {
    let mut app = NativeWindowApp::new_with_command(None, PtyCommand::new("pwsh"));
    app.dispatch_app_action(rssh_core::app_shell::AppAction::NewTab { launch: None })
        .unwrap();

    assert!(app.effective_window_title().contains("tab 2/2"));
}
```

**Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p rssh-app window_app_dispatches_new_tab_action
cargo test -p rssh-app window_title_reports_app_shell_state
```

Expected: compile failure until action dispatch exists.

**Step 3: Implement dispatch boundary**

Add `dispatch_app_action` on `NativeWindowApp` that calls
`self.app_shell.apply_action(...)` and refreshes the title/snapshot state as
needed. For v1, action dispatch updates state only; spawning multiple live PTYs
can be a later task after the model is stable.

Update `effective_window_title` to append compact state such as
`workspace default - tab 1/1 - pane 1/1` while preserving PTY-provided titles.

**Step 4: Run test to verify it passes**

Run:

```powershell
cargo test -p rssh-app window_app_dispatches_new_tab_action
cargo test -p rssh-app window_title_reports_app_shell_state
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: dispatch app shell actions in window"
```

### Task 8: Add Minimal Key Bindings For Shell Actions

**Files:**
- Modify: `crates/rssh-app/src/window.rs`

**Step 1: Write the failing tests**

Add tests for recognizers, not the OS event loop:

```rust
#[test]
fn recognizes_new_tab_shortcut() {
    assert_eq!(
        app_shell_shortcut(
            &Key::Character("t".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        ),
        Some(AppActionShortcut::NewTab)
    );
}

#[test]
fn unmodified_t_is_not_shell_shortcut() {
    assert_eq!(
        app_shell_shortcut(&Key::Character("t".into()), ModifiersState::empty()),
        None
    );
}
```

**Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p rssh-app shortcut
```

Expected: compile failure until shortcut recognizers exist.

**Step 3: Implement recognizers and keyboard hook**

Add shortcuts:

- `Ctrl+Shift+T`: new tab
- `Ctrl+Shift+W`: close tab
- `Ctrl+Shift+]`: next tab
- `Ctrl+Shift+[`: previous tab
- `Ctrl+Shift+D`: split pane right
- `Ctrl+Shift+E`: split pane down

Hook these before sending input to the active PTY. If an action fails because
it would close the last tab/pane, keep the current session active.

**Step 4: Run test to verify it passes**

Run:

```powershell
cargo test -p rssh-app shortcut
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs
git commit -m "feat: add native window shell shortcuts"
```

### Task 9: Document App Shell v1 Status

**Files:**
- Modify: `README.md`
- Modify: `docs/architecture.md`
- Create: `docs/mvp-6-app-shell-v1.md`

**Step 1: Write the documentation**

Document:

- WezTerm parity direction.
- App Shell v1 completed scope.
- Startup compatibility: `rssh-app` and `rssh-app window` still open one local
  PTY pane by default.
- Current limitations: action state model exists, full multi-PTY pane rendering
  and mux/domain work are later stages.
- Shortcuts added in Task 8.

**Step 2: Run docs-adjacent checks**

Run:

```powershell
rg "App Shell v1|WezTerm|tabs|panes|workspaces" README.md docs
```

Expected: output includes the new status file and README/architecture updates.

**Step 3: Commit**

```powershell
git add README.md docs/architecture.md docs/mvp-6-app-shell-v1.md
git commit -m "docs: describe app shell v1"
```

### Task 10: Run Final Verification

**Files:**
- No source edits unless verification finds a bug.

**Step 1: Run formatting**

```powershell
cargo fmt --all -- --check
```

Expected: PASS.

**Step 2: Run tests**

```powershell
cargo test --workspace
```

Expected: PASS.

**Step 3: Run clippy**

```powershell
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

**Step 4: Run release build**

```powershell
cargo build --release -p rssh-app
```

Expected: PASS.

**Step 5: Run native-window smoke**

```powershell
.\target\release\rssh-app.exe window --frames 30 --metrics-json -- cmd.exe /C echo app-shell-smoke
```

Expected: process exits successfully and prints metrics JSON.

**Step 6: Commit any verification fixes**

Only if required:

```powershell
git add <fixed-files>
git commit -m "fix: stabilize app shell v1"
```

### Task 11: Update Parity Tracking

**Files:**
- Modify: `docs/plans/2026-06-07-wezterm-parity-app-shell-v1-design.md`
- Create or modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Record verified status**

Add a short matrix showing App Shell v1 completed and the next still-open
WezTerm parity gaps: visual tab bar/splits, mux/domain, GPU/font, image
protocols, command palette/quick select/copy mode, config/plugin layer.

**Step 2: Commit**

```powershell
git add docs/plans/2026-06-07-wezterm-parity-app-shell-v1-design.md docs/research/wezterm-parity-gap.md
git commit -m "docs: update wezterm parity tracker"
```
