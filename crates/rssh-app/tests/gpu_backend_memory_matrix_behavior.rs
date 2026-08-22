#![cfg(target_os = "windows")]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

#[test]
fn matrix_accepts_strict_cpu_and_gpu_records_and_emits_exact_current_evidence() {
    let fixture = MatrixFixture::new("valid", "valid");
    let output = fixture.run();
    assert_success(&output);

    let report = fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    assert_eq!(probes.len(), 4);
    assert_cpu_identity_omitted(probe(probes, "cpu"));
    for backend in ["dx12", "vulkan", "gl"] {
        let report = probe(probes, backend);
        assert_eq!(report["status"], "succeeded");
        assert_eq!(report["requested_gpu_backend"], backend);
        assert_eq!(report["actual_gpu_backend"], backend);
        assert_eq!(report["adapter_name"], "fixture-adapter");
        assert_eq!(report["adapter_vendor_id"], 4318);
        let expected_device_id = if backend == "gl" { 0 } else { 9860 };
        assert_eq!(report["adapter_device_id"], expected_device_id);
        assert_eq!(report["adapter_type"], "discrete-gpu");
    }
    assert_eq!(
        report["evidence"]["raw_files"],
        serde_json::json!([
            "raw/cpu-01.json",
            "raw/dx12-01.json",
            "raw/vulkan-01.json",
            "raw/gl-01.json"
        ])
    );
}

#[test]
fn matrix_rejects_a_non_empty_output_directory_before_collecting_evidence() {
    let fixture = MatrixFixture::new("non-empty", "valid");
    fs::create_dir_all(&fixture.output_directory).expect("create output directory");
    fs::write(fixture.output_directory.join("stale.json"), b"stale").expect("write stale file");

    let output = fixture.run();
    assert!(
        !output.status.success(),
        "non-empty evidence directory was accepted"
    );
    assert!(
        combined_output(&output).contains("OutputDirectory must be empty"),
        "unexpected output: {}",
        combined_output(&output)
    );
    assert!(!fixture.output_directory.join("aggregate.json").exists());
}

#[test]
fn matrix_rejects_null_memory_bytes_without_coercing_them_to_zero() {
    let fixture = MatrixFixture::new("malformed-bytes", "malformed-dx12-bytes");
    let output = fixture.run();
    assert_success(&output);

    let report = fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    let dx12 = probe(probes, "dx12");
    assert_eq!(dx12["status"], "failed");
    assert!(
        dx12["probe_failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("sample bytes"))
    );
    assert_eq!(probe(probes, "vulkan")["status"], "succeeded");
    assert_eq!(probe(probes, "gl")["status"], "succeeded");
}

#[test]
fn matrix_continues_after_failed_cpu_and_gpu_probes_with_safe_identity_fields() {
    let fixture = MatrixFixture::new("failed-probes", "fail-cpu-and-dx12");
    let output = fixture.run();
    assert_success(&output);

    let report = fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    assert_eq!(probes.len(), 4);
    let cpu = probe(probes, "cpu");
    assert_eq!(cpu["status"], "failed");
    assert_cpu_identity_omitted(cpu);
    let dx12 = probe(probes, "dx12");
    assert_eq!(dx12["status"], "failed");
    assert_eq!(dx12["requested_gpu_backend"], "dx12");
    for field in [
        "actual_gpu_backend",
        "adapter_name",
        "adapter_vendor_id",
        "adapter_device_id",
        "adapter_type",
    ] {
        assert!(
            dx12.get(field).is_none(),
            "failed GPU report leaked {field}"
        );
    }
    assert_eq!(probe(probes, "vulkan")["status"], "succeeded");
    assert_eq!(probe(probes, "gl")["status"], "succeeded");
}

fn assert_cpu_identity_omitted(report: &Value) {
    for field in [
        "requested_gpu_backend",
        "actual_gpu_backend",
        "adapter_name",
        "adapter_vendor_id",
        "adapter_device_id",
        "adapter_type",
    ] {
        assert!(report.get(field).is_none(), "CPU report leaked {field}");
    }
}

fn probe<'a>(probes: &'a [Value], name: &str) -> &'a Value {
    probes
        .iter()
        .find(|probe| probe["name"] == name)
        .unwrap_or_else(|| panic!("missing probe {name}"))
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "matrix runner failed: {}",
        combined_output(output)
    );
}

