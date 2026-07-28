use rssh_renderer::gpu::{
    GpuContext, GpuContextOptions, SurfaceFault, SurfaceRecovery, SurfaceRecoveryState,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    time::Duration,
};

#[test]
fn surface_fault_policy_reconfigures_lost_and_outdated_surfaces() {
    assert_eq!(
        SurfaceRecovery::for_fault(SurfaceFault::Outdated),
        SurfaceRecovery::ReconfigureAndRetry
    );
    assert_eq!(
        SurfaceRecovery::for_fault(SurfaceFault::Lost),
        SurfaceRecovery::RecreateAndRetry
    );
    assert_eq!(
        SurfaceRecovery::for_fault(SurfaceFault::Timeout),
        SurfaceRecovery::SkipFrame
    );
    assert_eq!(
        SurfaceRecovery::for_fault(SurfaceFault::Occluded),
        SurfaceRecovery::SkipFrame
    );
    assert_eq!(
        SurfaceRecovery::for_fault(SurfaceFault::Validation),
        SurfaceRecovery::Report
    );
}

#[test]
fn surface_recovery_state_allows_only_one_reconfigure_or_recreate_retry() {
    let mut outdated = SurfaceRecoveryState::new();
    assert_eq!(
        outdated.action(SurfaceFault::Outdated),
        SurfaceRecovery::ReconfigureAndRetry
    );
    assert_eq!(
        outdated.action(SurfaceFault::Outdated),
        SurfaceRecovery::Report
    );

    let mut lost = SurfaceRecoveryState::new();
    assert_eq!(
        lost.action(SurfaceFault::Lost),
        SurfaceRecovery::RecreateAndRetry
    );
    assert_eq!(lost.action(SurfaceFault::Lost), SurfaceRecovery::Report);
}

#[test]
fn headless_context_reports_the_selected_adapter() {
    let mut context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("the native backend or its software fallback must provide a headless adapter");
    let metrics = context.metrics();

    assert!(!metrics.backend.is_empty());
    assert!(!metrics.adapter_name.is_empty());
    assert!(!metrics.adapter_type.is_empty());
    assert_eq!(
        metrics.software_adapter,
        matches!(metrics.adapter_type.as_str(), "cpu")
    );
    assert!(metrics.surface_format.is_none());
    assert!(metrics.present_mode.is_none());
    assert!(metrics.surface_width.is_none());
    assert!(metrics.surface_height.is_none());
    assert_eq!(metrics.rendered_frames, 0);
    assert_eq!(metrics.presented_frames, 0);

    context
        .run_headless_submission_probe(Duration::from_secs(5))
        .expect("tiny write, submit, and bounded poll must complete");
}

#[test]
fn uncaptured_validation_is_reported_without_panicking_or_counting_a_frame() {
    let mut context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
        .expect("headless adapter");
    let invalid_size = context
        .device()
        .limits()
        .max_buffer_size
        .checked_add(1)
        .expect("native max buffer size leaves room for an invalid probe");

    let creation = catch_unwind(AssertUnwindSafe(|| {
        context.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("rssh-invalid-buffer-probe"),
            size: invalid_size,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }));
    assert!(
        creation.is_ok(),
        "registered uncaptured error handler must replace wgpu's default panic"
    );
    context
        .device()
        .poll(wgpu::PollType::Poll)
        .expect("validation probe poll");

    let error = context
        .run_headless_submission_probe(Duration::from_secs(5))
        .expect_err("pending uncaptured validation must stop later GPU work");
    assert_eq!(
        error.kind(),
        rssh_renderer::gpu::GpuContextErrorKind::Validation
    );
    assert_eq!(context.metrics().rendered_frames, 0);
    assert_eq!(context.metrics().presented_frames, 0);
    assert_eq!(context.metrics().uncaptured_errors, 1);
}
