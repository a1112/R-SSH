use super::*;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

#[test]
fn inspect_pane_typed_action_has_palette_metadata_and_no_default_shortcut() {
    let mut app = NativeWindowApp::new(None);

    assert!(WINDOW_COMMANDS.contains(&WindowCommand::InspectPane));
    assert_eq!(WindowCommand::InspectPane.label(), "Inspect Pane");
    assert!(
        !NATIVE_WINDOW_KEY_ASSIGNMENTS
            .iter()
            .any(|assignment| assignment.command == WindowCommand::InspectPane)
    );

    app.enter_command_palette_mode();
    app.command_palette_set_query("wezterm.action.InspectPane".to_owned());
    let labels = app
        .command_palette_filtered_commands()
        .iter()
        .map(WindowCommand::label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["Inspect Pane"]);

    assert!(app.command_palette_execute(WindowCommand::InspectPane));
    assert_eq!(app.command_palette_frecency("Inspect Pane").uses, 1);
    assert_eq!(app.pane_inspection, Some(app.active_pane_id()));
}

#[test]
fn pane_inspection_metadata_is_live_for_an_inactive_pane_and_hides_environment_values() {
    let mut app = NativeWindowApp::new_with_workspace(
        None,
        PtyCommand::default_shell(),
        Some("inspect-workspace"),
    );
    let source = app.active_pane_id();
    let launch = PaneLaunch::local("inspect-program")
        .with_args(["--mode", "safe"])
        .with_cwd("file://host/work")
        .with_environment([("PUBLIC_NAME", "secret-value")]);
    app.dispatch_app_action(AppAction::SplitPane {
        pane: source,
        direction: SplitDirection::Right,
        launch: Some(launch),
    })
    .unwrap();
    let inspected = app.active_pane_id();
    app.handle_pty_output(b"\x1b]2;live title\x07\x1b]7;file://host/work\x07")
        .unwrap();
    app.session_process_id = Some(42_424);
    app.dispatch_app_action(AppAction::ActivatePane { pane: source })
        .unwrap();

    let lines = app.pane_inspection_lines(inspected).unwrap();
    let text = lines.join("\n");

    assert!(text.contains(&format!("Pane {}", inspected.get())));
    assert!(text.contains("workspace: inspect-workspace (1)"));
    assert!(text.contains("tab: 1"));
    assert!(text.contains("pane: 2"));
    assert!(text.contains("title: live title"));
    assert!(text.contains("dimensions: 80x24"));
    assert!(text.contains("pid: 42424"));
    assert!(text.contains("cwd: file://host/work"), "{text}");
    assert!(text.contains("program: inspect-program"));
    assert!(text.contains("args: --mode safe"));
    assert!(text.contains("domain: local"));
    assert!(text.contains("environment: 1 variable"));
    assert!(!text.contains("secret-value"));

    app.pane_runtimes
        .get_mut(&inspected)
        .unwrap()
        .session_process_id = None;
    let refreshed = app.pane_inspection_lines(inspected).unwrap().join("\n");
    assert!(refreshed.contains("pid: unavailable"));
}

#[test]
fn pane_inspection_overlay_targets_inactive_rect_and_clips_tiny_panes() {
    let mut app = NativeWindowApp::new(None);
    let inactive = app.active_pane_id();
    app.dispatch_app_action(AppAction::SplitPane {
        pane: inactive,
        direction: SplitDirection::Right,
        launch: Some(PaneLaunch::local("right")),
    })
    .unwrap();
    app.request_pane_inspection(inactive);

    let layout = app.pane_render_layout();
    let target = layout
        .panes
        .iter()
        .find(|rect| rect.pane_id == inactive)
        .copied()
        .unwrap();
    let cells = app.pane_inspection_cells(&layout);

    assert!(!cells.is_empty());
    assert!(cells.iter().all(|cell| {
        cell.row >= target.row
            && cell.row < target.row.saturating_add(target.rows)
            && cell.column >= target.column
            && cell.column < target.column.saturating_add(target.columns)
    }));
    assert_eq!(cells[0].row, target.row);
    assert_eq!(cells[0].column, target.column);
    assert_eq!(cells[0].ch, 'P');
    assert_eq!(cells[0].foreground, PANE_INSPECTION_FOREGROUND);
    assert_eq!(cells[0].background, PANE_INSPECTION_BACKGROUND);
    let rendered = app.render_snapshot();
    let rendered_first = rendered
        .cells()
        .iter()
        .find(|cell| cell.row == target.row && cell.column == target.column)
        .unwrap();
    assert_eq!(rendered_first.ch, 'P');
    assert_eq!(rendered_first.background, PANE_INSPECTION_BACKGROUND);

    let tiny = PaneRenderRect {
        pane_id: inactive,
        row: 7,
        column: 11,
        rows: 1,
        columns: 1,
    };
    let tiny_cells = pane_inspection_cells_for_rect(&["Pane".to_owned()], tiny);
    assert_eq!(tiny_cells.len(), 1);
    assert_eq!((tiny_cells[0].row, tiny_cells[0].column), (7, 11));
    assert_eq!(tiny_cells[0].ch, 'P');
}