fn combined_output(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

struct MatrixFixture {
    root: PathBuf,
    app: PathBuf,
    launcher: PathBuf,
    output_directory: PathBuf,
}

impl MatrixFixture {
    fn new(label: &str, mode: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rssh-gpu-matrix-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create fixture root");
        let app = root.join("fake-rssh-app.exe");
        fs::write(&app, b"fixture").expect("write fake app");
        let launcher_script = root.join("fake-launcher.ps1");
        fs::write(
            &launcher_script,
            FAKE_LAUNCHER.replace("__FIXTURE_MODE__", mode),
        )
        .expect("write fake launcher script");
        let launcher = root.join("fake-launcher.cmd");
        fs::write(
            &launcher,
            b"@echo off\r\npwsh.exe -NoProfile -NonInteractive -File \"%~dp0fake-launcher.ps1\" %*\r\nexit /b %errorlevel%\r\n",
        )
        .expect("write fake launcher command");
        let output_directory = root.join("evidence");
        Self {
            root,
            app,
            launcher,
            output_directory,
        }
    }

    fn run(&self) -> Output {
        Command::new("pwsh.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-File",
                repo_path("scripts/ci/run-gpu-backend-memory-matrix.ps1")
                    .to_str()
                    .expect("UTF-8 runner path"),
                "-Profile",
                "release",
                "-Warmups",
                "0",
                "-Samples",
                "1",
                "-OutputDirectory",
                self.output_directory.to_str().expect("UTF-8 output path"),
                "-SkipBuild",
                "-AppPath",
                self.app.to_str().expect("UTF-8 app path"),
                "-LauncherPath",
                self.launcher.to_str().expect("UTF-8 launcher path"),
            ])
            .current_dir(repo_path("."))
            .output()
            .expect("execute matrix runner")
    }

    fn report(&self) -> Value {
        let path = self.output_directory.join("aggregate.json");
        let bytes =
            fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        serde_json::from_slice(&bytes).expect("parse aggregate JSON")
    }
}

impl Drop for MatrixFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn repo_path(relative: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

const FAKE_LAUNCHER: &str = r#"
$mode = "__FIXTURE_MODE__"
$options = @{}
for ($index = 0; $index -lt $args.Count; $index++) {
    if ($args[$index].StartsWith("--") -and $index + 1 -lt $args.Count -and -not $args[$index + 1].StartsWith("--")) {
        $options[$args[$index]] = $args[$index + 1]
        $index++
    }
}
$renderer = $options["--renderer"]
$backend = $options["--gpu-backend"]
$probe = if ($renderer -eq "cpu") { "cpu" } else { $backend }
$configuration = [ordered]@{
    stabilization_ms = 5000
    sample_interval_ms = 100
    sample_count = 10
    columns = 80
    rows = 24
    scale_factor_milli = 1000
    requested_renderer = $renderer
}
if ($null -ne $backend) {
    $configuration["requested_gpu_backend"] = $backend
}
$rendererSummary = if ($probe -eq "cpu") {
    [ordered]@{ first = "cpu"; final = "cpu" }
} else {
    [ordered]@{
        first = "cpu"
        final = "gpu"
        backend = $backend
        adapter_name = "fixture-adapter"
        adapter_vendor_id = 4318
        adapter_device_id = if ($backend -eq "gl") { 0 } else { 9860 }
        adapter_type = "discrete-gpu"
    }
}
$samples = @(
    for ($sequence = 0; $sequence -lt 10; $sequence++) {
        $bytes = if ($mode -eq "malformed-dx12-bytes" -and $probe -eq "dx12" -and $sequence -eq 4) { $null } else { 1048576 + $sequence }
        [ordered]@{ sequence = $sequence; elapsed_ms = 5000 + (100 * $sequence); bytes = $bytes }
    }
)
$failed = $mode -eq "fail-cpu-and-dx12" -and ($probe -eq "cpu" -or $probe -eq "dx12")
$record = [ordered]@{
    schema = "rssh.diagnostics/v2"
    configuration = $configuration
    readiness = [ordered]@{ status = if ($failed) { "failed" } else { "ready" } }
    renderer = $rendererSummary
    memory = [ordered]@{
        metric = "windows_private_working_set_bytes"
        unit = "bytes"
        samples = $samples
    }
    failures = if ($failed) { @([ordered]@{ code = "fixture_failure"; phase = "fixture"; message = "requested fixture failure" }) } else { @() }
}
$record | ConvertTo-Json -Depth 20 -Compress
"#;
