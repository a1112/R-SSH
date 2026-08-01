use std::{fs, path::PathBuf};

const ARTIFACTS: [(&str, &str, &str); 6] = [
    ("R-SSH-windows-x64.zip", "windows-x86_64", "windows-conpty"),
    (
        "R-SSH-windows-arm64.zip",
        "windows-aarch64",
        "windows-conpty",
    ),
    ("R-SSH-linux-x64.tar.gz", "linux-x86_64", "unix-pty"),
    ("R-SSH-linux-arm64.tar.gz", "linux-aarch64", "unix-pty"),
    ("R-SSH-macos-x64.tar.gz", "macos-x86_64", "unix-pty"),
    ("R-SSH-macos-arm64.tar.gz", "macos-aarch64", "unix-pty"),
];

#[test]
fn release_declares_six_stable_native_artifacts_and_runtime_identities() {
    let workflow = read_repo_file(".github/workflows/release.yml");

    for (artifact, target, pty_backend) in ARTIFACTS {
        assert!(
            workflow.contains(artifact),
            "release workflow is missing stable artifact {artifact}"
        );
        assert!(
            workflow.contains(target),
            "release workflow is missing runtime target {target}"
        );
        assert!(
            workflow.contains(pty_backend),
            "release workflow is missing PTY backend {pty_backend}"
        );
    }
}

#[test]
fn package_smoke_and_machine_readable_manifests_are_mandatory() {
    let workflow = read_repo_file(".github/workflows/release.yml");
    let info_plist = read_repo_file("packaging/Info.plist");

    for path in [
        "scripts/ci/package-smoke.ps1",
        "scripts/ci/package-smoke.sh",
        "packaging/package-manifest.json",
        "packaging/rssh-console.cmd",
        "packaging/rssh-console.sh",
        "packaging/Info.plist",
    ] {
        assert!(repo_root().join(path).is_file(), "missing {path}");
    }

    for contract in [
        "package-smoke.ps1",
        "package-smoke.sh",
        "manifest.json",
        "SHA256SUMS",
        "--harness-self-test",
        "RSSH_TEST_APP_EXECUTABLE",
        "RSSH_REQUIRE_OPENSSH",
        "native_window_e2e",
    ] {
        assert!(
            workflow.contains(contract),
            "release workflow is missing package contract {contract}"
        );
    }
    assert!(workflow.matches("dist/signed-package-smoke").count() >= 6);
    assert!(workflow.contains("cargo build --locked --release -p rssh-app --all-targets"));
    assert!(info_plist.contains("LSArchitecturePriority"));
    assert!(info_plist.contains("__ARCHITECTURE__"));
    let unix_smoke = read_repo_file("scripts/ci/package-smoke.sh");
    let unix_package = read_repo_file("scripts/ci/package-native.sh");
    for contract in [
        "packaged macOS CLI launcher",
        "validate packaged macOS bundle identity",
        "CFBundleExecutable",
        "CFBundleShortVersionString",
        "LSArchitecturePriority",
    ] {
        assert!(
            unix_smoke.contains(contract),
            "missing macOS smoke {contract}"
        );
    }
    assert!(unix_package.contains("'rssh-app', 'R-SSH.app/Contents/Info.plist'"));
}

#[test]
fn tag_publication_requires_protected_signing_smoke_and_attestation() {
    let workflow = read_repo_file(".github/workflows/release.yml");

    for contract in [
        "release-windows-signing",
        "release-linux-signing",
        "release-macos-signing",
        "signtool",
        "cosign",
        "notarytool",
        "stapler",
        "sbom",
        "attest-build-provenance",
        "attestations: write",
        "id-token: write",
        "signed-package-smoke",
    ] {
        assert!(
            workflow.contains(contract),
            "release workflow is missing protected release contract {contract}"
        );
    }

    assert!(workflow.contains("needs: attest-release"));
    assert!(workflow.contains("permissions:\n  contents: read"));
    assert_action_refs_are_pinned(&workflow);
}

