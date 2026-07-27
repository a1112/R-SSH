use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::*;

#[test]
fn window_app_recognizes_restart_pane_typed_action_query() {
    let mut app = NativeWindowApp::new(None);
    app.enter_command_palette_mode();
    app.command_palette_set_query("wezterm.action.RestartPane".to_owned());

    let labels = app
        .command_palette_filtered_commands()
        .iter()
        .map(WindowCommand::label)
        .collect::<Vec<_>>();

    assert_eq!(labels, ["Restart Pane"]);
}

#[test]
fn restart_pane_command_has_palette_metadata_and_records_frecency() {
    let mut app = NativeWindowApp::new(None);

    assert!(WINDOW_COMMANDS.contains(&WindowCommand::RestartPane));
    assert_eq!(WindowCommand::RestartPane.label(), "Restart Pane");
    assert_eq!(app.command_palette_frecency("Restart Pane").uses, 0);

    app.enter_command_palette_mode();
    assert!(app.command_palette_execute(WindowCommand::RestartPane));

    assert_eq!(app.command_palette_frecency("Restart Pane").uses, 1);
}

#[test]
fn restart_pane_does_not_replace_reload_configuration_shortcut() {
    let assignment = NATIVE_WINDOW_KEY_ASSIGNMENTS
        .iter()
        .find(|assignment| assignment.keys == "CTRL+SHIFT+R")
        .expect("default reload shortcut");

    assert_eq!(assignment.command, WindowCommand::ReloadConfiguration);
    assert_ne!(assignment.command, WindowCommand::RestartPane);
}

#[test]
fn window_app_restart_pane_retires_active_runtime_and_owner_state() {
    let mut app = NativeWindowApp::new(None);
    app.handle_pty_output(b"inactive").unwrap();
    let launch = PaneLaunch::local("restart-shell")
        .with_args(["--login"])
        .with_cwd("file://host/work")
        .with_environment([("RESTART_TEST", "preserved")]);
    app.dispatch_app_action(AppAction::SplitPane {
        pane: app.active_pane_id(),
        direction: SplitDirection::Right,
        launch: Some(launch.clone()),
    })
    .unwrap();
    app.handle_pty_output(b"active").unwrap();
    let active = app.active_pane_id();
    let inactive = rssh_core::PaneId::new(1);
    app.dispatch_app_action(AppAction::TogglePaneZoom { pane: active })
        .unwrap();

    let writer_dropped = Arc::new(AtomicUsize::new(0));
    app.writer = Some(Box::new(DropTrackingWriter(Arc::clone(&writer_dropped))));
    app.session_process_id = Some(41_101);
    app.session_tty_name = Some("old-tty".to_owned());
    app.reader_thread = Some(std::thread::spawn(|| {}));
    let initial_copy_mode = app.initial_copy_mode();
    app.active_ui.enter_copy_mode(initial_copy_mode);
    app.ime_preedit = Some("old-preedit".to_owned());
    app.dead_key_active = true;
    app.dead_key_text = Some("old-dead-key".to_owned());
    app.selecting = true;
    app.active_mouse_button = Some(MouseButton::Left);
    app.scrollbar_dragging = true;
    app.ui_left_release_pending = true;
    app.pressed_pane_close_button = Some(active);
    let inactive_snapshot = app.pane_snapshot(inactive).unwrap().clone();
    let inactive_process_id = app
        .pane_runtimes
        .get(&inactive)
        .and_then(|runtime| runtime.session_process_id);
    let pane_ids = app.app_shell.pane_ids();
    let launch_before = app.app_shell.active_pane().launch().clone();

    assert!(app.command_palette_execute(WindowCommand::RestartPane));

    assert_eq!(app.active_pane_id(), active);
    assert_eq!(app.app_shell.active_pane().launch(), &launch_before);
    assert_eq!(app.app_shell.pane_ids(), pane_ids);
    assert_eq!(app.app_shell.active_tab().zoomed_pane_id(), Some(active));
    assert_eq!(writer_dropped.load(Ordering::Relaxed), 1);
    assert!(app.session.is_none());
    assert!(app.session_process_id.is_none());
    assert!(app.session_tty_name.is_none());
    assert!(app.writer.is_none());
    assert!(app.reader_thread.is_none());
    assert_eq!(snapshot_char(&app.snapshot, 0, 0), None);
    assert!(app.active_ui.ordinary_selection.is_none());
    assert!(!pane_copy_overlay_rendering(&app.active_ui));
    assert!(app.selection.is_none());
    assert!(app.ime_preedit.is_none());
    assert!(!app.dead_key_active);
    assert!(app.dead_key_text.is_none());
    assert!(!app.selecting);
    assert!(app.active_mouse_button.is_none());
    assert!(!app.scrollbar_dragging);
    assert!(!app.ui_left_release_pending);
    assert!(app.pressed_pane_close_button.is_none());
    assert_eq!(app.pane_snapshot(inactive).unwrap(), &inactive_snapshot);
    assert_eq!(
        app.pane_runtimes
            .get(&inactive)
            .and_then(|runtime| runtime.session_process_id),
        inactive_process_id
    );
}

