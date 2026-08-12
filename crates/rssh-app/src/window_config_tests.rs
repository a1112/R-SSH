use std::sync::Arc;

use super::{NativeConfigSnapshot, NativeWindowApp, NativeWindowConfigPatch};

#[test]
fn window_stores_one_shared_effective_config_and_small_runtime_patch() {
    let app = NativeWindowApp::new(Some(0));

    assert!(Arc::ptr_eq(
        &app.config_overrides.effective,
        &app.base_config_overrides.effective,
    ));
    assert!(
        std::mem::size_of::<NativeWindowConfigPatch>() <= std::mem::size_of::<usize>() * 2,
        "per-window runtime overrides must stay pointer-sized",
    );
}

#[test]
fn window_reload_reuses_unchanged_effective_config_subtrees() {
    let mut app = NativeWindowApp::new(Some(0));
    let before = Arc::clone(&app.config_overrides.effective);

    app.set_config_overrides(NativeConfigSnapshot {
        term: Some("xterm-rssh".to_owned()),
        ..NativeConfigSnapshot::default()
    });

    assert!(!Arc::ptr_eq(
        &before.terminal,
        &app.config_overrides.effective.terminal,
    ));
    assert!(Arc::ptr_eq(&before.font, &app.config_overrides.effective.font));
    assert!(Arc::ptr_eq(&before.input, &app.config_overrides.effective.input));
    assert!(Arc::ptr_eq(&before.window, &app.config_overrides.effective.window));
    assert!(Arc::ptr_eq(&before.render, &app.config_overrides.effective.render));
    assert!(Arc::ptr_eq(&before.domain, &app.config_overrides.effective.domain));
    assert!(Arc::ptr_eq(
        &before.lifecycle,
        &app.config_overrides.effective.lifecycle,
    ));
    assert_eq!(app.config_overrides.effective.terminal.term, "xterm-rssh");
}

#[test]
fn domain_and_lifecycle_values_live_in_one_copy_on_write_config_snapshot() {
    let mut app = NativeWindowApp::new(Some(0));
    let before = Arc::clone(&app.applied_config);

    app.set_config_overrides(NativeConfigSnapshot {
        default_domain: Some("local".to_owned()),
        default_workspace: Some("shared-config".to_owned()),
        ..NativeConfigSnapshot::default()
    });

    assert!(!Arc::ptr_eq(&before, &app.applied_config));
    assert_eq!(before.default_workspace, super::DEFAULT_WORKSPACE_NAME);
    assert_eq!(app.applied_config.default_workspace, "shared-config");
    assert_eq!(app.default_workspace, "shared-config");
    assert_eq!(
        std::mem::size_of_val(&app.applied_config),
        std::mem::size_of::<Arc<super::NativeAppliedConfig>>(),
    );

    let mut inherited = NativeWindowApp::new(Some(0));
    inherited.inherit_effective_config_from(&app);
    assert!(Arc::ptr_eq(&app.applied_config, &inherited.applied_config));
}