#[test]
fn build_matrix_maps_each_native_runner_without_secrets_or_write_permissions() {
    let workflow = read_repo_file(".github/workflows/release.yml");
    let build = job_section(&workflow, "build-package", "sign-windows");
    for mapping in [
        (
            "windows-x64",
            "windows-2025",
            "x86_64-pc-windows-msvc",
            "windows-x86_64",
            "windows-conpty",
            "R-SSH-windows-x64-unsigned",
            "R-SSH-windows-x64.zip",
            "R-SSH-windows-x64-unsigned.zip",
            "target/release/rssh-app.exe",
        ),
        (
            "windows-arm64",
            "windows-11-arm",
            "aarch64-pc-windows-msvc",
            "windows-aarch64",
            "windows-conpty",
            "R-SSH-windows-arm64-unsigned",
            "R-SSH-windows-arm64.zip",
            "R-SSH-windows-arm64-unsigned.zip",
            "target/release/rssh-app.exe",
        ),
        (
            "linux-x64",
            "ubuntu-24.04",
            "x86_64-unknown-linux-gnu",
            "linux-x86_64",
            "unix-pty",
            "R-SSH-linux-x64-unsigned",
            "R-SSH-linux-x64.tar.gz",
            "R-SSH-linux-x64-unsigned.tar.gz",
            "target/release/rssh-app",
        ),
        (
            "linux-arm64",
            "ubuntu-24.04-arm",
            "aarch64-unknown-linux-gnu",
            "linux-aarch64",
            "unix-pty",
            "R-SSH-linux-arm64-unsigned",
            "R-SSH-linux-arm64.tar.gz",
            "R-SSH-linux-arm64-unsigned.tar.gz",
            "target/release/rssh-app",
        ),
        (
            "macos-x64",
            "macos-15-intel",
            "x86_64-apple-darwin",
            "macos-x86_64",
            "unix-pty",
            "R-SSH-macos-x64-unsigned",
            "R-SSH-macos-x64.tar.gz",
            "R-SSH-macos-x64-unsigned.tar.gz",
            "target/release/rssh-app",
        ),
        (
            "macos-arm64",
            "macos-15",
            "aarch64-apple-darwin",
            "macos-aarch64",
            "unix-pty",
            "R-SSH-macos-arm64-unsigned",
            "R-SSH-macos-arm64.tar.gz",
            "R-SSH-macos-arm64-unsigned.tar.gz",
            "target/release/rssh-app",
        ),
    ] {
        let contract = format!(
            "- slug: {}\n            runner: {}\n            rust_target: {}\n            runtime_target: {}\n            pty_backend: {}\n            package_root: {}\n            artifact: {}\n            unsigned_artifact: {}\n            binary: {}",
            mapping.0,
            mapping.1,
            mapping.2,
            mapping.3,
            mapping.4,
            mapping.5,
            mapping.6,
            mapping.7,
            mapping.8,
        );
        assert!(
            build.contains(&contract),
            "missing runner mapping {mapping:?}"
        );
    }
    assert_eq!(build.matches("- slug:").count(), 6);
    assert!(build.contains("permissions:\n      contents: read"));
    assert!(!build.contains("contents: write"));
    assert!(!build.contains("${{ secrets."));
    assert!(build.contains("persist-credentials: false"));
    for command in [
        "cargo test --locked --workspace --all-targets",
        "cargo clippy --locked --workspace --all-targets -- -D warnings",
        "cargo build --locked --release -p rssh-app --all-targets",
    ] {
        assert!(build.contains(command), "build job is missing {command}");
    }
}

