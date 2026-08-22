use rssh_diagnostics::{
    MarkerCollector, MarkerDisposition, MarkerError, MarkerIdentity, MarkerKind, RendererKind,
    Scenario,
};

const FIRST_PRESENT: &str = concat!(
    "rssh_diagnostic ",
    r#"{"schema":"rssh.diagnostics/v2","run_id":"r1","pid":42,"scenario":"empty_window","kind":"first_present","elapsed_ms":12,"renderer":"cpu"}"#
);

const GPU_READY: &str = concat!(
    "rssh_diagnostic ",
    r#"{"schema":"rssh.diagnostics/v2","run_id":"r1","pid":42,"scenario":"empty_window","kind":"gpu_ready","elapsed_ms":50,"renderer":"gpu"}"#
);

fn collector() -> MarkerCollector {
    MarkerCollector::new(MarkerIdentity::new("r1", 42, Scenario::EmptyWindow))
}

#[test]
fn parser_ignores_plain_output_and_accepts_cpu_first_gpu_later() {
    let mut collector = collector();

    assert_eq!(
        collector.push_line("ordinary diagnostic").unwrap(),
        MarkerDisposition::Ignored
    );
    assert!(matches!(
        collector.push_line(FIRST_PRESENT).unwrap(),
        MarkerDisposition::Accepted(record) if record.kind == MarkerKind::FirstPresent
    ));
    assert!(matches!(
        collector.push_line(GPU_READY).unwrap(),
        MarkerDisposition::Accepted(record) if record.kind == MarkerKind::GpuReady
    ));

    let trace = collector.trace();
    assert_eq!(trace.milestones.first_present_ms, Some(12));
    assert_eq!(trace.milestones.gpu_ready_ms, Some(50));
    assert_eq!(trace.first_renderer, Some(RendererKind::Cpu));
    assert_eq!(trace.final_renderer, Some(RendererKind::Gpu));
}

#[test]
fn gpu_ready_collects_selected_backend_and_adapter_identity() {
    let gpu_ready = concat!(
        "rssh_diagnostic ",
        r#"{"schema":"rssh.diagnostics/v2","run_id":"r1","pid":42,"scenario":"empty_window","kind":"gpu_ready","elapsed_ms":50,"renderer":"gpu","gpu_backend":"Dx12","gpu_adapter_name":"fixture-adapter","gpu_adapter_vendor_id":4318,"gpu_adapter_device_id":9860,"gpu_adapter_type":"discrete-gpu"}"#
    );
    let mut collector = collector();

    collector.push_line(gpu_ready).unwrap();

    let trace = collector.trace();
    assert_eq!(
        trace.gpu_backend,
        Some(rssh_diagnostics::DiagnosticGpuBackend::Dx12)
    );
    assert_eq!(trace.gpu_adapter_name.as_deref(), Some("fixture-adapter"));
    assert_eq!(trace.gpu_adapter_vendor_id, Some(4318));
    assert_eq!(trace.gpu_adapter_device_id, Some(9860));
    assert_eq!(trace.gpu_adapter_type.as_deref(), Some("discrete-gpu"));
}

#[test]
fn missing_or_malformed_gpu_identity_preserves_gpu_ready_marker_semantics() {
    let malformed_gpu_ready = concat!(
        "rssh_diagnostic ",
        r#"{"schema":"rssh.diagnostics/v2","run_id":"r1","pid":42,"scenario":"empty_window","kind":"gpu_ready","elapsed_ms":50,"renderer":"gpu","gpu_backend":7,"gpu_adapter_name":false,"gpu_adapter_vendor_id":"4318","gpu_adapter_device_id":-1,"gpu_adapter_type":{"unexpected":true}}"#
    );
    let mut malformed = collector();

    malformed.push_line(malformed_gpu_ready).unwrap();

    let trace = malformed.trace();
    assert_eq!(trace.milestones.gpu_ready_ms, Some(50));
    assert_eq!(trace.final_renderer, Some(RendererKind::Gpu));
    assert_eq!(trace.gpu_backend, None);
    assert_eq!(trace.gpu_adapter_name, None);
    assert_eq!(trace.gpu_adapter_vendor_id, None);
    assert_eq!(trace.gpu_adapter_device_id, None);
    assert_eq!(trace.gpu_adapter_type, None);

    let mut legacy = collector();
    legacy.push_line(GPU_READY).unwrap();
    let trace = legacy.trace();
    assert_eq!(trace.milestones.gpu_ready_ms, Some(50));
    assert_eq!(trace.final_renderer, Some(RendererKind::Gpu));
    assert_eq!(trace.gpu_backend, None);
    assert_eq!(trace.gpu_adapter_name, None);
    assert_eq!(trace.gpu_adapter_vendor_id, None);
    assert_eq!(trace.gpu_adapter_device_id, None);
    assert_eq!(trace.gpu_adapter_type, None);
}

