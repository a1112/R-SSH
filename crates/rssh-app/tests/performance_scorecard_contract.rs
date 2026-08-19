use std::{fs, path::PathBuf, process::Command};

use serde_json::Value;

#[test]
fn checked_in_baseline_contains_the_approved_measurements_and_gates() {
    let baseline = read_repo_json("scripts/perf/baselines/windows-x64-rust-1.89.json");
    let schema = read_repo_json("scripts/perf/scorecard.schema.json");

    assert_eq!(
        schema["$id"],
        "https://r-ssh.dev/schemas/performance-scorecard-v1.json"
    );
    assert_eq!(baseline["schema_version"], 1);
    assert_eq!(
        baseline["baseline_commit"],
        "9e99ba755fbdbd8896d849bc8934e187ab9062b5"
    );
    assert_eq!(baseline["machine"]["os"], "windows");
    assert_eq!(baseline["machine"]["arch"], "x86_64");
    assert_eq!(baseline["machine"]["rustc"], "1.89.0");
    assert_eq!(baseline["machine"]["cargo"], "1.89.0");
    assert_non_empty_string(&baseline["machine"]["cpu"], "machine.cpu");
    assert_non_empty_string(
        &baseline["machine"]["power_profile"],
        "machine.power_profile",
    );

    assert_eq!(baseline["protocol"]["warmups"], 2);
    assert_eq!(baseline["protocol"]["samples"], 7);
    assert_eq!(baseline["protocol"]["bytes"], 1_048_576);
    assert_eq!(baseline["protocol"]["chunk_size"], 8_192);
    assert_eq!(baseline["protocol"]["render_frames"], 30);
    assert_eq!(baseline["protocol"]["idle_ms"], 1_000);

    assert_workload(
        &baseline,
        "ansi-scroll-query",
        [4_444_069, 2_321, 328, 52_297_728, 44_986_368, 235],
        [4_888_476, 2_088, 311, 49_682_841],
    );
    assert_workload(
        &baseline,
        "plain-scroll",
        [3_806_866, 2_562, 373, 52_027_392, 45_322_240, 275],
        [5_242_880, 2_305, 354, 49_426_022],
    );
    assert_workload(
        &baseline,
        "ansi-scroll",
        [3_518_809, 3_016, 430, 52_203_520, 45_039_616, 297],
        [3_870_690, 2_714, 408, 49_593_344],
    );

    let build = &baseline["build"];
    assert_eq!(build["baseline"]["clean_check_ms"], 51_692);
    assert_eq!(build["baseline"]["warm_check_ms"], 776);
    assert_eq!(build["baseline"]["package_rebuild_ms"], 18_101);
    assert_eq!(build["baseline"]["test_no_run_ms"], 94_736);
    assert_eq!(build["baseline"]["target_bytes"], 7_252_825_489_u64);
    assert_eq!(build["baseline"]["largest_harness_bytes"], 74_248_192);
    assert_eq!(build["baseline"]["unit_execution_ms"], 18_737);
    assert_eq!(build["baseline"]["release_executable_bytes"], 28_459_520);
    assert_eq!(build["gates"]["clean_check_ms_max"], 41_400);
    assert_eq!(build["gates"]["package_rebuild_ms_max"], 12_700);
    assert_eq!(build["gates"]["test_no_run_ms_max"], 66_300);
    assert_eq!(build["gates"]["target_bytes_max"], 5_439_619_116_u64);
    assert_eq!(build["gates"]["largest_harness_bytes_max"], 55_700_000);
    assert_eq!(build["gates"]["unit_execution_ms_max"], 15_000);
    assert_eq!(build["gates"]["release_executable_bytes_max"], 25_620_000);

    for command in baseline["commands"]
        .as_object()
        .expect("commands must be an object")
        .values()
    {
        assert_non_empty_string(command, "command fingerprint");
    }
}