#[test]
fn protected_jobs_are_scoped_and_publication_has_a_complete_dag() {
    let workflow = read_repo_file(".github/workflows/release.yml");
    let windows = job_section(&workflow, "sign-windows", "sign-linux");
    let linux = job_section(&workflow, "sign-linux", "sign-macos");
    let macos = job_section(&workflow, "sign-macos", "attest-release");
    let attest = job_section(&workflow, "attest-release", "publish-release");
    let publish = workflow
        .split("  publish-release:\n")
        .nth(1)
        .expect("publish-release job");

    for (section, environment, signed_smoke) in [
        (
            windows,
            "release-windows-signing",
            "signed-package-smoke Windows",
        ),
        (linux, "release-linux-signing", "signed-package-smoke Linux"),
        (macos, "release-macos-signing", "signed-package-smoke macOS"),
    ] {
        assert!(section.contains("if: startsWith(github.ref, 'refs/tags/v')"));
        assert!(section.contains(&format!("environment: {environment}")));
        assert!(section.contains("needs: build-package"));
        assert!(section.contains(signed_smoke));
        assert!(section.contains("dist/signed-package-smoke"));
    }
    assert!(windows.contains("${{ secrets.WINDOWS_SIGNING_CERTIFICATE_BASE64 }}"));
    assert!(windows.contains("$archivedBinary = (Resolve-Path"));
    assert!(windows.contains("signtool verify /pa /all /v $archivedBinary"));
    assert!(windows.contains("$archivedSignature = Get-AuthenticodeSignature"));
    assert!(linux.contains("id-token: write"));
    assert!(macos.contains("${{ secrets.MACOS_NOTARY_PRIVATE_KEY_BASE64 }}"));
    assert!(macos.contains("archived_app=\"dist/signed-package-smoke/"));
    assert!(macos.contains("codesign --verify --deep --strict --verbose=2 \"$archived_app\""));
    assert!(macos.contains("xcrun stapler validate \"$archived_app\""));
    assert!(attest.contains("needs: [sign-windows, sign-linux, sign-macos]"));
    assert!(attest.contains("environment: release-provenance"));
    assert!(attest.contains("attestations: write"));
    assert!(attest.contains("id-token: write"));
    assert!(publish.contains("needs: attest-release"));
    assert!(attest.contains("if: startsWith(github.ref, 'refs/tags/v')"));
    assert!(publish.contains("if: startsWith(github.ref, 'refs/tags/v')"));
    assert!(publish.contains("environment: release-publish"));
    assert!(publish.contains("contents: write"));
    assert!(!publish.contains("${{ secrets."));
    for artifact in ARTIFACTS.map(|entry| entry.0) {
        assert!(publish.contains(artifact));
    }
    assert_all_sign_matrices(windows, linux, macos);
}

fn assert_all_sign_matrices(windows: &str, linux: &str, macos: &str) {
    assert_sign_matrix(
        windows,
        [
            (
                "windows-x64",
                "windows-2025",
                "windows-x86_64",
                "R-SSH-windows-x64",
                "R-SSH-windows-x64.zip",
            ),
            (
                "windows-arm64",
                "windows-11-arm",
                "windows-aarch64",
                "R-SSH-windows-arm64",
                "R-SSH-windows-arm64.zip",
            ),
        ],
    );
    assert_sign_matrix(
        linux,
        [
            (
                "linux-x64",
                "ubuntu-24.04",
                "linux-x86_64",
                "R-SSH-linux-x64",
                "R-SSH-linux-x64.tar.gz",
            ),
            (
                "linux-arm64",
                "ubuntu-24.04-arm",
                "linux-aarch64",
                "R-SSH-linux-arm64",
                "R-SSH-linux-arm64.tar.gz",
            ),
        ],
    );
    assert_sign_matrix(
        macos,
        [
            (
                "macos-x64",
                "macos-15-intel",
                "macos-x86_64",
                "R-SSH-macos-x64",
                "R-SSH-macos-x64.tar.gz",
            ),
            (
                "macos-arm64",
                "macos-15",
                "macos-aarch64",
                "R-SSH-macos-arm64",
                "R-SSH-macos-arm64.tar.gz",
            ),
        ],
    );
}

fn assert_sign_matrix(section: &str, entries: [(&str, &str, &str, &str, &str); 2]) {
    for (slug, runner, runtime, root, artifact) in entries {
        let pty = if runtime.starts_with("windows-") {
            "windows-conpty"
        } else {
            "unix-pty"
        };
        let contract = format!(
            "- slug: {slug}\n            runner: {runner}\n            runtime_target: {runtime}\n            pty_backend: {pty}\n            package_root: {root}\n            artifact: {artifact}"
        );
        assert!(
            section.contains(&contract),
            "miswired signed artifact {slug}"
        );
    }
}

fn read_repo_file(path: &str) -> String {
    let path = repo_root().join(path);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

fn assert_action_refs_are_pinned(workflow: &str) {
    for action in workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- uses: "))
    {
        let revision = action
            .split_once('@')
            .unwrap_or_else(|| panic!("action has no revision: {action}"))
            .1
            .split_whitespace()
            .next()
            .expect("action revision after @");
        assert_eq!(
            revision.len(),
            40,
            "action is not pinned to a full SHA: {action}"
        );
        assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}

fn job_section<'a>(workflow: &'a str, job: &str, next_job: &str) -> &'a str {
    workflow
        .split(&format!("  {job}:\n"))
        .nth(1)
        .unwrap_or_else(|| panic!("missing job {job}"))
        .split(&format!("\n  {next_job}:\n"))
        .next()
        .expect("job section before next job")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root above rssh-app")
        .to_owned()
}
