use std::path::PathBuf;
use std::time::Duration;

use rssh_diagnostics::{
    LauncherFailureCode, LauncherOptions, LauncherPhase, LauncherStateMachine, MarkerKind,
    RunConfiguration, Scenario,
};

fn existing_app() -> String {
    std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn base_args(scenario: &str) -> Vec<String> {
    vec![
        "rssh-bench-launcher".to_owned(),
        "--app".to_owned(),
        existing_app(),
        "--scenario".to_owned(),
        scenario.to_owned(),
        "--json".to_owned(),
    ]
}

#[test]
fn defaults_encode_the_approved_sampling_contract() {
    let options = LauncherOptions::parse(base_args("empty-window")).unwrap();

    assert_eq!(options.scenario, Scenario::EmptyWindow);
    assert_eq!(options.stabilization, Duration::from_millis(5_000));
    assert_eq!(options.sample_interval, Duration::from_millis(100));
    assert_eq!(options.sample_count, 10);
    assert_eq!(options.shutdown_timeout, Duration::from_millis(2_000));
    assert_eq!(options.columns, 80);
    assert_eq!(options.rows, 24);
    assert!(options.json);
}

#[test]
fn parser_accepts_ssh1_and_explicit_sampling_values() {
    let mut args = base_args("ssh1");
    args.extend([
        "--stabilization-ms".to_owned(),
        "25".to_owned(),
        "--sample-interval-ms".to_owned(),
        "5".to_owned(),
        "--sample-count".to_owned(),
        "3".to_owned(),
    ]);

    let options = LauncherOptions::parse(args).unwrap();
    assert_eq!(options.scenario, Scenario::Ssh1);
    assert_eq!(options.stabilization, Duration::from_millis(25));
    assert_eq!(options.sample_interval, Duration::from_millis(5));
    assert_eq!(options.sample_count, 3);
}

#[test]
fn parser_rejects_missing_app_zero_values_and_unknown_arguments() {
    let missing_app = LauncherOptions::parse(vec![
        "rssh-bench-launcher".to_owned(),
        "--scenario".to_owned(),
        "empty-window".to_owned(),
        "--json".to_owned(),
    ])
    .unwrap_err();
    assert!(missing_app.to_string().contains("--app"));

    let mut zero = base_args("empty-window");
    zero.extend(["--sample-count".to_owned(), "0".to_owned()]);
    assert!(
        LauncherOptions::parse(zero)
            .unwrap_err()
            .to_string()
            .contains("positive")
    );

    let mut unknown = base_args("empty-window");
    unknown.push("--surprise".to_owned());
    assert!(
        LauncherOptions::parse(unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown argument")
    );

    let nonexistent = PathBuf::from("definitely-not-an-rssh-app-binary");
    let mut absent = base_args("empty-window");
    absent[2] = nonexistent.to_string_lossy().into_owned();
    assert!(
        LauncherOptions::parse(absent)
            .unwrap_err()
            .to_string()
            .contains("does not exist")
    );
}

#[test]
fn readiness_stabilization_and_sampling_follow_exact_deadlines() {
    let configuration = RunConfiguration {
        stabilization_ms: 5_000,
        sample_interval_ms: 100,
        sample_count: 3,
        ..RunConfiguration::default()
    };
    let mut state = LauncherStateMachine::new(configuration);

    state.child_started(42).unwrap();
    assert_eq!(state.phase(), LauncherPhase::AwaitMarkers);
    state.observe_marker(MarkerKind::FirstPresent, 80).unwrap();
    assert_eq!(state.phase(), LauncherPhase::AwaitScenarioReady);
    state
        .observe_marker(MarkerKind::ScenarioReady, 100)
        .unwrap();
    assert_eq!(state.phase(), LauncherPhase::Stabilize);
    assert_eq!(state.next_deadline_ms(), Some(5_100));

    state.advance_to(5_099).unwrap();
    assert_eq!(state.phase(), LauncherPhase::Stabilize);
    state.advance_to(5_100).unwrap();
    assert_eq!(state.phase(), LauncherPhase::Sample);
    assert_eq!(state.next_deadline_ms(), Some(5_100));

    state.record_sample(5_100, 10).unwrap();
    assert_eq!(state.next_deadline_ms(), Some(5_200));
    state.record_sample(5_200, 20).unwrap();
    state.record_sample(5_300, 30).unwrap();
    assert_eq!(state.phase(), LauncherPhase::RequestShutdown);
    assert_eq!(state.sample_bytes(), &[10, 20, 30]);
}

#[test]
fn early_child_exit_is_a_structured_terminal_failure() {
    let mut state = LauncherStateMachine::new(RunConfiguration::default());
    state.child_started(42).unwrap();
    state.observe_marker(MarkerKind::FirstPresent, 10).unwrap();

    let failure = state.child_exited(Some(7), 11).unwrap_err();
    assert_eq!(failure.code, LauncherFailureCode::ChildExitedEarly);
    assert_eq!(failure.phase, LauncherPhase::AwaitScenarioReady);
    assert_eq!(state.phase(), LauncherPhase::Failed);
}

#[test]
fn shutdown_is_graceful_first_and_can_escalate_to_forced_reap() {
    let configuration = RunConfiguration {
        stabilization_ms: 1,
        sample_interval_ms: 1,
        sample_count: 1,
        ..RunConfiguration::default()
    };
    let mut state = LauncherStateMachine::new(configuration);
    state.child_started(42).unwrap();
    state.observe_marker(MarkerKind::ScenarioReady, 1).unwrap();
    state.advance_to(2).unwrap();
    state.record_sample(2, 10).unwrap();

    state.graceful_shutdown_requested(3).unwrap();
    assert_eq!(state.phase(), LauncherPhase::Reap);
    assert!(!state.forced_shutdown());
    state.force_shutdown(4).unwrap();
    assert!(state.forced_shutdown());
    state.child_reaped(Some(0), 5).unwrap();
    assert_eq!(state.phase(), LauncherPhase::EmitResult);
}