#[test]
fn wheel_inspect_targets_the_inactive_pane_without_focusing_it() {
    let mut app = NativeWindowApp::new(None);
    let inactive = app.active_pane_id();
    app.dispatch_app_action(AppAction::SplitPane {
        pane: inactive,
        direction: SplitDirection::Right,
        launch: Some(PaneLaunch::local("active")),
    })
    .unwrap();
    let active = app.active_pane_id();
    let rect = app.pane_render_rect(inactive).unwrap();
    let target = WheelTarget {
        pane_id: inactive,
        rect,
        cell: PaneMouseCell {
            pane_id: inactive,
            row: 0,
            column: 0,
        },
        pixel_position: PhysicalPosition::new(1.0, 1.0),
    };

    app.apply_wheel_pane_action(target, WindowCommand::InspectPane)
        .unwrap();

    assert_eq!(app.active_pane_id(), active);
    assert_eq!(app.pane_inspection, Some(inactive));
}

#[test]
fn inspect_refuses_existing_pane_overlay_and_clears_input_transients_when_opened() {
    let mut app = NativeWindowApp::new(None);
    let pane = app.active_pane_id();
    app.enter_copy_mode();

    app.request_pane_inspection(pane);

    assert!(app.pane_inspection.is_none());
    assert!(pane_copy_overlay_rendering(&app.active_ui));

    app.exit_copy_mode();
    app.ime_preedit = Some("compose".to_owned());
    app.dead_key_active = true;
    app.dead_key_text = Some("^".to_owned());
    app.selecting = true;
    app.active_mouse_button = Some(MouseButton::Left);
    app.scrollbar_dragging = true;
    app.current_mouse_wheel_delta = Some(MouseScrollDelta::LineDelta(0.0, 1.0));
    app.last_mouse_info = Some(ItermMouseInfo {
        pane_id: pane,
        x: 1,
        y: 1,
        button: 1,
        click_count: 1,
        modifier_mask: 0,
        side_effects: 0,
        event_type: 0,
    });

    app.request_pane_inspection(pane);

    assert_eq!(app.pane_inspection, Some(pane));
    assert!(app.ime_preedit.is_none());
    assert!(!app.dead_key_active);
    assert!(app.dead_key_text.is_none());
    assert!(!app.selecting);
    assert!(app.active_mouse_button.is_none());
    assert!(!app.scrollbar_dragging);
    assert!(app.current_mouse_wheel_delta.is_none());
    assert!(app.last_mouse_info.is_none());
    assert!(app.ui_left_release_pending);
}

#[test]
fn inspect_swallows_terminal_input_and_the_paired_close_key_release() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let mut app = NativeWindowApp::new(None);
    app.handle_pty_output(b"\x1b[?9001h\x1b[?1003h").unwrap();
    app.writer = Some(Box::new(SharedWriter(Arc::clone(&written))));
    app.clipboard_reader = Box::new(|| Some("clipboard-secret".to_owned()));
    app.mouse_position = Some((0, 0));
    app.mouse_pixel_position = Some(PhysicalPosition::new(1.0, 1.0));
    app.request_pane_inspection(app.active_pane_id());

    app.handle_keyboard_input_event(
        &Key::Character("x".into()),
        PhysicalKey::Code(WinitKeyCode::KeyX),
        Some("x"),
        ElementState::Pressed,
        KittyKeyEventKind::Press,
    )
    .unwrap();
    app.handle_ime_commit("かな").unwrap();
    assert!(app.handle_window_paste().unwrap());
    assert!(
        app.handle_dropped_file_path(std::path::Path::new("secret.txt"))
            .unwrap()
    );
    assert!(
        app.handle_mouse_input(ElementState::Pressed, MouseButton::Right)
            .unwrap()
    );
    assert!(
        app.handle_cursor_moved(PhysicalPosition::new(2.0, 2.0))
            .unwrap()
    );
    assert!(
        app.handle_window_mouse_wheel(MouseScrollDelta::LineDelta(0.0, 1.0))
            .unwrap()
    );
    assert!(written.lock().unwrap().is_empty());

    app.handle_keyboard_input_event(
        &Key::Named(NamedKey::Enter),
        PhysicalKey::Code(WinitKeyCode::Enter),
        None,
        ElementState::Pressed,
        KittyKeyEventKind::Press,
    )
    .unwrap();
    assert!(app.pane_inspection.is_none());
    app.handle_keyboard_input_event(
        &Key::Named(NamedKey::Enter),
        PhysicalKey::Code(WinitKeyCode::Enter),
        None,
        ElementState::Released,
        KittyKeyEventKind::Release,
    )
    .unwrap();

    app.request_pane_inspection(app.active_pane_id());
    app.handle_keyboard_input_event(
        &Key::Named(NamedKey::Escape),
        PhysicalKey::Code(WinitKeyCode::Escape),
        None,
        ElementState::Pressed,
        KittyKeyEventKind::Press,
    )
    .unwrap();
    app.handle_keyboard_input_event(
        &Key::Named(NamedKey::Escape),
        PhysicalKey::Code(WinitKeyCode::Escape),
        None,
        ElementState::Released,
        KittyKeyEventKind::Release,
    )
    .unwrap();

    assert!(written.lock().unwrap().is_empty());
}