#[test]
#[cfg(target_os = "windows")]
fn scorecard_runners_validate_the_checked_in_contract_without_running_benchmarks() {
    for script in [
        "scripts/perf/runtime-scorecard.ps1",
        "scripts/perf/build-scorecard.ps1",
    ] {
        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-File",
                repo_path(script).to_str().expect("UTF-8 script path"),
                "-ValidationOnly",
            ])
            .output()
            .unwrap_or_else(|error| panic!("execute {script}: {error}"));
        assert!(
            output.status.success(),
            "{script} validation failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("parse {script} validation output: {error}"));
        assert_eq!(result["ok"], true);
        assert_eq!(result["mode"], "validation-only");
        assert_eq!(result["schema_version"], 1);
        assert_eq!(
            result["baseline_commit"],
            "9e99ba755fbdbd8896d849bc8934e187ab9062b5"
        );
    }
}

#[test]
fn ssh_gui_absolute_startup_gate_is_isolated_to_the_fixed_release_runner() {
    let release = read_repo_file(".github/workflows/release.yml");
    let pull_request_ci = read_repo_file(".github/workflows/ci.yml");
    let fixed_performance = release
        .split("  fixed-performance:\n")
        .nth(1)
        .expect("release workflow fixed-performance job")
        .split("\n  build-package:")
        .next()
        .expect("fixed-performance job boundary");
    let comparison = read_repo_file("scripts/ci/run-rterm-release-comparison.ps1");
    for argument in ["-Profile release", "run-ssh-gui-startup.ps1", "-SkipBuild"] {
        assert!(
            comparison.contains(argument),
            "release comparison is missing the absolute startup gate contract {argument}"
        );
    }
    for argument in ["-Warmups 5", "-Samples 40"] {
        assert!(
            fixed_performance.contains(argument),
            "fixed runner release comparison is missing {argument}"
        );
    }
    assert!(
        !pull_request_ci.contains("run-ssh-gui-startup.ps1"),
        "shared PR CI must not enforce machine-specific absolute startup budgets"
    );
}

#[test]
fn rterm_release_comparison_is_protected_structured_and_fixed_runner_only() {
    let release = read_repo_file(".github/workflows/release.yml");
    let pull_request_ci = read_repo_file(".github/workflows/ci.yml");
    let script = read_repo_file("scripts/ci/run-rterm-release-comparison.ps1");
    let fixed_performance = release
        .split("  fixed-performance:\n")
        .nth(1)
        .expect("release workflow fixed-performance job")
        .split("\n  build-package:")
        .next()
        .expect("fixed-performance job boundary");

    for contract in [
        "[int] $Warmups = 5",
        "[int] $Samples = 40",
        "[double] $RelativeRegressionCeiling = 0.05",
        "git clone --no-local",
        "candidate-target",
        "rollback-target",
        "run-ssh-gui-startup.ps1",
        "package-native.ps1",
        "package-smoke.ps1",
        "production-gui",
        "first_present_p95_ratio",
        "private_bytes_p95_ratio",
        "machine fingerprint mismatch",
        "threshold_violations",
    ] {
        assert!(
            script.contains(contract),
            "comparison script is missing {contract}"
        );
    }
    for contract in [
        "fetch-depth: 0",
        "scripts/ci/run-rterm-release-comparison.ps1",
        "-Warmups 5",
        "-Samples 40",
        "artifacts/rterm-release-comparison/report.json",
        "if-no-files-found: error",
    ] {
        assert!(
            fixed_performance.contains(contract),
            "fixed release runner is missing {contract}"
        );
    }
    assert!(
        !pull_request_ci.contains("run-rterm-release-comparison.ps1"),
        "shared PR CI must not run the fixed-machine release comparison"
    );
}

#[test]
#[cfg(target_os = "windows")]
fn rterm_release_comparison_validates_ratio_boundaries_without_building() {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-File",
            repo_path("scripts/ci/run-rterm-release-comparison.ps1")
                .to_str()
                .expect("UTF-8 script path"),
            "-ValidationOnly",
        ])
        .output()
        .expect("run release comparison validation");
    assert!(
        output.status.success(),
        "validation failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("validation JSON");
    assert_eq!(report["ok"], true);
    assert_eq!(report["mode"], "validation-only");
    assert_eq!(report["relative_regression_ceiling"], 0.05);
}

