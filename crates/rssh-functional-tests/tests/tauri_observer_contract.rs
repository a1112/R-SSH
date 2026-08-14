use std::{fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn tauri_test_build_projects_web_state_into_the_read_only_observer() {
    let root = root();
    let manifest = fs::read_to_string(root.join("tauri/src-tauri/Cargo.toml")).unwrap();
    let backend = fs::read_to_string(root.join("tauri/src-tauri/src/lib.rs")).unwrap();
    let web = fs::read_to_string(root.join("web/src/main.ts")).unwrap();

    assert!(manifest.contains("functional-test-observer"));
    assert!(manifest.contains("rssh-functional-tests"));
    assert!(backend.contains("functional_observer_publish"));
    assert!(backend.contains("ObserverServer::bind"));
    assert!(backend.contains("wait_until_delivered"));
    assert!(web.contains("invoke('functional_observer_publish'"));
    assert!(web.contains("terminal.onWriteParsed"));
}

#[test]
fn closing_the_main_window_drives_backend_cleanup_and_app_exit() {
    let backend =
        fs::read_to_string(root().join("tauri/src-tauri/src/lib.rs")).expect("Tauri backend");

    assert!(backend.contains("tauri::WindowEvent::CloseRequested"));
    assert!(backend.contains("label == \"main\""));
    assert!(backend.contains("api.prevent_close()"));
    assert!(backend.contains("exit_after_final_observation_delivery"));
    assert!(backend.contains("Duration::from_secs(2)"));
    assert!(backend.contains("thread::sleep(Duration::from_secs(1))"));
    assert!(backend.contains("exit_handle.exit(0)"));
}

#[test]
fn production_tauri_build_does_not_enable_the_observer_feature() {
    let package = fs::read_to_string(root().join("tauri/package.json")).unwrap();
    assert!(package.contains("--features functional-test-observer"));
    assert!(package.contains("build:functional"));
    assert!(
        !package.lines().any(|line| {
            line.contains("\"build\"") && line.contains("functional-test-observer")
        })
    );
}

#[test]
fn tauri_remote_ipc_is_scoped_to_the_functional_feature_and_loopback() {
    let root = root();
    let build = fs::read_to_string(root.join("tauri/src-tauri/build.rs")).unwrap();
    let production =
        fs::read_to_string(root.join("tauri/src-tauri/capabilities/production/default.json"))
            .unwrap();
    let functional =
        fs::read_to_string(root.join("tauri/src-tauri/capabilities/functional/observer.json"))
            .unwrap();
    let functional_permission =
        fs::read_to_string(root.join("tauri/src-tauri/permissions/functional/observer.toml"))
            .unwrap();

    assert!(build.contains("CARGO_FEATURE_FUNCTIONAL_TEST_OBSERVER"));
    assert!(build.contains("AppManifest::new"));
    assert!(build.contains("capabilities/production"));
    assert!(build.contains("capabilities/functional"));

    assert!(production.contains("http://127.0.0.1:*"));
    assert!(!production.contains("functional_observer_publish"));
    assert!(!production.contains("allow-functional-observer-publish"));
    assert!(!production.contains("core:default"));
    assert!(functional.contains("http://127.0.0.1:*"));
    assert!(functional.contains("allow-functional-observer-publish"));
    assert!(!functional.contains("core:default"));
    assert!(functional_permission.contains("functional_observer_publish"));
    assert!(!functional.contains("https://"));
    assert!(!functional.contains("http://*"));
}