#[test]
fn inspect_keeps_a_visible_stable_target_and_cancels_when_target_leaves_the_active_tab() {
    let mut app = NativeWindowApp::new(None);
    let source = app.active_pane_id();
    app.dispatch_app_action(AppAction::SplitPane {
        pane: source,
        direction: SplitDirection::Right,
        launch: Some(PaneLaunch::local("target")),
    })
    .unwrap();
    let target = app.active_pane_id();
    app.request_pane_inspection(target);

    app.dispatch_app_action(AppAction::ActivatePane { pane: source })
        .unwrap();
    assert_eq!(app.pane_inspection, Some(target));
    app.dispatch_app_action(AppAction::ActivatePane { pane: target })
        .unwrap();
    assert_eq!(app.pane_inspection, Some(target));

    app.dispatch_app_action(AppAction::NewTab { launch: None })
        .unwrap();
    assert!(app.pane_inspection.is_none());

    let new_tab = app.active_tab_id();
    app.dispatch_app_action(AppAction::ActivateTab {
        tab: rssh_core::TabId::new(1),
    })
    .unwrap();
    app.request_pane_inspection(target);
    app.dispatch_app_action(AppAction::MovePaneToNewWindow { pane: target })
        .unwrap();
    assert!(app.pane_inspection.is_none());

    app.dispatch_app_action(AppAction::ActivateTab { tab: new_tab })
        .unwrap();
    app.request_pane_inspection(app.active_pane_id());
    app.dispatch_app_action(AppAction::NewWorkspace {
        name: "other-workspace".to_owned(),
        launch: None,
    })
    .unwrap();
    assert!(app.pane_inspection.is_none());

    let source = app.active_pane_id();
    app.dispatch_app_action(AppAction::SplitPane {
        pane: source,
        direction: SplitDirection::Down,
        launch: Some(PaneLaunch::local("close-target")),
    })
    .unwrap();
    let close_target = app.active_pane_id();
    app.request_pane_inspection(close_target);
    app.dispatch_app_action(AppAction::ClosePane { pane: close_target })
        .unwrap();
    assert!(app.pane_inspection.is_none());
}

#[test]
fn pane_overlays_and_modals_opened_after_inspect_take_priority_by_cancelling_it() {
    let mut app = NativeWindowApp::new(None);
    let pane = app.active_pane_id();

    app.request_pane_inspection(pane);
    app.enter_copy_mode();
    assert!(app.pane_inspection.is_none());
    assert!(pane_copy_overlay_rendering(&app.active_ui));

    app.exit_copy_mode();
    app.request_pane_inspection(pane);
    app.enter_confirmation_mode(WindowConfirmationOptions {
        message: "higher priority".to_owned(),
        action: Box::new(WindowCommand::Nop),
        cancel: None,
    });
    assert!(app.pane_inspection.is_none());
    assert!(app.confirmation.is_some());

    app.exit_confirmation_mode();
    app.request_pane_inspection(pane);
    app.enter_close_confirmation_mode(WindowCloseTarget::Pane(pane));
    assert!(app.pane_inspection.is_none());
    assert!(app.close_confirmation.is_some());
}

#[test]
fn inspect_reads_restarted_runtime_metadata_without_replacing_the_stable_target() {
    let mut app = NativeWindowApp::new(None);
    let pane = app.active_pane_id();
    app.session_process_id = Some(7_001);
    app.handle_pty_output(b"\x1b]2;before restart\x07").unwrap();
    app.request_pane_inspection(pane);
    assert!(
        app.pane_inspection_lines(pane)
            .unwrap()
            .join("\n")
            .contains("pid: 7001")
    );

    app.restart_pane_runtime_with(pane, |app, _| {
        let mut runtime = app.new_inactive_pane_runtime();
        runtime
            .runtime
            .feed_pty_output_with_display(b"\x1b]2;after restart\x07");
        runtime.snapshot = terminal_runtime_snapshot(&runtime.runtime, runtime.ui.stable_viewport);
        runtime.session_process_id = Some(7_002);
        Ok::<_, Box<dyn std::error::Error>>(runtime)
    })
    .unwrap();

    assert_eq!(app.pane_inspection, Some(pane));
    let refreshed = app.pane_inspection_lines(pane).unwrap().join("\n");
    assert!(refreshed.contains("pid: 7002"));
    assert!(refreshed.contains("title: after restart"));
    assert!(!refreshed.contains("before restart"));
}

struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