#[test]
fn process_harness_timestamps_startup_before_resuming_the_child() {
    let harness = read_repo_file("scripts/ci/process-harness.ps1");
    let timestamp_offset = harness
        .find("long resumeTimestamp = Stopwatch.GetTimestamp();")
        .expect("startup resume timestamp capture");
    let resume_offset = harness
        .find("if (ResumeThread(processInformation.Thread) == UInt32.MaxValue)")
        .expect("suspended child resume call");

    assert!(
        timestamp_offset < resume_offset,
        "taking the timestamp after ResumeThread lets the child run before timing begins"
    );
}

#[test]
fn ssh_gui_startup_runner_validates_marker_fields_and_metrics_consistency() {
    let runner = read_repo_file("scripts/ci/run-ssh-gui-startup.ps1");

    for contract in [
        "$requiredMarkerFields",
        "$requiredMemoryMarkerFields",
        "$markerFields.ContainsKey($requiredField)",
        "$memoryMarkerFields.ContainsKey($requiredField)",
        "[double]::TryParse",
        "[UInt64]::TryParse",
        "$reportedProcessToFirstPresentMs -le 0",
        "$privateBytes -eq 0",
        "$renderer -ne \"cpu\"",
        "$metrics.process_to_first_present_ms -ne $reportedProcessToFirstPresentMs",
        "$metrics.first_frame_private_bytes -ne $privateBytes",
        "$metrics.final_renderer -ne $renderer",
        "if ($line.StartsWith(\"first_present \"",
        "if ($line.StartsWith(\"first_frame_memory \"",
    ] {
        assert!(
            runner.contains(contract),
            "startup runner is missing marker validation contract: {contract}"
        );
    }
}

#[test]
fn ssh_gui_release_startup_gate_uses_a_benchmark_only_100_percent_dpi_override() {
    let runner = read_repo_file("scripts/ci/run-ssh-gui-startup.ps1");
    let window_hooks = read_repo_file("crates/rssh-app/src/window_parts/part15.rs");
    let window_startup = read_repo_file("crates/rssh-app/src/window_parts/part08.rs");

    assert!(runner.contains("$env:RSSH_BENCHMARK_WINDOW_SCALE_FACTOR = \"1\""));
    assert!(runner.contains("$previousBenchmarkScale"));
    assert!(
        window_hooks.contains("fn benchmark_window_scale_factor(benchmark_startup: bool)"),
        "the release binary needs a scale override that is gated by --benchmark-startup"
    );
    assert!(
        window_startup.contains("startup_window_scale_factor("),
        "window creation must apply the benchmark-only scale override"
    );
    assert!(
        window_hooks
            .split("WindowEvent::ScaleFactorChanged")
            .nth(1)
            .is_some_and(|handler| handler.contains("startup_window_scale_factor(")),
        "scale-change events must not replace the fixed benchmark scale with the host DPI"
    );
}

#[test]
fn first_present_marker_precedes_private_bytes_sampling() {
    let metrics = read_repo_file("crates/rssh-app/src/startup_metrics.rs");
    let window_metrics = read_repo_file("crates/rssh-app/src/window_parts/part07.rs");
    let presentation = read_repo_file("crates/rssh-app/src/window_parts/diagnostics.rs");

    assert!(metrics.contains("first_frame_memory first_frame_private_bytes="));
    assert!(window_metrics.contains("fn record_first_present(&mut self, renderer: RendererKind)"));
    assert!(
        window_metrics
            .contains("fn record_first_frame_private_bytes(&mut self, private_bytes: u64)")
    );

    for renderer in ["RendererKind::Cpu", "RendererKind::Gpu"] {
        let marker = format!("record_first_present({renderer})");
        let marker_offset = presentation
            .find(&marker)
            .unwrap_or_else(|| panic!("missing {renderer} first-present marker"));
        let memory_offset = presentation[marker_offset..]
            .find("current_process_private_bytes()")
            .map_or_else(
                || panic!("missing {renderer} Private Bytes sample"),
                |offset| marker_offset + offset,
            );
        assert!(
            marker_offset < memory_offset,
            "{renderer} must emit first_present before scanning process memory"
        );
    }
}

