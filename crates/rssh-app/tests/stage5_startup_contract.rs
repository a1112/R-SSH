use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

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
    assert!(manifest.contains("diagnostic-tools = [\"rssh-fonts/diagnostic-tools\"]"));
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
fn packaged_font_dependency_cannot_name_private_proof_constructors() {
    let packaged = compile_font_proof_consumer("packaged", false);
    assert!(
        !packaged.status.success(),
        "packaged dependency unexpectedly named proof constructors"
    );
    let packaged_stderr = String::from_utf8_lossy(&packaged.stderr);
    for constructor in [
        "from_sources_shared_for_diagnostics",
        "from_sources_copied_for_diagnostics",
    ] {
        assert!(
            packaged_stderr.contains(constructor),
            "packaged compile failure omitted {constructor}: {packaged_stderr}"
        );
    }

    let diagnostic = compile_font_proof_consumer("diagnostic", true);
    assert!(
        diagnostic.status.success(),
        "diagnostic feature did not expose proof constructors: stdout={} stderr={}",
        String::from_utf8_lossy(&diagnostic.stdout),
        String::from_utf8_lossy(&diagnostic.stderr)
    );
}

fn compile_font_proof_consumer(label: &str, diagnostic_tools: bool) -> Output {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rssh-font-feature-contract-{label}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("src")).expect("create compile contract fixture");
    let font_path = repository_root()
        .join("crates/rterm-fonts")
        .to_string_lossy()
        .replace('\\', "/");
    let features = if diagnostic_tools {
        ", features = [\"diagnostic-tools\"]"
    } else {
        ""
    };
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"font-feature-contract-{label}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nrterm-fonts = {{ path = \"{font_path}\", default-features = false{features} }}\n"
        ),
    )
    .expect("write compile contract manifest");
    fs::write(
        root.join("src/main.rs"),
        r#"use rterm_fonts::{FontCatalog, FontSource};

fn main() {
    let _ = FontCatalog::from_sources_shared_for_diagnostics(
        "en-US",
        std::iter::empty::<FontSource>(),
    );
    let _ = FontCatalog::from_sources_copied_for_diagnostics(
        "en-US",
        std::iter::empty::<FontSource>(),
    );
}
"#,
    )
    .expect("write compile contract source");
    let target = root.join("target");
    let temporary = root.join("tmp");
    fs::create_dir_all(&temporary).expect("create compile contract temp");
    let output = Command::new("cargo")
        .args(["check", "--offline", "--quiet", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target)
        .env("TEMP", &temporary)
        .env("TMP", &temporary)
        .env("TMPDIR", &temporary)
        .output()
        .expect("execute compile feature contract");
    remove_fixture(&root);
    output
}

fn remove_fixture(root: &Path) {
    fs::remove_dir_all(root).unwrap_or_else(|error| {
        panic!(
            "remove compile contract fixture {}: {error}",
            root.display()
        )
    });
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