#[test]
fn window_app_restart_pane_installs_fresh_runtime_without_touching_other_owner() {
    let mut app = NativeWindowApp::new(None);
    app.handle_pty_output(b"inactive-old").unwrap();
    app.dispatch_app_action(AppAction::SplitPane {
        pane: app.active_pane_id(),
        direction: SplitDirection::Right,
        launch: Some(PaneLaunch::local("restart-success")),
    })
    .unwrap();
    app.handle_pty_output(b"active-old").unwrap();
    let active = app.active_pane_id();
    let inactive = rssh_core::PaneId::new(1);
    let inactive_snapshot = app.pane_snapshot(inactive).unwrap().clone();
    let old_writer_dropped = Arc::new(AtomicUsize::new(0));
    app.writer = Some(Box::new(DropTrackingWriter(Arc::clone(
        &old_writer_dropped,
    ))));
    app.session_process_id = Some(41_201);

    app.restart_pane_runtime_with(active, |app| {
        let mut runtime = app.new_inactive_pane_runtime();
        runtime.runtime.feed_pty_output_with_display(b"fresh");
        runtime
            .ui
            .reconcile_terminal_mutation(runtime.runtime.terminal());
        runtime.snapshot = terminal_runtime_snapshot(&runtime.runtime, runtime.ui.stable_viewport);
        runtime.session_process_id = Some(41_202);
        Ok::<_, Box<dyn std::error::Error>>(runtime)
    })
    .unwrap();

    assert_eq!(old_writer_dropped.load(Ordering::Relaxed), 1);
    assert_eq!(app.session_process_id, Some(41_202));
    assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('f'));
    assert_eq!(app.pane_snapshot(inactive).unwrap(), &inactive_snapshot);
    assert_eq!(app.active_pane_id(), active);
}

#[test]
fn window_manager_ignores_events_from_retired_pane_runtime_generation() {
    let mut app = NativeWindowApp::new(None);
    let pane_id = app.active_pane_id();
    let window_id = app.app_window_id_for_test();
    let retired_generation = app.active_runtime_generation;

    app.restart_pane_runtime_with(pane_id, |app| {
        let mut runtime = app.new_inactive_pane_runtime();
        runtime.runtime.feed_pty_output_with_display(b"fresh");
        runtime.snapshot = terminal_runtime_snapshot(&runtime.runtime, runtime.ui.stable_viewport);
        runtime.session_process_id = Some(41_302);
        Ok::<_, Box<dyn std::error::Error>>(runtime)
    })
    .unwrap();
    let active_generation = app.active_runtime_generation;
    assert_ne!(active_generation, retired_generation);
    let mut manager = NativeWindowManager::new(app);

    assert_eq!(
        manager.dispatch_user_event_to_owner(WindowUserEvent::Output {
            window_id,
            pane_id,
            runtime_generation: retired_generation,
            bytes: b"stale".to_vec(),
        }),
        Some(false)
    );
    assert_eq!(
        manager.dispatch_user_event_to_owner(WindowUserEvent::Exited {
            window_id,
            pane_id,
            runtime_generation: retired_generation,
        }),
        Some(false)
    );

    let app = manager.startup_app.as_ref().unwrap();
    assert_eq!(app.active_runtime_generation, active_generation);
    assert_eq!(app.session_process_id, Some(41_302));
    assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('f'));
}

#[test]
fn window_manager_ignores_retired_events_after_pane_owner_transfer() {
    let mut primary = NativeWindowApp::new(None);
    primary.runtime.resize(rssh_core::TerminalSize::new(16, 1));
    primary.handle_pty_output(b"relocated").unwrap();
    primary.active_runtime_generation = 41;
    primary.next_runtime_generation = 42;
    primary
        .dispatch_app_action(AppAction::SplitPane {
            pane: rssh_core::PaneId::new(1),
            direction: SplitDirection::Right,
            launch: None,
        })
        .unwrap();
    primary
        .dispatch_app_action(AppAction::MovePaneToNewWindow {
            pane: rssh_core::PaneId::new(1),
        })
        .unwrap();
    let mut manager = NativeWindowManager::new_for_test(primary);
    manager.collect_pending_window_apps_from_primary_for_test();

    assert_eq!(
        manager.dispatch_user_event_to_owner(WindowUserEvent::Output {
            window_id: rssh_core::WindowId::new(1),
            pane_id: rssh_core::PaneId::new(1),
            runtime_generation: 40,
            bytes: b"-stale".to_vec(),
        }),
        Some(false)
    );

    let pending_text = manager
        .pending_apps
        .first()
        .map(|app| snapshot_row_text(&app.snapshot, 0, 16));
    assert_eq!(
        pending_text.as_deref().map(str::trim_end),
        Some("relocated")
    );
}

fn snapshot_char(
    snapshot: &rssh_renderer::TerminalRenderSnapshot,
    row: u16,
    column: u16,
) -> Option<char> {
    snapshot
        .cells()
        .iter()
        .find(|cell| cell.row == row && cell.column == column)
        .map(|cell| cell.ch)
}

fn snapshot_row_text(
    snapshot: &rssh_renderer::TerminalRenderSnapshot,
    row: u16,
    columns: u16,
) -> String {
    (0..columns)
        .map(|column| snapshot_char(snapshot, row, column).unwrap_or(' '))
        .collect()
}

struct DropTrackingWriter(Arc<AtomicUsize>);

impl Write for DropTrackingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for DropTrackingWriter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
