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
fn privileged_self_hosted_jobs_reject_untrusted_fork_pull_requests() {
    let workflow = fs::read_to_string(root().join(".github/workflows/functional.yml")).unwrap();
    let trusted_source = "github.event_name == 'workflow_dispatch' || github.event.pull_request.head.repo.full_name == github.repository";
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
            definition.contains("self-hosted") && definition.contains(trusted_source),
            "privileged job {job} must reject code from untrusted fork pull requests"
        );
    }
}

#[test]
fn fork_pull_requests_keep_hosted_tauri_rows_and_an_exact_safe_aggregate() {
    let workflow = fs::read_to_string(root().join(".github/workflows/functional.yml")).unwrap();
    let trusted_source = "github.event_name == 'workflow_dispatch' || github.event.pull_request.head.repo.full_name == github.repository";
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
        assert!(!definition.contains(trusted_source));
        assert!(definition.contains("windows-2025"));
        assert!(definition.contains("ubuntu-24.04"));
    }
    assert!(workflow.contains("  aggregate-evidence-fork:"));
    assert!(workflow.contains("functional-tests/fork-matrix.toml"));
    assert!(workflow.contains(
        "github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name != github.repository"
    ));
}

#[test]
fn fork_matrix_is_the_full_matrix_without_privileged_macos_targets() {
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
    let fork: toml::Value = toml::from_str(
        &fs::read_to_string(root().join("functional-tests/fork-matrix.toml")).unwrap(),
    )
    .unwrap();
    let full_targets = scenario_targets(&full);
    let fork_targets = scenario_targets(&fork);
    assert_eq!(
        full_targets.keys().collect::<Vec<_>>(),
        fork_targets.keys().collect::<Vec<_>>()
    );
    for (scenario, targets) in &fork_targets {
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
    assert_eq!(full["playwright_runs"], fork["playwright_runs"]);
}
