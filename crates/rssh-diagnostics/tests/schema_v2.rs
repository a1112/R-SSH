use rssh_diagnostics::{
    ConnectionState, DiagnosticGpuBackend, DiagnosticRendererMode, DiagnosticsResult, MemoryMetric,
    Platform, RendererKind, RunConfiguration, RunIdentity, Scenario, SchemaVersion,
    StartupMilestones,
};

#[test]
fn diagnostic_renderer_mode_exposes_stable_cli_value() {
    assert_eq!(DiagnosticRendererMode::Auto.as_str(), "auto");
}

#[test]
fn diagnostic_gpu_backend_parses_supported_cli_value() {
    assert_eq!(
        "dx12".parse::<DiagnosticGpuBackend>().unwrap(),
        DiagnosticGpuBackend::Dx12
    );
}

#[test]
fn diagnostic_gpu_backend_rejects_unsupported_cli_value() {
    assert!("metal".parse::<DiagnosticGpuBackend>().is_err());
}

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
fn schema_v2_decodes_legacy_results_without_backend_identity() {
    let result = DiagnosticsResult::successful_fixture(
        RunIdentity::fixture(Scenario::EmptyWindow, Platform::Windows),
        MemoryMetric::WindowsPrivateWorkingSetBytes,
        RunConfiguration::default(),
    );
    let mut legacy = serde_json::to_value(result).unwrap();
    let configuration = legacy["configuration"].as_object_mut().unwrap();
    configuration.remove("requested_renderer");
    configuration.remove("requested_gpu_backend");
    let renderer = legacy["renderer"].as_object_mut().unwrap();
    renderer.remove("backend");
    renderer.remove("adapter_name");
    renderer.remove("adapter_vendor_id");
    renderer.remove("adapter_device_id");
    renderer.remove("adapter_type");

    let decoded: DiagnosticsResult = serde_json::from_value(legacy).unwrap();

    assert_eq!(
        decoded.configuration.requested_renderer,
        DiagnosticRendererMode::Auto
    );
    assert_eq!(decoded.configuration.requested_gpu_backend, None);
    assert_eq!(decoded.renderer.backend, None);
    assert_eq!(decoded.renderer.adapter_name, None);
    assert_eq!(decoded.renderer.adapter_vendor_id, None);
    assert_eq!(decoded.renderer.adapter_device_id, None);
    assert_eq!(decoded.renderer.adapter_type, None);
}

#[test]
fn schema_v2_serializes_requested_and_selected_gpu_backend() {
    let configuration = RunConfiguration {
        requested_renderer: DiagnosticRendererMode::Auto,
        requested_gpu_backend: Some(DiagnosticGpuBackend::Dx12),
        ..RunConfiguration::default()
    };
    let mut result = DiagnosticsResult::successful_fixture(
        RunIdentity::fixture(Scenario::EmptyWindow, Platform::Windows),
        MemoryMetric::WindowsPrivateWorkingSetBytes,
        configuration,
    );
    result.renderer.backend = Some(DiagnosticGpuBackend::Dx12);
    result.renderer.adapter_name = Some("fixture-adapter".to_owned());
    result.renderer.adapter_vendor_id = Some(0x10de);
    result.renderer.adapter_device_id = Some(0x1234);
    result.renderer.adapter_type = Some("discrete_gpu".to_owned());

    let value = serde_json::to_value(result).unwrap();

    assert_eq!(value["configuration"]["requested_renderer"], "auto");
    assert_eq!(value["configuration"]["requested_gpu_backend"], "dx12");
    assert_eq!(value["renderer"]["backend"], "dx12");
    assert_eq!(value["renderer"]["adapter_name"], "fixture-adapter");
    assert_eq!(value["renderer"]["adapter_vendor_id"], 0x10de);
    assert_eq!(value["renderer"]["adapter_device_id"], 0x1234);
    assert_eq!(value["renderer"]["adapter_type"], "discrete_gpu");
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
