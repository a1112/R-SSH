use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[test]
fn release_renderer_excludes_identifier_limit_test_api_and_artifacts() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("renderer crate belongs to workspace");
    let probe = workspace
        .join("target")
        .join("tmp")
        .join("renderer-release-api");
    let source = probe.join("src");
    fs::create_dir_all(&source).expect("create release API probe");
    let renderer = toml_path(&workspace.join("crates/rssh-renderer"));
    let fonts = toml_path(&workspace.join("crates/rssh-fonts"));
    let glyphon = toml_path(&workspace.join("vendor/glyphon-0.12.0"));
    let gpu_allocator = toml_path(&workspace.join("vendor/gpu-allocator-0.28.0"));
    fs::write(
        probe.join("Cargo.toml"),
        format!(
            "[package]\nname = \"renderer-release-api-probe\"\nversion = \"0.0.0\"\n\
             edition = \"2024\"\n\n[workspace]\n\n[dependencies]\nrssh-renderer = {{ path = \"{renderer}\" }}\n\
             rssh-fonts = {{ path = \"{fonts}\" }}\n\n[patch.crates-io]\n\
             glyphon = {{ path = \"{glyphon}\" }}\n\
             gpu-allocator = {{ path = \"{gpu_allocator}\" }}\n"
        ),
    )
    .expect("write release API probe manifest");
    fs::write(
        source.join("main.rs"),
        "use rssh_fonts::RasterCacheConfig;\n\
         use rssh_renderer::gpu::GpuTextConfig;\n\
         fn main() {\n\
             let _ = GpuTextConfig::new(1024, RasterCacheConfig::new(1024))\n\
                 .with_identifier_limit_for_tests(1);\n\
         }\n",
    )
    .expect("write release API probe source");
    let target = probe.join("target");
    let output = Command::new(env!("CARGO"))
        .args([
            "check",
            "--release",
            "--offline",
            "--manifest-path",
            probe.join("Cargo.toml").to_str().expect("UTF-8 probe path"),
            "--target-dir",
            target.to_str().expect("UTF-8 target path"),
        ])
        .output()
        .expect("run release API probe");
    assert!(
        !output.status.success(),
        "release renderer unexpectedly exposed with_identifier_limit_for_tests"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no method named `with_identifier_limit_for_tests`")
            || stderr.contains("no method named 'with_identifier_limit_for_tests'"),
        "release API probe failed for an unrelated reason:\n{stderr}"
    );

    let deps = target.join("release/deps");
    for artifact in release_renderer_artifacts(&deps) {
        let bytes = fs::read(&artifact).expect("read release renderer artifact");
        for forbidden in [
            b"with_identifier_limit_for_tests".as_slice(),
            b"configured identifier limit".as_slice(),
        ] {
            assert!(
                !bytes
                    .windows(forbidden.len())
                    .any(|window| window == forbidden),
                "release artifact {} contains forbidden test seam {:?}",
                artifact.display(),
                String::from_utf8_lossy(forbidden)
            );
        }
    }
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn release_renderer_artifacts(deps: &Path) -> Vec<PathBuf> {
    fs::read_dir(deps)
        .expect("release probe deps directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            name.starts_with("librssh_renderer-")
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("rlib" | "rmeta")
                )
        })
        .collect()
}
