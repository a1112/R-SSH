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

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .join(path)
}
