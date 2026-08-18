use std::{fs, path::PathBuf};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn read_repository_file(path: &str) -> String {
    fs::read_to_string(repository_root().join(path)).expect("read repository contract file")
}

#[test]
fn cpu_renderer_separates_basic_gif_and_legacy_image_decoders() {
    let manifest = read_repository_file("crates/rterm-render-cpu/Cargo.toml");

    assert!(manifest.contains("default = [\"image-basic\", \"image-gif\", \"image-legacy\"]"));
    assert!(manifest.contains("image-basic = [\"image/png\", \"image/jpeg\"]"));
    assert!(manifest.contains("image-gif = [\"image/gif\"]"));
    assert!(manifest.contains(
        "image-legacy = [\"image/dds\", \"image/ff\", \"image/ico\", \"image/pnm\", \"image/tga\", \"image/tiff\"]"
    ));
    assert!(manifest.contains("default-features = false"));
}

#[test]
fn packaged_gui_feature_excludes_diagnostics_transfers_and_optional_images() {
    let manifest = read_repository_file("crates/rssh-app/Cargo.toml");

    assert!(
        manifest
            .contains("production-gui = [\"native-gui\", \"ssh\", \"local-pty\", \"image-basic\"]")
    );
    assert!(manifest.contains("diagnostic-tools = []"));
    assert!(manifest.contains("transfer-tools = []"));
    assert!(
        !manifest
            .lines()
            .find(|line| line.starts_with("production-gui ="))
            .expect("production GUI feature")
            .contains("diagnostic-tools")
    );
    assert!(
        !manifest
            .lines()
            .find(|line| line.starts_with("production-gui ="))
            .expect("production GUI feature")
            .contains("transfer-tools")
    );
}

#[test]
fn release_packaging_builds_the_explicit_minimal_gui_feature_set() {
    let release = read_repository_file(".github/workflows/release.yml");
    let windows_smoke = read_repository_file("scripts/ci/package-smoke.ps1");
    let unix_smoke = read_repository_file("scripts/ci/package-smoke.sh");
    let package_job = release
        .split_once("  build-package:")
        .expect("package job")
        .1
        .split_once("\n  publish-release:")
        .map_or_else(|| release.as_str(), |(job, _)| job);

    assert!(package_job.contains(
        "cargo build --locked --release -p rssh-app --no-default-features --features production-gui"
    ));
    for smoke in [&windows_smoke, &unix_smoke] {
        assert!(!smoke.contains("packaged doctor"));
        assert!(!smoke.contains("packaged self-test"));
        assert!(!smoke.contains("packaged benchmark gate"));
        assert!(smoke.contains("packaged OpenSSH loopback"));
        assert!(smoke.contains("packaged native ten-frame E2E"));
    }
}