#[test]
fn unknown_backend_and_overflowing_adapter_ids_are_ignored_independently() {
    let gpu_ready = concat!(
        "rssh_diagnostic ",
        r#"{"schema":"rssh.diagnostics/v2","run_id":"r1","pid":42,"scenario":"empty_window","kind":"gpu_ready","elapsed_ms":50,"renderer":"gpu","gpu_backend":"future-backend","gpu_adapter_name":"fixture-adapter","gpu_adapter_vendor_id":4294967296,"gpu_adapter_device_id":4294967296,"gpu_adapter_type":"discrete-gpu"}"#
    );
    let mut collector = collector();

    collector.push_line(gpu_ready).unwrap();

    let trace = collector.trace();
    assert_eq!(trace.milestones.gpu_ready_ms, Some(50));
    assert_eq!(trace.final_renderer, Some(RendererKind::Gpu));
    assert_eq!(trace.gpu_backend, None);
    assert_eq!(trace.gpu_adapter_vendor_id, None);
    assert_eq!(trace.gpu_adapter_device_id, None);
    assert_eq!(trace.gpu_adapter_name.as_deref(), Some("fixture-adapter"));
    assert_eq!(trace.gpu_adapter_type.as_deref(), Some("discrete-gpu"));
}

#[test]
fn malformed_prefixed_json_is_not_treated_as_plain_output() {
    let error = collector()
        .push_line("rssh_diagnostic {broken")
        .unwrap_err();

    assert!(matches!(error, MarkerError::Malformed { .. }));
}

#[test]
fn marker_identity_mismatch_rejects_cross_run_and_cross_process_lines() {
    let wrong_run = FIRST_PRESENT.replace("\"r1\"", "\"r2\"");
    assert!(matches!(
        collector().push_line(&wrong_run),
        Err(MarkerError::IdentityMismatch {
            field: "run_id",
            ..
        })
    ));

    let wrong_pid = FIRST_PRESENT.replace("\"pid\":42", "\"pid\":43");
    assert!(matches!(
        collector().push_line(&wrong_pid),
        Err(MarkerError::IdentityMismatch { field: "pid", .. })
    ));

    let wrong_scenario = FIRST_PRESENT.replace("empty_window", "ssh1");
    assert!(matches!(
        collector().push_line(&wrong_scenario),
        Err(MarkerError::IdentityMismatch {
            field: "scenario",
            ..
        })
    ));
}

#[test]
fn marker_timestamps_must_not_decrease() {
    let mut collector = collector();
    collector.push_line(GPU_READY).unwrap();

    let error = collector.push_line(FIRST_PRESENT).unwrap_err();
    assert_eq!(
        error,
        MarkerError::DecreasingElapsed {
            previous_ms: 50,
            observed_ms: 12,
        }
    );
}

#[test]
fn singleton_and_terminal_markers_reject_duplicates() {
    let mut collector = collector();
    collector.push_line(FIRST_PRESENT).unwrap();
    assert_eq!(
        collector.push_line(FIRST_PRESENT).unwrap_err(),
        MarkerError::Duplicate(MarkerKind::FirstPresent)
    );

    let exited = concat!(
        "rssh_diagnostic ",
        r#"{"schema":"rssh.diagnostics/v2","run_id":"r1","pid":42,"scenario":"empty_window","kind":"process_exited","elapsed_ms":60}"#
    );
    collector.push_line(exited).unwrap();
    assert_eq!(
        collector.push_line(exited).unwrap_err(),
        MarkerError::Duplicate(MarkerKind::ProcessExited)
    );
}

#[test]
fn selected_backend_marker_fields_are_preserved_for_result_enrichment() {
    let line = FIRST_PRESENT.replace(
        "\"renderer\":\"cpu\"",
        "\"renderer\":\"cpu\",\"backend\":\"dx12\",\"adapter_name\":\"fixture-adapter\",\"future_detail\":{\"value\":7}",
    );
    let disposition = collector().push_line(&line).unwrap();
    let MarkerDisposition::Accepted(record) = disposition else {
        panic!("marker was not accepted");
    };

    assert_eq!(record.extra["backend"], "dx12");
    assert_eq!(record.extra["adapter_name"], "fixture-adapter");
    assert_eq!(record.extra["future_detail"]["value"], 7);
}

#[test]
fn unknown_schema_and_marker_kind_are_protocol_errors() {
    let wrong_schema = FIRST_PRESENT.replace("rssh.diagnostics/v2", "rssh.diagnostics/v3");
    assert!(matches!(
        collector().push_line(&wrong_schema),
        Err(MarkerError::Malformed { .. })
    ));

    let wrong_kind = FIRST_PRESENT.replace("first_present", "future_kind");
    assert!(matches!(
        collector().push_line(&wrong_kind),
        Err(MarkerError::Malformed { .. })
    ));
}