#[test]
fn readme_documents_the_supported_ssh_gui_contract() {
    let readme = read_repo_file("README.md");

    for contract in [
        "ssh --gui",
        "--renderer auto",
        "--renderer cpu",
        "--renderer gpu",
        "--benchmark-startup",
        "gui = true",
        "renderer = \"auto\"",
        "host_key_policy = \"prompt\"",
        "GUI SSH does not support forwarding",
        "GUI SSH does not support `--no-shell`",
        "GUI SSH does not support OpenSSH passthrough options",
    ] {
        assert!(
            readme.contains(contract),
            "README is missing SSH GUI user contract: {contract}"
        );
    }
}

#[test]
fn window_metrics_json_schema_has_an_explicit_compatibility_inventory() {
    let tests = read_repo_file("crates/rssh-app/src/window_compat_tests/part04_tests.rs");

    for contract in [
        "LEGACY_WINDOW_METRICS_JSON_FIELDS",
        "STARTUP_WINDOW_METRICS_JSON_FIELDS",
        "window_metrics_json_preserves_legacy_and_startup_fields",
    ] {
        assert!(
            tests.contains(contract),
            "window metrics JSON compatibility test is missing: {contract}"
        );
    }
}

#[test]
fn ssh_profile_prompt_policy_has_a_dedicated_mapping_test() {
    let profiles = read_repo_file("crates/rssh-app/src/profiles.rs");

    assert!(
        profiles.contains("gui_ssh_profile_prompt_policy_maps_to_interactive_verification"),
        "profiles must pin host_key_policy=prompt behavior for GUI SSH"
    );
}

#[test]
fn stage0_shared_ci_runs_deterministic_tests_without_absolute_memory_gates() {
    let ci = read_repo_file(".github/workflows/ci.yml");

    assert!(ci.contains("cargo test --locked -p rssh-diagnostics --all-targets -j1"));
    assert!(!ci.contains("run-stage0-diagnostics.ps1"));
    assert!(!ci.contains("stage0_empty_window_target_bytes"));
    assert!(!ci.contains("stage0_ssh1_target_bytes"));
}

#[test]
fn stage0_fixed_runner_collects_both_scenarios_and_uploads_raw_and_aggregate_json() {
    let release = read_repo_file(".github/workflows/release.yml");
    let fixed_performance = release
        .split("  fixed-performance:\n")
        .nth(1)
        .expect("release workflow fixed-performance job")
        .split("\n  build-package:")
        .next()
        .expect("fixed-performance job boundary");

    for contract in [
        "cargo build --locked --release -p rssh-app",
        "cargo build --locked --release -p rssh-diagnostics --bin rssh-bench-launcher",
        "scripts/ci/run-stage0-diagnostics.ps1",
        "-Profile release",
        "-Warmups 5",
        "-Samples 30",
        "-SkipBuild",
        "stage0-diagnostics/raw",
        "stage0-diagnostics/aggregate.json",
        "actions/upload-artifact@",
    ] {
        assert!(
            fixed_performance.contains(contract),
            "fixed runner is missing Stage 0 contract: {contract}"
        );
    }

    let runner = read_repo_file("scripts/ci/run-stage0-diagnostics.ps1");
    for contract in [
        "empty-window",
        "ssh1",
        "$Warmups = 5",
        "$Samples = 30",
        "--cols",
        "80",
        "--rows",
        "24",
        "RSSH_BENCHMARK_WINDOW_SCALE_FACTOR",
        "stage0_empty_window_target_bytes",
        "47185920",
        "stage0_ssh1_target_bytes",
        "62914560",
        "Write-Warning",
    ] {
        assert!(
            runner.contains(contract),
            "Stage 0 runner is missing: {contract}"
        );
    }
    assert!(
        !runner.contains("throw \"Stage 0 memory target"),
        "45/60 MiB targets must remain report-only during Stage 0"
    );
}

