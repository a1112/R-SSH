use rssh_diagnostics::{
    ConnectionState, DiagnosticsResult, MemoryMetric, Platform, RendererKind, RunConfiguration,
    RunIdentity, Scenario, SchemaVersion, StartupMilestones,
};

#[test]
fn schema_v2_serializes_stable_discriminators_and_optional_hybrid_milestones() {
    let result = DiagnosticsResult::successful_fixture(
        RunIdentity::fixture(Scenario::EmptyWindow, Platform::Windows),
        MemoryMetric::WindowsPrivateWorkingSetBytes,
        RunConfiguration::default(),
    );
    let value = serde_json::to_value(result).unwrap();

    assert_eq!(value["schema"], "rssh.diagnostics/v2");
    assert_eq!(value["run"]["scenario"], "empty_window");
    assert_eq!(
        value["memory"]["metric"],
        "windows_private_working_set_bytes"
    );
    assert!(value["milestones"]["gpu_ready_ms"].is_null());
    assert_eq!(value["renderer"]["first"], "cpu");
    assert_eq!(value["connection"]["final_state"], "not_started");
}

#[test]
fn schema_v2_accepts_cpu_first_and_gpu_ready_later() {
    let mut result = DiagnosticsResult::successful_fixture(
        RunIdentity::fixture(Scenario::EmptyWindow, Platform::Windows),
        MemoryMetric::WindowsPrivateWorkingSetBytes,
        RunConfiguration::default(),
    );
    result.milestones = StartupMilestones {
        process_started_ms: 0,
        first_present_ms: Some(10),
        gpu_ready_ms: Some(40),
        scenario_ready_ms: Some(10),
        sampling_started_ms: Some(5_010),
        sampling_finished_ms: Some(5_910),
        process_exited_ms: Some(6_000),
        ..StartupMilestones::default()
    };
    result.renderer.first = Some(RendererKind::Cpu);
    result.renderer.final_renderer = Some(RendererKind::Gpu);
    result.connection.final_state = ConnectionState::NotStarted;

    result.validate().unwrap();
    let json = serde_json::to_string(&result).unwrap();
    let decoded: DiagnosticsResult = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.schema, SchemaVersion::V2);
    assert_eq!(decoded.milestones.first_present_ms, Some(10));
    assert_eq!(decoded.milestones.gpu_ready_ms, Some(40));
}

#[test]
fn successful_schema_requires_the_configured_sample_count() {
    let mut result = DiagnosticsResult::successful_fixture(
        RunIdentity::fixture(Scenario::EmptyWindow, Platform::Linux),
        MemoryMetric::LinuxPssBytes,
        RunConfiguration::default(),
    );
    result.memory.samples.pop();

    let error = result.validate().unwrap_err();
    assert_eq!(error.to_string(), "expected 10 memory samples, observed 9");
}

#[test]
fn schema_version_rejects_unknown_wire_values() {
    let error = serde_json::from_str::<SchemaVersion>(r#""rssh.diagnostics/v3""#).unwrap_err();
    assert!(error.to_string().contains("rssh.diagnostics/v2"));
}
