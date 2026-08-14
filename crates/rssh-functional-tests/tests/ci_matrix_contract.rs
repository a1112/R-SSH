use std::{collections::BTreeMap, fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn pr_functional_workflow_has_the_fixed_required_matrix_and_hard_deadlines() {
    let workflow = fs::read_to_string(root().join(".github/workflows/functional.yml")).unwrap();
    for contract in [
        "contract-catalog",
        "cli-transport",
        "native-windows",
        "native-x11",
        "native-wayland",
        "native-macos-hosted",
        "native-macos-accessibility",
        "web-browser",
        "tauri-platform",
        "production-package-smoke",
        "aggregate-evidence",
        "chromium",
        "firefox",
        "webkit",
        "timeout-minutes: 18",
    ] {
        assert!(
            workflow.contains(contract),
            "missing CI contract {contract}"
        );
    }
    assert!(!workflow.contains("continue-on-error: true"));
    assert!(!workflow.contains("retry"));
    assert!(workflow.contains("if-no-files-found: error"));
    assert!(workflow.contains("$PSNativeCommandUseErrorActionPreference = $true"));
    let upload_sha = "actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02";
    assert_eq!(
        workflow.matches("actions/upload-artifact@").count(),
        workflow.matches(upload_sha).count(),
        "every artifact upload must use the reviewed pinned action SHA"
    );
    assert_eq!(
        workflow.matches("actions/upload-artifact@").count(),
        workflow.matches("if: ${{ always() }}").count(),
        "every evidence upload must run after both success and failure"
    );
}

#[test]
fn cli_transport_matrix_executes_runner_owned_lpt_shards() {
    let workflow = fs::read_to_string(root().join(".github/workflows/functional.yml")).unwrap();
    assert!(
        workflow.contains("run-shard --count 2 --index"),
        "the PR CLI/transport matrix must execute stable runner-owned LPT shards"
    );
    assert!(workflow.contains("shard_index:"));
    assert!(workflow.contains("--surface console"));
    assert!(
        !workflow.contains("--scenario startup.version"),
        "hand-written scenario commands bypass LPT scheduling"
    );
}

#[test]
fn wayland_job_routes_xtest_through_a_nested_weston_x11_seat() {
    let workflow = fs::read_to_string(root().join(".github/workflows/functional.yml")).unwrap();
    assert!(workflow.contains("scripts/functional/run-wayland-seat.sh"));
    assert!(workflow.contains("RSSH_FUNCTIONAL_WESTON_BACKEND"));
    assert!(workflow.contains("weston"));
    assert!(workflow.contains("xvfb"));
}

#[test]
fn functional_workflow_has_no_expression_inside_an_inline_yaml_map() {
    let workflow = fs::read_to_string(root().join(".github/workflows/functional.yml")).unwrap();
    assert!(
        !workflow
            .lines()
            .any(|line| line.contains("with: {") && line.contains("${{")),
        "GitHub expressions must use block mappings so the workflow parses as YAML"
    );
}

#[test]
fn privileged_self_hosted_jobs_run_only_on_manual_dispatch() {
    let workflow = fs::read_to_string(root().join(".github/workflows/functional.yml")).unwrap();
    for (job, next_job) in [
        ("  native-macos-accessibility:", "  web-browser:"),
        ("  tauri-platform-macos:", "  production-package-smoke:"),
        (
            "  production-tauri-bundle-smoke-macos:",
            "  aggregate-evidence:",
        ),
    ] {
        let start = workflow.find(job).expect("privileged job is present");
        let end = workflow[start..]
            .find(next_job)
            .map_or(workflow.len(), |offset| start + offset);
        let definition = &workflow[start..end];
        assert!(
            definition.contains("self-hosted")
                && definition.contains("if: github.event_name == 'workflow_dispatch'")
                && !definition.contains("github.event.pull_request"),
            "privileged job {job} must run only on explicit manual dispatch"
        );
    }
}

#[test]
fn pull_requests_never_wait_for_privileged_self_hosted_macos_jobs() {
    let workflow = fs::read_to_string(root().join(".github/workflows/functional.yml")).unwrap();
    for (job, next_job) in [
        ("  native-macos-accessibility:", "  web-browser:"),
        ("  tauri-platform-macos:", "  production-package-smoke:"),
        (
            "  production-tauri-bundle-smoke-macos:",
            "  aggregate-evidence:",
        ),
    ] {
        let start = workflow.find(job).expect("privileged job is present");
        let end = start
            + workflow[start..]
                .find(next_job)
                .expect("following job is present");
        let definition = &workflow[start..end];
        assert!(definition.contains("if: github.event_name == 'workflow_dispatch'"));
        assert!(!definition.contains("github.event.pull_request"));
    }

    let pr_aggregate = workflow
        .split("  aggregate-evidence-pr:")
        .nth(1)
        .expect("hosted PR aggregate must be present");
    assert!(pr_aggregate.contains("if: github.event_name == 'pull_request'"));
    assert!(pr_aggregate.contains("functional-tests/hosted-matrix.toml"));
    let definition = pr_aggregate.split("  aggregate-evidence:").next().unwrap();
    assert!(!definition.contains("native-macos-accessibility"));
    assert!(!definition.contains("tauri-platform-macos"));
    assert!(!definition.contains("production-tauri-bundle-smoke-macos"));

    let full_aggregate = workflow
        .split("  aggregate-evidence:")
        .nth(1)
        .expect("manual full aggregate must be present");
    assert!(full_aggregate.contains("if: github.event_name == 'workflow_dispatch'"));
    assert!(full_aggregate.contains("functional-tests/matrix.toml"));
}

#[test]
fn pull_requests_keep_hosted_tauri_rows_and_an_exact_hosted_aggregate() {
    let workflow = fs::read_to_string(root().join(".github/workflows/functional.yml")).unwrap();
    for (job, next_job) in [
        ("  tauri-platform:", "  tauri-platform-macos:"),
        (
            "  production-tauri-bundle-smoke:",
            "  production-tauri-bundle-smoke-macos:",
        ),
    ] {
        let start = workflow.find(job).expect("hosted Tauri job is present");
        let end = start
            + workflow[start..]
                .find(next_job)
                .expect("next job is present");
        let definition = &workflow[start..end];
        assert!(!definition.contains("self-hosted"));
        assert!(!definition.contains("github.event.pull_request.head.repo.full_name"));
        assert!(definition.contains("windows-2025"));
        assert!(definition.contains("ubuntu-24.04"));
    }
    assert!(workflow.contains("  aggregate-evidence-pr:"));
    assert!(workflow.contains("functional-tests/hosted-matrix.toml"));
    assert!(workflow.contains("if: github.event_name == 'pull_request'"));
}

#[test]
fn hosted_matrix_is_the_full_matrix_without_privileged_macos_targets() {
    fn scenario_targets(document: &toml::Value) -> BTreeMap<String, Vec<String>> {
        document["scenario_runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|run| {
                let id = run["scenario_id"].as_str().unwrap().to_owned();
                let targets = run["targets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|target| target.as_str().unwrap().to_owned())
                    .collect();
                (id, targets)
            })
            .collect()
    }

    let full: toml::Value =
        toml::from_str(&fs::read_to_string(root().join("functional-tests/matrix.toml")).unwrap())
            .unwrap();
    let hosted: toml::Value = toml::from_str(
        &fs::read_to_string(root().join("functional-tests/hosted-matrix.toml")).unwrap(),
    )
    .unwrap();
    let full_targets = scenario_targets(&full);
    let hosted_targets = scenario_targets(&hosted);
    assert_eq!(
        full_targets.keys().collect::<Vec<_>>(),
        hosted_targets.keys().collect::<Vec<_>>()
    );
    for (scenario, targets) in &hosted_targets {
        assert!(
            targets
                .iter()
                .all(|target| { target != "macos-accessibility" && target != "macos-terminalapp" })
        );
        assert!(
            targets
                .iter()
                .all(|target| full_targets[scenario].contains(target))
        );
    }
    assert_eq!(full["playwright_runs"], hosted["playwright_runs"]);
}
