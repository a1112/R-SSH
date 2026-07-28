use rssh_renderer::gpu::{
    GpuContext, GpuContextOptions, SurfaceFault, SurfaceRecovery, SurfaceRecoveryState,
};
use std::time::Duration;

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
    let context = pollster::block_on(GpuContext::new_headless(GpuContextOptions::default()))
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
