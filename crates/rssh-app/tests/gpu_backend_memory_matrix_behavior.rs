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
    assert_eq!(report["binary_source"], "override");
    assert_eq!(report["certification_eligible"], false);
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

#[test]
fn matrix_rejects_numeric_strings_and_object_or_array_adapter_identity() {
    let numeric_fixture = MatrixFixture::new("numeric-strings", "numeric-strings-dx12");
    let output = numeric_fixture.run();
    assert_success(&output);
    let report = numeric_fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    assert_eq!(probe(probes, "dx12")["status"], "failed");
    assert!(
        probe(probes, "dx12")["probe_failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("integer scalar"))
    );
    assert_eq!(probe(probes, "vulkan")["status"], "succeeded");

    let adapter_fixture = MatrixFixture::new("adapter-types", "object-array-vulkan-adapter");
    let output = adapter_fixture.run();
    assert_success(&output);
    let report = adapter_fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    assert_eq!(probe(probes, "vulkan")["status"], "failed");
    assert!(
        probe(probes, "vulkan")["probe_failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("actual non-empty string"))
    );
    assert_eq!(probe(probes, "gl")["status"], "succeeded");
}

#[test]
fn matrix_rejects_adapter_identity_drift_across_measured_runs() {
    let fixture = MatrixFixture::new("identity-drift", "drift-vulkan-adapter");
    let output = fixture.run_with_samples(2);
    assert_success(&output);

    let report = fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    let vulkan = probe(probes, "vulkan");
    assert_eq!(vulkan["status"], "failed");
    assert!(
        vulkan["probe_failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("GPU identity drift"))
    );
    for field in [
        "actual_gpu_backend",
        "adapter_name",
        "adapter_vendor_id",
        "adapter_device_id",
        "adapter_type",
    ] {
        assert!(
            vulkan.get(field).is_none(),
            "drifted GPU report leaked {field}"
        );
    }
    assert_eq!(probe(probes, "gl")["status"], "succeeded");
}

#[test]
fn matrix_redacts_resolved_and_original_override_paths_from_failures() {
    let fixture = MatrixFixture::new("path-redaction", "path-failure");
    let output = fixture.run();
    assert_success(&output);

    let report = fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    let message = probe(probes, "cpu")["probe_failure"]["message"]
        .as_str()
        .expect("CPU failure message");
    for path in fixture.redacted_paths() {
        assert!(
            !message.contains(path.to_str().expect("UTF-8 fixture path")),
            "failure leaked path {}: {message}",
            path.display()
        );
    }
    assert!(message.contains("[path]"));
}

#[test]
fn matrix_rejects_case_changed_wire_identity_values() {
    let first_fixture = MatrixFixture::new("uppercase-wire-a", "uppercase-wire-a");
    let output = first_fixture.run();
    assert_success(&output);
    let report = first_fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    for name in ["cpu", "dx12", "vulkan", "gl"] {
        assert_eq!(
            probe(probes, name)["status"],
            "failed",
            "case-changed wire value was accepted for {name}"
        );
    }

    let second_fixture = MatrixFixture::new("uppercase-wire-b", "uppercase-wire-b");
    let output = second_fixture.run();
    assert_success(&output);
    let report = second_fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    assert_eq!(probe(probes, "dx12")["status"], "failed");
    assert_eq!(probe(probes, "vulkan")["status"], "failed");
    assert_eq!(probe(probes, "gl")["status"], "succeeded");
}

#[test]
fn matrix_rejects_string_encoded_configuration_numbers() {
    let fixture = MatrixFixture::new("string-configuration", "string-config-dx12");
    let output = fixture.run();
    assert_success(&output);

    let report = fixture.report();
    let probes = report["probes"].as_array().expect("probe reports");
    let dx12 = probe(probes, "dx12");
    assert_eq!(dx12["status"], "failed");
    assert!(
        dx12["probe_failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("configuration.stabilization_ms"))
    );
    assert_eq!(probe(probes, "vulkan")["status"], "succeeded");
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
    app_argument: PathBuf,
    launcher_argument: PathBuf,
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
        fs::create_dir_all(root.join("alias")).expect("create alias path component");
        let app = root.join("fake-rssh-app.exe");
        fs::write(&app, b"fixture").expect("write fake app");
        let launcher_script = root.join("fake-launcher.ps1");
        let launcher = root.join("fake-launcher.cmd");
        let app_argument = root.join("alias/../fake-rssh-app.exe");
        let launcher_argument = root.join("alias/../fake-launcher.cmd");
        let fake_launcher = FAKE_LAUNCHER
            .replace("__FIXTURE_MODE__", mode)
            .replace(
                "__RESOLVED_LAUNCHER_PATH__",
                launcher.to_str().expect("UTF-8 launcher path"),
            )
            .replace(
                "__ORIGINAL_APP_PATH__",
                app_argument.to_str().expect("UTF-8 app argument"),
            )
            .replace(
                "__ORIGINAL_LAUNCHER_PATH__",
                launcher_argument.to_str().expect("UTF-8 launcher argument"),
            );
        fs::write(&launcher_script, fake_launcher).expect("write fake launcher script");
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
            app_argument,
            launcher_argument,
            output_directory,
        }
    }

    fn run(&self) -> Output {
        self.run_with_samples(1)
    }

    fn run_with_samples(&self, samples: u32) -> Output {
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
                &samples.to_string(),
                "-OutputDirectory",
                self.output_directory.to_str().expect("UTF-8 output path"),
                "-SkipBuild",
                "-AppPath",
                self.app_argument.to_str().expect("UTF-8 app path"),
                "-LauncherPath",
                self.launcher_argument
                    .to_str()
                    .expect("UTF-8 launcher path"),
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

    fn redacted_paths(&self) -> [&Path; 4] {
        [
            &self.app,
            &self.launcher,
            &self.app_argument,
            &self.launcher_argument,
        ]
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
$counterPath = Join-Path $PSScriptRoot "invocations-$probe.txt"
$invocation = if (Test-Path -LiteralPath $counterPath) { [int] (Get-Content -Raw $counterPath) + 1 } else { 1 }
Set-Content -LiteralPath $counterPath -Value $invocation
$configuration = [ordered]@{
    stabilization_ms = if ($mode -eq "string-config-dx12" -and $probe -eq "dx12") { "5000" } else { 5000 }
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
$schema = if ($mode -eq "uppercase-wire-a" -and $probe -eq "cpu") { "RSSH.DIAGNOSTICS/V2" } else { "rssh.diagnostics/v2" }
if ($mode -eq "uppercase-wire-a" -and $probe -eq "dx12") {
    $configuration["requested_renderer"] = "AUTO"
}
if ($mode -eq "uppercase-wire-a" -and $probe -eq "vulkan") {
    $configuration["requested_gpu_backend"] = "VULKAN"
}
$rendererSummary = if ($probe -eq "cpu") {
    [ordered]@{ first = "cpu"; final = "cpu" }
} else {
    $adapterName = if ($mode -eq "drift-vulkan-adapter" -and $probe -eq "vulkan" -and $invocation -gt 1) { "fixture-adapter-drifted" } else { "fixture-adapter" }
    if ($mode -eq "object-array-vulkan-adapter" -and $probe -eq "vulkan") {
        $adapterName = [ordered]@{ unexpected = "object" }
    }
    $adapterType = if ($mode -eq "object-array-vulkan-adapter" -and $probe -eq "vulkan") { @("discrete-gpu") } else { "discrete-gpu" }
    [ordered]@{
        first = "cpu"
        final = if ($mode -eq "uppercase-wire-b" -and $probe -eq "dx12") { "GPU" } else { "gpu" }
        backend = if ($mode -eq "uppercase-wire-b" -and $probe -eq "vulkan") { "VULKAN" } else { $backend }
        adapter_name = $adapterName
        adapter_vendor_id = if ($mode -eq "numeric-strings-dx12" -and $probe -eq "dx12") { "4318" } else { 4318 }
        adapter_device_id = if ($backend -eq "gl") { 0 } else { 9860 }
        adapter_type = $adapterType
    }
}
$samples = @(
    for ($sequence = 0; $sequence -lt 10; $sequence++) {
        $bytes = if ($mode -eq "malformed-dx12-bytes" -and $probe -eq "dx12" -and $sequence -eq 4) { $null } elseif ($mode -eq "numeric-strings-dx12" -and $probe -eq "dx12") { [string] (1048576 + $sequence) } else { 1048576 + $sequence }
        $sampleSequence = if ($mode -eq "numeric-strings-dx12" -and $probe -eq "dx12") { [string] $sequence } else { $sequence }
        [ordered]@{ sequence = $sampleSequence; elapsed_ms = 5000 + (100 * $sequence); bytes = $bytes }
    }
)
$failed = ($mode -eq "fail-cpu-and-dx12" -and ($probe -eq "cpu" -or $probe -eq "dx12")) -or $mode -eq "path-failure"
$failureMessage = if ($mode -eq "path-failure") {
    "$($options['--app'])|__RESOLVED_LAUNCHER_PATH__|__ORIGINAL_APP_PATH__|__ORIGINAL_LAUNCHER_PATH__"
} else {
    "requested fixture failure"
}
$record = [ordered]@{
    schema = $schema
    configuration = $configuration
    readiness = [ordered]@{ status = if ($failed) { "failed" } else { "ready" } }
    renderer = $rendererSummary
    memory = [ordered]@{
        metric = if ($mode -eq "uppercase-wire-a" -and $probe -eq "gl") { "WINDOWS_PRIVATE_WORKING_SET_BYTES" } else { "windows_private_working_set_bytes" }
        unit = "bytes"
        samples = $samples
    }
    failures = if ($failed) { @([ordered]@{ code = "fixture_failure"; phase = "fixture"; message = $failureMessage }) } else { @() }
}
$record | ConvertTo-Json -Depth 20 -Compress
"#;