#[test]
fn stage0_documentation_freezes_metric_semantics_schema_and_gate_status() {
    let documentation = read_repo_file("docs/benchmarks/stage0-schema-v2.md");
    for contract in [
        "rssh.diagnostics/v2",
        "windows_private_working_set_bytes",
        "linux_pss_bytes",
        "macos_phys_footprint_bytes",
        "45 MiB",
        "60 MiB",
        "report-only",
        "first-present p95",
        "500 ms",
        "raw/",
        "aggregate.json",
        "unsupported",
    ] {
        assert!(
            documentation.contains(contract),
            "Stage 0 documentation is missing: {contract}"
        );
    }
    let readme = read_repo_file("README.md");
    assert!(readme.contains("run-stage0-diagnostics"));
    assert!(readme.contains("stage0-schema-v2.md"));
}

#[test]
fn stage4_snapshot_cache_gate_runs_only_on_the_fixed_release_runner() {
    let release = read_repo_file(".github/workflows/release.yml");
    let pull_request_ci = read_repo_file(".github/workflows/ci.yml");
    let runner = read_repo_file("scripts/ci/run-stage4-snapshot-cache.ps1");
    let core_manifest = read_repo_file("crates/rterm-render-core/Cargo.toml");

    for contract in [
        "scripts/ci/run-stage4-snapshot-cache.ps1",
        "-Profile release",
        "-Stage0Aggregate artifacts/stage0-diagnostics/aggregate.json",
        "stage4-snapshot-cache-windows-x64",
    ] {
        assert!(
            release.contains(contract),
            "release workflow is missing {contract}"
        );
    }
    assert!(!pull_request_ci.contains("run-stage4-snapshot-cache.ps1"));
    for contract in [
        "cargo bench --locked -p rterm-render-core --bench snapshot_memory",
        "ansi-scroll-query",
        "$parserBaseline * 0.98",
        "empty-window",
        "ssh1",
        "Stage 4 memory trend must be downward",
    ] {
        assert!(
            runner.contains(contract),
            "Stage 4 runner is missing {contract}"
        );
    }
    assert!(core_manifest.contains("name = \"snapshot_memory\""));
    assert!(core_manifest.contains("harness = false"));
}

fn assert_workload(baseline: &Value, name: &str, measured: [u64; 6], gates: [u64; 4]) {
    let workload = &baseline["runtime"]["workloads"][name];
    assert_eq!(
        workload["baseline"]["throughput_bytes_per_sec"],
        measured[0]
    );
    assert_eq!(workload["baseline"]["chunk_p95_us"], measured[1]);
    assert_eq!(workload["baseline"]["render_frame_p95_us"], measured[2]);
    assert_eq!(workload["baseline"]["process_memory_bytes"], measured[3]);
    assert_eq!(
        workload["baseline"]["process_virtual_memory_bytes"],
        measured[4]
    );
    assert_eq!(workload["baseline"]["elapsed_ms"], measured[5]);
    assert_eq!(workload["gates"]["throughput_bytes_per_sec_min"], gates[0]);
    assert_eq!(workload["gates"]["chunk_p95_us_max"], gates[1]);
    assert_eq!(workload["gates"]["render_frame_p95_us_max"], gates[2]);
    assert_eq!(workload["gates"]["process_memory_bytes_max"], gates[3]);
    assert_eq!(workload["gates"]["scrolled_survivor_cell_clones_max"], 0);
    assert_eq!(workload["gates"]["history_row_relocations_max"], 0);
}

fn assert_non_empty_string(value: &Value, path: &str) {
    assert!(
        value.as_str().is_some_and(|value| !value.trim().is_empty()),
        "{path} must be a non-empty string, got {value}"
    );
}

fn read_repo_json(path: &str) -> Value {
    let bytes = fs::read(repo_path(path)).unwrap_or_else(|error| {
        panic!("read checked-in scorecard {path}: {error}");
    });
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!("parse checked-in scorecard {path}: {error}");
    })
}

fn read_repo_file(path: &str) -> String {
    fs::read_to_string(repo_path(path)).unwrap_or_else(|error| {
        panic!("read repository file {path}: {error}");
    })
}

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .join(path)
}
